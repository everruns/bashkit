"""Shared core of bashkit's `requests` and `httpx` modules.

Both talk to `_bashkit.http` directly (the host runs every request through
bashkit's egress pipeline), not through http.client/urllib3.

Decisions:
- Small, API-compatible subsets written for bashkit instead of the upstream
  packages: upstream `requests` pulls in 155 modules (urllib3, email,
  http.cookiejar, charset_normalizer) and costs ~3 s per call to import on
  Pulley. These modules use only what the snapshot already preloads, and
  are preloaded themselves, so importing them is free.
- Redirects are followed here, so every hop is a new request the host checks
  against the allowlist again. `Authorization` is dropped when the host
  changes.
- Responses are buffered by the host (its response cap applies); streaming
  APIs iterate over the buffered body.
- The host does not decompress (TM-NET-013); gzip/deflate bodies are
  decompressed here, inside the guest's memory limit.
- Cookies: a minimal per-host jar (name=value, no path/expiry handling).
"""

import base64
import json
import os
import zlib
from collections.abc import Mapping, MutableMapping
from urllib.parse import urlencode, urljoin, urlsplit, urlunsplit

import _bashkit

REDIRECT_CODES = (301, 302, 303, 307, 308)
MAX_REDIRECTS = 30


def reason_phrase(status):
    from http import HTTPStatus

    try:
        return HTTPStatus(status).phrase
    except ValueError:
        return ""


class CaseInsensitiveDict(MutableMapping):
    """Header mapping: case-insensitive keys, original case preserved."""

    def __init__(self, data=None, **kwargs):
        self._store = {}
        self.update(data or {}, **kwargs)

    def __setitem__(self, key, value):
        self._store[key.lower()] = (key, value)

    def __getitem__(self, key):
        return self._store[key.lower()][1]

    def __delitem__(self, key):
        del self._store[key.lower()]

    def __iter__(self):
        return (k for k, _ in self._store.values())

    def __len__(self):
        return len(self._store)

    def lower_items(self):
        return ((k, v[1]) for k, v in self._store.items())

    def __eq__(self, other):
        if isinstance(other, Mapping):
            other = CaseInsensitiveDict(other)
        else:
            return NotImplemented
        return dict(self.lower_items()) == dict(other.lower_items())

    def copy(self):
        return CaseInsensitiveDict(self._store.values())

    def __repr__(self):
        return str(dict(self.items()))


def header_items(headers):
    """Normalize a mapping or a list of pairs to a list of (str, str)."""
    if headers is None:
        return []
    items = headers.items() if hasattr(headers, "items") else headers
    out = []
    for k, v in items:
        if v is None:
            continue
        if isinstance(k, bytes):
            k = k.decode("latin-1")
        if isinstance(v, bytes):
            v = v.decode("latin-1")
        out.append((str(k), str(v)))
    return out


def merge_headers(base, extra):
    """Merge header lists; later names replace earlier ones (case-insensitive).
    A None value in `extra` removes the header."""
    merged = CaseInsensitiveDict()
    for k, v in base:
        merged[k] = v
    if extra is not None:
        items = extra.items() if hasattr(extra, "items") else extra
        for k, v in items:
            if v is None:
                merged.pop(k, None)
            else:
                merged[k] = v
    return header_items(merged)


def add_params(url, params):
    if not params:
        return url
    if isinstance(params, (str, bytes)):
        query = params.decode() if isinstance(params, bytes) else params
    else:
        items = params.items() if hasattr(params, "items") else params
        pairs = []
        for k, v in items:
            if v is None:
                continue
            if isinstance(v, (list, tuple)):
                pairs.extend((k, x) for x in v)
            else:
                pairs.append((k, v))
        query = urlencode(pairs)
    if not query:
        return url
    parts = urlsplit(url)
    joined = f"{parts.query}&{query}" if parts.query else query
    return urlunsplit(parts._replace(query=joined))


def encode_form(data):
    if isinstance(data, (str, bytes)):
        return data.encode() if isinstance(data, str) else data
    items = data.items() if hasattr(data, "items") else data
    pairs = []
    for k, v in items:
        if isinstance(v, (list, tuple)):
            pairs.extend((k, x) for x in v)
        elif v is not None:
            pairs.append((k, v))
    return urlencode(pairs).encode()


def encode_multipart(data, files):
    """Return (body, content_type) for multipart/form-data."""
    boundary = os.urandom(16).hex()
    out = bytearray()

    def part(disposition, content, ctype=None):
        out.extend(f"--{boundary}\r\nContent-Disposition: {disposition}\r\n".encode())
        if ctype:
            out.extend(f"Content-Type: {ctype}\r\n".encode())
        out.extend(b"\r\n")
        out.extend(content)
        out.extend(b"\r\n")

    fields = data.items() if hasattr(data, "items") else (data or [])
    for name, value in fields:
        values = value if isinstance(value, (list, tuple)) else [value]
        for v in values:
            if v is None:
                continue
            if not isinstance(v, bytes):
                v = str(v).encode()
            part(f'form-data; name="{_quote(name)}"', v)
    items = files.items() if hasattr(files, "items") else files
    for name, spec in items:
        filename, content, ctype = None, spec, None
        if isinstance(spec, (tuple, list)):
            filename = spec[0]
            content = spec[1]
            ctype = spec[2] if len(spec) > 2 else None
        if hasattr(content, "read"):
            if filename is None:
                filename = os.path.basename(getattr(content, "name", "") or "") or name
            content = content.read()
        if isinstance(content, str):
            content = content.encode()
        if filename is None:
            filename = name
        disposition = f'form-data; name="{_quote(name)}"'
        if filename:
            disposition += f'; filename="{_quote(filename)}"'
        part(disposition, content, ctype or "application/octet-stream")
    out.extend(f"--{boundary}--\r\n".encode())
    return bytes(out), f"multipart/form-data; boundary={boundary}"


def _quote(s):
    return str(s).replace("\\", "\\\\").replace('"', '\\"').replace("\r", " ").replace("\n", " ")


def basic_auth(user, password):
    if isinstance(user, str):
        user = user.encode("latin-1")
    if isinstance(password, str):
        password = password.encode("latin-1")
    return "Basic " + base64.b64encode(user + b":" + password).decode()


def json_body(obj, compact=True):
    """httpx sends compact UTF-8 JSON; requests uses json.dumps defaults."""
    if compact:
        return json.dumps(obj, allow_nan=False, separators=(",", ":"), ensure_ascii=False).encode()
    return json.dumps(obj, allow_nan=False).encode()


def decode_body(headers, body):
    """Undo Content-Encoding (gzip, deflate); unknown encodings pass through."""
    enc = ""
    for k, v in headers:
        if k.lower() == "content-encoding":
            enc = v.strip().lower()
    if not body or enc in ("", "identity"):
        return body
    if enc in ("gzip", "x-gzip"):
        return zlib.decompress(body, 16 + zlib.MAX_WBITS)
    if enc == "deflate":
        try:
            return zlib.decompress(body)
        except zlib.error:
            return zlib.decompress(body, -zlib.MAX_WBITS)
    return body


def charset_of(headers):
    for k, v in headers:
        if k.lower() == "content-type":
            for param in v.split(";")[1:]:
                name, _, value = param.partition("=")
                if name.strip().lower() == "charset":
                    return value.strip().strip("\"'") or None
    return None


def content_type_of(headers):
    for k, v in headers:
        if k.lower() == "content-type":
            return v.split(";", 1)[0].strip().lower()
    return ""


def parse_set_cookies(headers):
    """name -> value for every Set-Cookie header (attributes ignored)."""
    out = {}
    for k, v in headers:
        if k.lower() == "set-cookie":
            name, sep, value = v.split(";", 1)[0].partition("=")
            if sep and name.strip():
                out[name.strip()] = value.strip()
    return out


class CookieJar:
    """Per-host name=value cookies."""

    def __init__(self):
        self._hosts = {}

    def update_from(self, host, headers):
        cookies = parse_set_cookies(headers)
        if cookies:
            self._hosts.setdefault(host, {}).update(cookies)

    def set(self, name, value, host=""):
        self._hosts.setdefault(host, {})[name] = value

    def header_for(self, host, extra=None):
        merged = dict(self._hosts.get("", {}))
        merged.update(self._hosts.get(host, {}))
        if extra:
            merged.update(extra)
        if not merged:
            return None
        return "; ".join(f"{k}={v}" for k, v in merged.items())

    def as_dict(self):
        out = {}
        for cookies in self._hosts.values():
            out.update(cookies)
        return out


def split_url(url):
    """Validate an absolute http(s) URL. Returns (scheme, host) or raises
    ValueError with a short reason ('scheme', 'missing', 'host')."""
    parts = urlsplit(url)
    if not parts.scheme:
        raise ValueError("missing")
    if parts.scheme.lower() not in ("http", "https"):
        raise ValueError("scheme")
    if not parts.hostname:
        raise ValueError("host")
    return parts.scheme.lower(), parts.hostname.lower()


def send(method, url, headers, body, timeout):
    """One exchange through the host. Raises ConnectionError, TimeoutError or
    ValueError (from `_bashkit.http`)."""
    return _bashkit.http(method, url, headers, body or b"", timeout)


def redirect_target(method, status, url, headers):
    """Next (method, url, keep_body) for a redirect response, or None."""
    if status not in REDIRECT_CODES:
        return None
    location = None
    for k, v in headers:
        if k.lower() == "location":
            location = v
    if not location:
        return None
    new_url = urljoin(url, location.strip())
    if status == 303 and method != "HEAD":
        return "GET", new_url, False
    if status in (301, 302) and method == "POST":
        return "GET", new_url, False
    return method, new_url, status in (307, 308)


def same_host(a, b):
    return urlsplit(a).hostname == urlsplit(b).hostname
