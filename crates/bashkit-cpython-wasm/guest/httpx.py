"""httpx for bashkit's CPython: the common `httpx` API over the host's HTTP
egress pipeline (see `_bashkit_webcore`). `httpx2` is the same module.

Supported: the verb helpers and `stream()`, `Client` and `AsyncClient`
(base_url, headers, params, cookies, auth, timeout, follow_redirects,
event_hooks, transport=MockTransport), `Request`, `Response` (status_code,
reason_phrase, headers, content, text, json(), url, history, next_request,
is_success/..., raise_for_status, iter_*/aiter_*), `URL`, `Headers`,
`QueryParams`, `Cookies`, `Timeout`, `BasicAuth`, `codes`, and the upstream
exception hierarchy.
Accepted and ignored: verify, cert, http2, proxy, limits, mounts, trust_env.
Bodies are buffered by the host, so streaming iterates a buffered body;
async requests run one at a time.
"""

import builtins
import datetime
import json as _json
import time
from collections.abc import Mapping, MutableMapping
from contextlib import asynccontextmanager, contextmanager
from http import HTTPStatus
from urllib.parse import parse_qsl, urlencode, urljoin, urlsplit, urlunsplit

import _bashkit_webcore as core

__version__ = "0.28.1+bashkit"
__title__ = "httpx"

# --- exceptions -----------------------------------------------------------


class HTTPError(Exception):
    def __init__(self, message):
        super().__init__(message)
        self._request = None

    @property
    def request(self):
        if self._request is None:
            raise RuntimeError("The .request property has not been set.")
        return self._request

    @request.setter
    def request(self, request):
        self._request = request


class RequestError(HTTPError):
    def __init__(self, message, *, request=None):
        super().__init__(message)
        self._request = request


class TransportError(RequestError):
    pass


class TimeoutException(TransportError):
    pass


class ConnectTimeout(TimeoutException):
    pass


class ReadTimeout(TimeoutException):
    pass


class WriteTimeout(TimeoutException):
    pass


class PoolTimeout(TimeoutException):
    pass


class NetworkError(TransportError):
    pass


class ReadError(NetworkError):
    pass


class WriteError(NetworkError):
    pass


class ConnectError(NetworkError):
    pass


class CloseError(NetworkError):
    pass


class ProxyError(TransportError):
    pass


class UnsupportedProtocol(TransportError):
    pass


class ProtocolError(TransportError):
    pass


class LocalProtocolError(ProtocolError):
    pass


class RemoteProtocolError(ProtocolError):
    pass


class DecodingError(RequestError):
    pass


class TooManyRedirects(RequestError):
    pass


class HTTPStatusError(HTTPError):
    def __init__(self, message, *, request, response):
        super().__init__(message)
        self.request = request
        self.response = response


class InvalidURL(Exception):
    pass


class CookieConflict(Exception):
    pass


class StreamError(RuntimeError):
    pass


class StreamConsumed(StreamError):
    pass


class StreamClosed(StreamError):
    pass


class ResponseNotRead(StreamError):
    pass


class RequestNotRead(StreamError):
    pass


# --- status codes ---------------------------------------------------------


class _Codes:
    def __getattr__(self, name):
        try:
            return HTTPStatus[name.upper()].value
        except KeyError:
            raise AttributeError(name) from None

    def __getitem__(self, name):
        return getattr(self, name)

    @staticmethod
    def get_reason_phrase(value):
        return core.reason_phrase(value)

    @staticmethod
    def is_informational(value):
        return 100 <= value <= 199

    @staticmethod
    def is_success(value):
        return 200 <= value <= 299

    @staticmethod
    def is_redirect(value):
        return 300 <= value <= 399

    @staticmethod
    def is_client_error(value):
        return 400 <= value <= 499

    @staticmethod
    def is_server_error(value):
        return 500 <= value <= 599

    @staticmethod
    def is_error(value):
        return 400 <= value <= 599


codes = _Codes()

# --- config ---------------------------------------------------------------


class Timeout:
    def __init__(self, timeout=5.0, *, connect=..., read=..., write=..., pool=...):
        if isinstance(timeout, Timeout):
            connect, read, write, pool = timeout.connect, timeout.read, timeout.write, timeout.pool
        elif isinstance(timeout, tuple):
            connect, read, write, pool = (tuple(timeout) + (None,) * 4)[:4]
        else:
            connect = timeout if connect is ... else connect
            read = timeout if read is ... else read
            write = timeout if write is ... else write
            pool = timeout if pool is ... else pool
        self.connect, self.read, self.write, self.pool = connect, read, write, pool

    def seconds(self):
        parts = [t for t in (self.connect, self.read) if t is not None]
        return sum(parts) if parts else None

    def __eq__(self, other):
        return isinstance(other, Timeout) and (
            self.connect, self.read, self.write, self.pool
        ) == (other.connect, other.read, other.write, other.pool)

    def __repr__(self):
        if len({self.connect, self.read, self.write, self.pool}) == 1:
            return f"Timeout(timeout={self.connect})"
        return (f"Timeout(connect={self.connect}, read={self.read}, "
                f"write={self.write}, pool={self.pool})")


DEFAULT_TIMEOUT_CONFIG = Timeout(timeout=5.0)
DEFAULT_MAX_REDIRECTS = 20
USE_CLIENT_DEFAULT = object()


class Limits:
    def __init__(self, *, max_connections=None, max_keepalive_connections=None,
                 keepalive_expiry=5.0):
        self.max_connections = max_connections
        self.max_keepalive_connections = max_keepalive_connections
        self.keepalive_expiry = keepalive_expiry


DEFAULT_LIMITS = Limits(max_connections=100, max_keepalive_connections=20)

# --- URL, params, headers, cookies ---------------------------------------


class QueryParams(Mapping):
    def __init__(self, *args, **kwargs):
        value = args[0] if args else kwargs
        items = []
        if value is None:
            pass
        elif isinstance(value, QueryParams):
            items = value.multi_items()
        elif isinstance(value, (str, bytes)):
            if isinstance(value, bytes):
                value = value.decode("ascii")
            items = parse_qsl(value, keep_blank_values=True)
        else:
            pairs = value.items() if hasattr(value, "items") else value
            for k, v in pairs:
                values = v if isinstance(v, (list, tuple)) else [v]
                items.extend((str(k), _primitive(x)) for x in values)
        self._items = items

    def keys(self):
        return list(dict.fromkeys(k for k, _ in self._items))

    def __getitem__(self, key):
        for k, v in self._items:
            if k == key:
                return v
        raise KeyError(key)

    def __iter__(self):
        return iter(self.keys())

    def __len__(self):
        return len(self.keys())

    def multi_items(self):
        return list(self._items)

    def get_list(self, key):
        return [v for k, v in self._items if k == key]

    def set(self, key, value):
        q = QueryParams(self)
        q._items = [(k, v) for k, v in self._items if k != key] + [(key, _primitive(value))]
        return q

    def add(self, key, value):
        q = QueryParams(self)
        q._items = self._items + [(key, _primitive(value))]
        return q

    def remove(self, key):
        q = QueryParams(self)
        q._items = [(k, v) for k, v in self._items if k != key]
        return q

    def merge(self, params=None):
        q = QueryParams(self)
        other = QueryParams(params)
        keys = set(other.keys())
        q._items = [(k, v) for k, v in self._items if k not in keys] + other._items
        return q

    def __str__(self):
        return urlencode(self._items)

    def __repr__(self):
        return f"QueryParams({str(self)!r})"

    def __eq__(self, other):
        if not isinstance(other, QueryParams):
            try:
                other = QueryParams(other)
            except Exception:
                return NotImplemented
        return sorted(self._items) == sorted(other._items)

    __hash__ = None


def _primitive(v):
    if v is True:
        return "true"
    if v is False:
        return "false"
    if v is None:
        return ""
    return str(v)


class URL:
    def __init__(self, url="", **kwargs):
        if isinstance(url, URL):
            url = url._url
        elif isinstance(url, bytes):
            url = url.decode("ascii")
        elif not isinstance(url, str):
            raise TypeError(f"Invalid type for url.  Expected str or httpx.URL, got {type(url)}")
        params = kwargs.pop("params", None)
        parts = urlsplit(url)
        for key, value in kwargs.items():
            if key == "host":
                netloc = value if parts.port is None else f"{value}:{parts.port}"
                parts = parts._replace(netloc=netloc)
            elif key in ("scheme", "path", "fragment"):
                parts = parts._replace(**{key: value})
            elif key == "query":
                parts = parts._replace(query=value.decode() if isinstance(value, bytes) else value)
            else:
                raise TypeError(f"{key!r} is an invalid keyword argument for URL()")
        if params is not None:
            parts = parts._replace(query=str(QueryParams(params)))
        if parts.scheme in ("http", "https") and parts.netloc and not parts.path:
            parts = parts._replace(path="/")
        self._url = urlunsplit(parts)
        self._parts = urlsplit(self._url)

    @property
    def scheme(self):
        return self._parts.scheme

    @property
    def host(self):
        return self._parts.hostname or ""

    @property
    def port(self):
        return self._parts.port

    @property
    def netloc(self):
        return self._parts.netloc.encode("ascii")

    @property
    def path(self):
        return self._parts.path or "/"

    @property
    def query(self):
        return self._parts.query.encode("ascii")

    @property
    def params(self):
        return QueryParams(self._parts.query)

    @property
    def raw_path(self):
        q = self._parts.query
        return (self.path + (f"?{q}" if q else "")).encode("ascii")

    @property
    def fragment(self):
        return self._parts.fragment

    @property
    def userinfo(self):
        netloc = self._parts.netloc
        return netloc.rpartition("@")[0].encode() if "@" in netloc else b""

    @property
    def username(self):
        return self._parts.username or ""

    @property
    def password(self):
        return self._parts.password or ""

    @property
    def is_absolute_url(self):
        return bool(self._parts.scheme and self._parts.netloc)

    @property
    def is_relative_url(self):
        return not self.is_absolute_url

    def copy_with(self, **kwargs):
        return URL(self, **kwargs)

    def copy_set_param(self, key, value):
        return self.copy_with(params=self.params.set(key, value))

    def copy_add_param(self, key, value):
        return self.copy_with(params=self.params.add(key, value))

    def copy_remove_param(self, key):
        return self.copy_with(params=self.params.remove(key))

    def copy_merge_params(self, params):
        return self.copy_with(params=self.params.merge(params))

    def join(self, url):
        return URL(urljoin(self._url, str(url)))

    def __str__(self):
        return self._url

    def __repr__(self):
        return f"URL({self._url!r})"

    def __eq__(self, other):
        return isinstance(other, (URL, str)) and str(self) == str(URL(other))

    def __hash__(self):
        return hash(self._url)


class Headers(MutableMapping):
    def __init__(self, headers=None, encoding=None):
        self._list = []
        if isinstance(headers, Headers):
            self._list = list(headers._list)
        elif headers is not None:
            self._list = core.header_items(headers)

    def __getitem__(self, key):
        values = self.get_list(key)
        if not values:
            raise KeyError(key)
        return ", ".join(values)

    def __setitem__(self, key, value):
        low = key.lower()
        self._list = [(k, v) for k, v in self._list if k.lower() != low]
        self._list.append((key, str(value)))

    def __delitem__(self, key):
        low = key.lower()
        if not any(k.lower() == low for k, _ in self._list):
            raise KeyError(key)
        self._list = [(k, v) for k, v in self._list if k.lower() != low]

    def __iter__(self):
        return iter(dict.fromkeys(k.lower() for k, _ in self._list))

    def __len__(self):
        return len(set(k.lower() for k, _ in self._list))

    def __contains__(self, key):
        low = key.lower()
        return any(k.lower() == low for k, _ in self._list)

    def get_list(self, key, split_commas=False):
        low = key.lower()
        values = [v for k, v in self._list if k.lower() == low]
        if split_commas:
            values = [x.strip() for v in values for x in v.split(",")]
        return values

    def multi_items(self):
        return [(k.lower(), v) for k, v in self._list]

    @property
    def raw(self):
        return [(k.encode("latin-1"), v.encode("latin-1")) for k, v in self._list]

    @property
    def encoding(self):
        return "ascii"

    def update(self, headers=None, **kwargs):
        for k, v in core.header_items(Headers(headers)._list if headers is not None else []):
            self[k] = v
        for k, v in kwargs.items():
            self[k] = v

    def copy(self):
        return Headers(self)

    def __eq__(self, other):
        try:
            other = Headers(other)
        except Exception:
            return False
        return sorted(self.multi_items()) == sorted(other.multi_items())

    __hash__ = None

    def __repr__(self):
        return f"Headers({dict(self.items())!r})"


class Cookies(MutableMapping):
    def __init__(self, cookies=None):
        self.jar = core.CookieJar()
        if isinstance(cookies, Cookies):
            for host, values in cookies.jar._hosts.items():
                self.jar._hosts[host] = dict(values)
        elif cookies:
            items = cookies.items() if hasattr(cookies, "items") else cookies
            for k, v in items:
                self.jar.set(k, v)

    def set(self, name, value, domain="", path="/"):
        self.jar.set(name, value, domain.lstrip(".").lower())

    def get(self, name, default=None, domain=None, path=None):
        if domain:
            return self.jar._hosts.get(domain.lstrip(".").lower(), {}).get(name, default)
        return self.jar.as_dict().get(name, default)

    def delete(self, name, domain=None, path=None):
        for host, values in self.jar._hosts.items():
            if domain is None or host == domain:
                values.pop(name, None)

    def clear(self, domain=None, path=None):
        if domain is None:
            self.jar._hosts.clear()
        else:
            self.jar._hosts.pop(domain, None)

    def update(self, cookies=None, **kwargs):
        for k, v in Cookies(cookies).items():
            self[k] = v
        for k, v in kwargs.items():
            self[k] = v

    def __getitem__(self, name):
        return self.jar.as_dict()[name]

    def __setitem__(self, name, value):
        self.set(name, value)

    def __delitem__(self, name):
        if name not in self.jar.as_dict():
            raise KeyError(name)
        self.delete(name)

    def __iter__(self):
        return iter(self.jar.as_dict())

    def __len__(self):
        return len(self.jar.as_dict())

    def __bool__(self):
        return bool(self.jar.as_dict())

    def __repr__(self):
        return f"<Cookies{self.jar.as_dict()}>"


# --- auth -----------------------------------------------------------------


class Auth:
    requires_request_body = False
    requires_response_body = False

    def auth_flow(self, request):
        yield request


class BasicAuth(Auth):
    def __init__(self, username, password):
        self._header = core.basic_auth(username, password)

    def auth_flow(self, request):
        request.headers["Authorization"] = self._header
        yield request


class FunctionAuth(Auth):
    def __init__(self, func):
        self._func = func

    def auth_flow(self, request):
        yield self._func(request)


def _build_auth(auth):
    if auth is None:
        return None
    if isinstance(auth, tuple):
        return BasicAuth(*auth)
    if isinstance(auth, Auth):
        return auth
    if callable(auth):
        return FunctionAuth(auth)
    raise TypeError(f'Invalid "auth" argument: {auth!r}')


def _apply_auth(auth, request):
    if auth is None:
        return request
    flow = auth.auth_flow(request)
    try:
        return next(flow)
    except StopIteration:
        return request


# --- models ---------------------------------------------------------------


class Request:
    def __init__(self, method, url, *, params=None, headers=None, cookies=None,
                 content=None, data=None, files=None, json=None, stream=None,
                 extensions=None):
        self.method = method.upper() if isinstance(method, str) else method.decode().upper()
        self.url = URL(url)
        if params is not None:
            self.url = self.url.copy_merge_params(params)
        self.headers = Headers(headers)
        if "host" not in self.headers and self.url._parts.netloc:
            # httpx sets Host first; the bashkit host replaces it anyway.
            self.headers._list.insert(0, ("Host", self.url._parts.netloc.rpartition("@")[2]))
        self.extensions = extensions or {}
        body, ctype = b"", None
        if content is not None:
            if isinstance(content, str):
                body = content.encode()
            elif isinstance(content, (bytes, bytearray)):
                body = bytes(content)
            else:
                body = b"".join(c.encode() if isinstance(c, str) else c for c in content)
        elif files:
            body, ctype = core.encode_multipart(data or {}, files)
        elif data is not None:
            if isinstance(data, (str, bytes)):
                body = data.encode() if isinstance(data, str) else data
            else:
                body, ctype = core.encode_form(data), "application/x-www-form-urlencoded"
        elif json is not None:
            body, ctype = core.json_body(json), "application/json"
        if ctype and "content-type" not in self.headers:
            self.headers["Content-Type"] = ctype
        if body and "content-length" not in self.headers:
            self.headers["Content-Length"] = str(len(body))
        if cookies:
            c = Cookies(cookies)
            header = c.jar.header_for(self.url.host)
            if header:
                self.headers["Cookie"] = header
        self._content = body

    @property
    def content(self):
        return self._content

    def read(self):
        return self._content

    async def aread(self):
        return self._content

    def __repr__(self):
        return f"<Request({self.method!r}, {str(self.url)!r})>"


class Response:
    def __init__(self, status_code, *, headers=None, content=None, text=None,
                 html=None, json=None, stream=None, request=None, extensions=None,
                 history=None, default_encoding="utf-8"):
        self.status_code = status_code
        self.headers = Headers(headers)
        self._request = request
        self.history = list(history or [])
        self.next_request = None
        self.extensions = extensions or {}
        self.is_closed = True
        self.is_stream_consumed = True
        self.default_encoding = default_encoding
        self._encoding = None
        self._elapsed = datetime.timedelta(0)
        self.cookies = Cookies()
        if content is not None:
            body = content.encode() if isinstance(content, str) else bytes(content)
        elif text is not None:
            body = text.encode()
            if "content-type" not in self.headers:
                self.headers["Content-Type"] = "text/plain; charset=utf-8"
        elif html is not None:
            body = html.encode()
            if "content-type" not in self.headers:
                self.headers["Content-Type"] = "text/html; charset=utf-8"
        elif json is not None:
            body = core.json_body(json)
            if "content-type" not in self.headers:
                self.headers["Content-Type"] = "application/json"
        else:
            body = b""
        self._content = body

    @property
    def request(self):
        if self._request is None:
            raise RuntimeError("The request instance has not been set on this response.")
        return self._request

    @request.setter
    def request(self, value):
        self._request = value

    @property
    def url(self):
        return self.request.url

    @property
    def elapsed(self):
        return self._elapsed

    @property
    def http_version(self):
        return "HTTP/1.1"

    @property
    def reason_phrase(self):
        return core.reason_phrase(self.status_code)

    @property
    def content(self):
        return self._content

    @property
    def charset_encoding(self):
        return core.charset_of(self.headers.multi_items())

    @property
    def encoding(self):
        if self._encoding is None:
            enc = self.charset_encoding
            if enc:
                try:
                    "".encode(enc)
                except LookupError:
                    enc = None
            self._encoding = enc or (self.default_encoding if isinstance(self.default_encoding, str)
                                     else self.default_encoding(self._content))
        return self._encoding

    @encoding.setter
    def encoding(self, value):
        self._encoding = value

    @property
    def text(self):
        return self._content.decode(self.encoding, errors="replace")

    def json(self, **kwargs):
        return _json.loads(self._content, **kwargs)

    @property
    def num_bytes_downloaded(self):
        return len(self._content)

    @property
    def is_informational(self):
        return codes.is_informational(self.status_code)

    @property
    def is_success(self):
        return codes.is_success(self.status_code)

    @property
    def is_redirect(self):
        return codes.is_redirect(self.status_code)

    @property
    def is_client_error(self):
        return codes.is_client_error(self.status_code)

    @property
    def is_server_error(self):
        return codes.is_server_error(self.status_code)

    @property
    def is_error(self):
        return codes.is_error(self.status_code)

    @property
    def has_redirect_location(self):
        return self.status_code in core.REDIRECT_CODES and "location" in self.headers

    @property
    def links(self):
        header = self.headers.get("link")
        links = {}
        if not header:
            return links
        for val in header.split(","):
            url, _, params = val.partition(";")
            link = {"url": url.strip(" <>'\"")}
            for param in params.split(";"):
                key, _, value = param.partition("=")
                if key.strip():
                    link[key.strip(" '\"")] = value.strip(" '\"")
            links[link.get("rel") or link["url"]] = link
        return links

    def raise_for_status(self):
        if self._request is None:
            raise RuntimeError(
                "Cannot call `raise_for_status` as the request instance has not been set on this response."
            )
        if self.is_success:
            return self
        if self.has_redirect_location:
            message = ("{error_type} '{0.status_code} {0.reason_phrase}' for url '{0.url}'\n"
                       "Redirect location: '{0.headers[location]}'\n"
                       "For more information check: https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/{0.status_code}")
        else:
            message = ("{error_type} '{0.status_code} {0.reason_phrase}' for url '{0.url}'\n"
                       "For more information check: https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/{0.status_code}")
        error_type = {1: "Informational response", 3: "Redirect response", 4: "Client error",
                      5: "Server error"}.get(self.status_code // 100, "Invalid status code")
        raise HTTPStatusError(message.format(self, error_type=error_type),
                              request=self._request, response=self)

    def read(self):
        return self._content

    async def aread(self):
        return self._content

    def iter_bytes(self, chunk_size=None):
        data = self._content
        size = chunk_size or len(data) or 1
        for i in range(0, len(data), size):
            yield data[i : i + size]

    iter_raw = iter_bytes

    def iter_text(self, chunk_size=None):
        text = self.text
        size = chunk_size or len(text) or 1
        for i in range(0, len(text), size):
            yield text[i : i + size]

    def iter_lines(self):
        yield from self.text.splitlines()

    async def aiter_bytes(self, chunk_size=None):
        for chunk in self.iter_bytes(chunk_size):
            yield chunk

    aiter_raw = aiter_bytes

    async def aiter_text(self, chunk_size=None):
        for chunk in self.iter_text(chunk_size):
            yield chunk

    async def aiter_lines(self):
        for line in self.iter_lines():
            yield line

    def close(self):
        pass

    async def aclose(self):
        pass

    def __enter__(self):
        return self

    def __exit__(self, *args):
        pass

    async def __aenter__(self):
        return self

    async def __aexit__(self, *args):
        pass

    def __repr__(self):
        return f"<Response [{self.status_code} {self.reason_phrase}]>"


# --- transports -----------------------------------------------------------


class BaseTransport:
    def handle_request(self, request):
        raise NotImplementedError()

    def close(self):
        pass

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()


class AsyncBaseTransport:
    async def handle_async_request(self, request):
        raise NotImplementedError()

    async def aclose(self):
        pass


class HTTPTransport(BaseTransport):
    """Sends through the bashkit host. Connection options are accepted and
    ignored: the host owns connections, TLS and proxies."""

    def __init__(self, *args, **kwargs):
        pass

    def handle_request(self, request):
        url = str(request.url)
        try:
            core.split_url(url)
        except ValueError as e:
            if str(e) == "missing":
                raise UnsupportedProtocol(
                    "Request URL is missing an 'http://' or 'https://' protocol.", request=request
                ) from None
            if str(e) == "scheme":
                raise UnsupportedProtocol(
                    f"Request URL has an unsupported protocol '{request.url.scheme}://'.",
                    request=request,
                ) from None
            raise LocalProtocolError("Request URL has no host.", request=request) from None
        timeout = request.extensions.get("timeout")
        headers = [(k, v) for k, v in request.headers._list if k.lower() != "content-length"]
        try:
            status, rheaders, body = core.send(request.method, url, headers, request.content, timeout)
        except TimeoutError as e:
            raise ReadTimeout(str(e), request=request) from None
        except builtins.ConnectionError as e:
            raise ConnectError(str(e), request=request) from None
        except ValueError as e:
            raise LocalProtocolError(str(e), request=request) from None
        if request.method != "HEAD":
            try:
                body = core.decode_body(rheaders, body)
            except Exception as e:
                raise DecodingError(str(e), request=request) from None
        rheaders = [(k, v) for k, v in rheaders if k.lower() != "content-encoding"]
        return Response(status, headers=rheaders, content=body, request=request)


class AsyncHTTPTransport(AsyncBaseTransport):
    def __init__(self, *args, **kwargs):
        self._sync = HTTPTransport()

    async def handle_async_request(self, request):
        return self._sync.handle_request(request)


class MockTransport(BaseTransport, AsyncBaseTransport):
    def __init__(self, handler):
        self.handler = handler

    def handle_request(self, request):
        return self.handler(request)

    async def handle_async_request(self, request):
        response = self.handler(request)
        if not isinstance(response, Response):
            response = await response
        return response


# --- clients --------------------------------------------------------------


class _BaseClient:
    def __init__(self, *, auth=None, params=None, headers=None, cookies=None, verify=True,
                 cert=None, http1=True, http2=False, proxy=None, mounts=None,
                 timeout=DEFAULT_TIMEOUT_CONFIG, follow_redirects=False, limits=DEFAULT_LIMITS,
                 max_redirects=DEFAULT_MAX_REDIRECTS, event_hooks=None, base_url="",
                 transport=None, trust_env=True, default_encoding="utf-8", **kwargs):
        self._base_url = self._enforce_trailing_slash(URL(base_url))
        self.auth = auth
        self._params = QueryParams(params)
        self.headers = Headers({
            "Accept": "*/*",
            "Accept-Encoding": "gzip, deflate",
            "Connection": "keep-alive",
            "User-Agent": f"python-httpx/{__version__}",
        })
        self.headers.update(headers)
        self._cookies = Cookies(cookies)
        self._timeout = Timeout(timeout)
        self.follow_redirects = follow_redirects
        self.max_redirects = max_redirects
        self._event_hooks = {"request": list((event_hooks or {}).get("request", [])),
                             "response": list((event_hooks or {}).get("response", []))}
        self._transport = transport
        self._default_encoding = default_encoding
        self.is_closed = False

    @staticmethod
    def _enforce_trailing_slash(url):
        if url._url and not url._parts.path.endswith("/"):
            return url.copy_with(path=url._parts.path + "/")
        return url

    @property
    def base_url(self):
        return self._base_url

    @base_url.setter
    def base_url(self, value):
        self._base_url = self._enforce_trailing_slash(URL(value))

    @property
    def params(self):
        return self._params

    @params.setter
    def params(self, value):
        self._params = QueryParams(value)

    @property
    def cookies(self):
        return self._cookies

    @cookies.setter
    def cookies(self, value):
        self._cookies = Cookies(value)

    @property
    def timeout(self):
        return self._timeout

    @timeout.setter
    def timeout(self, value):
        self._timeout = Timeout(value)

    @property
    def event_hooks(self):
        return self._event_hooks

    @event_hooks.setter
    def event_hooks(self, value):
        self._event_hooks = {"request": list(value.get("request", [])),
                             "response": list(value.get("response", []))}

    def _merge_url(self, url):
        url = URL(url)
        if url.is_relative_url and self._base_url._url:
            rel = str(url).lstrip("/")
            return URL(self._base_url._url + rel)
        return url

    def build_request(self, method, url, *, content=None, data=None, files=None, json=None,
                      params=None, headers=None, cookies=None, timeout=USE_CLIENT_DEFAULT,
                      extensions=None):
        url = self._merge_url(url)
        merged_params = self._params.merge(params) if params is not None else self._params
        if merged_params:
            url = url.copy_merge_params(merged_params)
        merged = Headers(self.headers)
        if headers is not None:
            for k, v in Headers(headers)._list:
                merged[k] = v
        t = self._timeout if timeout is USE_CLIENT_DEFAULT else Timeout(timeout)
        ext = dict(extensions or {})
        ext["timeout"] = t.seconds()
        request = Request(method, url, headers=merged, content=content, data=data, files=files,
                          json=json, extensions=ext)
        jar = Cookies(self._cookies)
        if cookies:
            for k, v in Cookies(cookies).items():
                jar.set(k, v)
        header = jar.jar.header_for(request.url.host)
        if header and "cookie" not in request.headers:
            request.headers["Cookie"] = header
        return request

    def _redirect_request(self, request, response):
        target = core.redirect_target(request.method, response.status_code, str(request.url),
                                      response.headers.multi_items())
        if target is None:
            return None
        method, new_url, keep_body = target
        headers = Headers(request.headers)
        headers.pop("host", None)
        if not core.same_host(str(request.url), new_url):
            headers.pop("authorization", None)
        headers.pop("cookie", None)
        new = Request(method, new_url, headers=headers,
                      content=request.content if keep_body else None,
                      extensions=dict(request.extensions))
        if not keep_body:
            for h in ("content-type", "content-length", "transfer-encoding"):
                new.headers.pop(h, None)
        header = self._cookies.jar.header_for(new.url.host)
        if header:
            new.headers["Cookie"] = header
        return new

    def _after_response(self, request, response, started):
        response.request = request
        response._elapsed = datetime.timedelta(seconds=time.monotonic() - started)
        response.default_encoding = self._default_encoding
        multi = response.headers.multi_items()
        self._cookies.jar.update_from(request.url.host, multi)
        response.cookies = Cookies(core.parse_set_cookies(multi))


class Client(_BaseClient):
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        if self._transport is None:
            self._transport = HTTPTransport()

    def request(self, method, url, *, content=None, data=None, files=None, json=None,
                params=None, headers=None, cookies=None, auth=USE_CLIENT_DEFAULT,
                follow_redirects=USE_CLIENT_DEFAULT, timeout=USE_CLIENT_DEFAULT,
                extensions=None):
        request = self.build_request(method, url, content=content, data=data, files=files,
                                     json=json, params=params, headers=headers, cookies=cookies,
                                     timeout=timeout, extensions=extensions)
        return self.send(request, auth=auth, follow_redirects=follow_redirects)

    @contextmanager
    def stream(self, method, url, **kwargs):
        response = self.request(method, url, **kwargs)
        try:
            yield response
        finally:
            response.close()

    def send(self, request, *, stream=False, auth=USE_CLIENT_DEFAULT,
             follow_redirects=USE_CLIENT_DEFAULT):
        if self.is_closed:
            raise RuntimeError("Cannot send a request, as the client has been closed.")
        follow = self.follow_redirects if follow_redirects is USE_CLIENT_DEFAULT else follow_redirects
        auth = _build_auth(self.auth if auth is USE_CLIENT_DEFAULT else auth)
        request = _apply_auth(auth, request)
        history = []
        while True:
            for hook in self._event_hooks["request"]:
                hook(request)
            started = time.monotonic()
            response = self._transport.handle_request(request)
            self._after_response(request, response, started)
            response.history = list(history)
            for hook in self._event_hooks["response"]:
                hook(response)
            nxt = self._redirect_request(request, response)
            if nxt is None:
                return response
            if not follow:
                response.next_request = nxt
                return response
            if len(history) >= self.max_redirects:
                raise TooManyRedirects("Exceeded maximum allowed redirects.", request=request)
            history.append(response)
            request = nxt

    def get(self, url, **kwargs):
        return self.request("GET", url, **kwargs)

    def options(self, url, **kwargs):
        return self.request("OPTIONS", url, **kwargs)

    def head(self, url, **kwargs):
        return self.request("HEAD", url, **kwargs)

    def post(self, url, **kwargs):
        return self.request("POST", url, **kwargs)

    def put(self, url, **kwargs):
        return self.request("PUT", url, **kwargs)

    def patch(self, url, **kwargs):
        return self.request("PATCH", url, **kwargs)

    def delete(self, url, **kwargs):
        return self.request("DELETE", url, **kwargs)

    def close(self):
        self.is_closed = True

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()


class AsyncClient(_BaseClient):
    """Same as `Client` with async methods. Requests run one at a time:
    the host call blocks the guest while it waits."""

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        if self._transport is None:
            self._transport = AsyncHTTPTransport()

    async def request(self, method, url, *, content=None, data=None, files=None, json=None,
                      params=None, headers=None, cookies=None, auth=USE_CLIENT_DEFAULT,
                      follow_redirects=USE_CLIENT_DEFAULT, timeout=USE_CLIENT_DEFAULT,
                      extensions=None):
        request = self.build_request(method, url, content=content, data=data, files=files,
                                     json=json, params=params, headers=headers, cookies=cookies,
                                     timeout=timeout, extensions=extensions)
        return await self.send(request, auth=auth, follow_redirects=follow_redirects)

    @asynccontextmanager
    async def stream(self, method, url, **kwargs):
        response = await self.request(method, url, **kwargs)
        try:
            yield response
        finally:
            await response.aclose()

    async def send(self, request, *, stream=False, auth=USE_CLIENT_DEFAULT,
                   follow_redirects=USE_CLIENT_DEFAULT):
        if self.is_closed:
            raise RuntimeError("Cannot send a request, as the client has been closed.")
        follow = self.follow_redirects if follow_redirects is USE_CLIENT_DEFAULT else follow_redirects
        auth = _build_auth(self.auth if auth is USE_CLIENT_DEFAULT else auth)
        request = _apply_auth(auth, request)
        history = []
        while True:
            for hook in self._event_hooks["request"]:
                await hook(request)
            started = time.monotonic()
            response = await self._transport.handle_async_request(request)
            self._after_response(request, response, started)
            response.history = list(history)
            for hook in self._event_hooks["response"]:
                await hook(response)
            nxt = self._redirect_request(request, response)
            if nxt is None:
                return response
            if not follow:
                response.next_request = nxt
                return response
            if len(history) >= self.max_redirects:
                raise TooManyRedirects("Exceeded maximum allowed redirects.", request=request)
            history.append(response)
            request = nxt

    async def get(self, url, **kwargs):
        return await self.request("GET", url, **kwargs)

    async def options(self, url, **kwargs):
        return await self.request("OPTIONS", url, **kwargs)

    async def head(self, url, **kwargs):
        return await self.request("HEAD", url, **kwargs)

    async def post(self, url, **kwargs):
        return await self.request("POST", url, **kwargs)

    async def put(self, url, **kwargs):
        return await self.request("PUT", url, **kwargs)

    async def patch(self, url, **kwargs):
        return await self.request("PATCH", url, **kwargs)

    async def delete(self, url, **kwargs):
        return await self.request("DELETE", url, **kwargs)

    async def aclose(self):
        self.is_closed = True

    async def __aenter__(self):
        return self

    async def __aexit__(self, *args):
        await self.aclose()


# --- top-level API --------------------------------------------------------


def request(method, url, *, params=None, content=None, data=None, files=None, json=None,
            headers=None, cookies=None, auth=None, proxy=None, timeout=DEFAULT_TIMEOUT_CONFIG,
            follow_redirects=False, verify=True, trust_env=True, **kwargs):
    with Client(cookies=cookies, timeout=timeout) as client:
        return client.request(method, url, content=content, data=data, files=files, json=json,
                              params=params, headers=headers, auth=auth,
                              follow_redirects=follow_redirects)


@contextmanager
def stream(method, url, **kwargs):
    yield request(method, url, **kwargs)


def get(url, **kwargs):
    return request("GET", url, **kwargs)


def options(url, **kwargs):
    return request("OPTIONS", url, **kwargs)


def head(url, **kwargs):
    return request("HEAD", url, **kwargs)


def post(url, **kwargs):
    return request("POST", url, **kwargs)


def put(url, **kwargs):
    return request("PUT", url, **kwargs)


def patch(url, **kwargs):
    return request("PATCH", url, **kwargs)


def delete(url, **kwargs):
    return request("DELETE", url, **kwargs)


__all__ = [
    "AsyncBaseTransport", "AsyncClient", "AsyncHTTPTransport", "Auth", "BaseTransport",
    "BasicAuth", "Client", "CloseError", "ConnectError", "ConnectTimeout", "CookieConflict",
    "Cookies", "DecodingError", "FunctionAuth", "HTTPError", "HTTPStatusError",
    "HTTPTransport", "Headers", "InvalidURL", "Limits", "LocalProtocolError", "MockTransport",
    "NetworkError", "PoolTimeout", "ProtocolError", "ProxyError", "QueryParams", "ReadError",
    "ReadTimeout", "RemoteProtocolError", "Request", "RequestError", "RequestNotRead",
    "Response", "ResponseNotRead", "StreamClosed", "StreamConsumed", "StreamError",
    "Timeout", "TimeoutException", "TooManyRedirects", "TransportError", "URL",
    "UnsupportedProtocol", "WriteError", "WriteTimeout", "codes", "delete", "get", "head",
    "options", "patch", "post", "put", "request", "stream",
]
