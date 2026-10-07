"""requests.models (bashkit): Request, PreparedRequest, Response."""

import datetime
import json as _json

import _bashkit_webcore as core

from .exceptions import HTTPError, JSONDecodeError, StreamConsumedError
from .structures import CaseInsensitiveDict


class Request:
    def __init__(self, method=None, url=None, headers=None, files=None, data=None,
                 params=None, auth=None, cookies=None, hooks=None, json=None):
        self.method = method
        self.url = url
        self.headers = headers or {}
        self.files = files or []
        self.data = data or []
        self.json = json
        self.params = params or {}
        self.auth = auth
        self.cookies = cookies
        self.hooks = hooks or {}

    def __repr__(self):
        return f"<Request [{self.method}]>"

    def prepare(self):
        p = PreparedRequest()
        p.prepare(method=self.method, url=self.url, headers=self.headers,
                  files=self.files, data=self.data, params=self.params,
                  auth=self.auth, json=self.json)
        return p


class PreparedRequest:
    def __init__(self):
        self.method = None
        self.url = None
        self.headers = CaseInsensitiveDict()
        self.body = None
        self.hooks = {"response": []}

    def prepare(self, method=None, url=None, headers=None, files=None, data=None,
                params=None, auth=None, cookies=None, hooks=None, json=None):
        self.method = (method or "GET").upper()
        self.url = core.add_params(str(url), params)
        self.headers = CaseInsensitiveDict(core.header_items(headers))
        body, ctype = None, None
        if files:
            body, ctype = core.encode_multipart(data or {}, files)
        elif data:
            if hasattr(data, "read"):
                body = data.read()
                body = body.encode() if isinstance(body, str) else body
            elif isinstance(data, (str, bytes)):
                body = data.encode() if isinstance(data, str) else data
            else:
                body = core.encode_form(data)
                ctype = "application/x-www-form-urlencoded"
        elif json is not None:
            body = core.json_body(json, compact=False)
            ctype = "application/json"
        if ctype and "content-type" not in {k.lower() for k in self.headers}:
            self.headers["Content-Type"] = ctype
        self.body = body
        if auth is not None:
            if isinstance(auth, tuple) and len(auth) == 2:
                self.headers["Authorization"] = core.basic_auth(*auth)
            else:
                r = auth(self)
                if r is not None and r is not self:
                    self.__dict__.update(r.__dict__)
        return self

    def copy(self):
        p = PreparedRequest()
        p.method, p.url, p.body = self.method, self.url, self.body
        p.headers = self.headers.copy()
        p.hooks = dict(self.hooks)
        return p

    @property
    def path_url(self):
        parts = core.urlsplit(self.url)
        return (parts.path or "/") + (f"?{parts.query}" if parts.query else "")

    def __repr__(self):
        return f"<PreparedRequest [{self.method}]>"


class Response:
    __attrs__ = ["_content", "status_code", "headers", "url", "history",
                 "encoding", "reason", "cookies", "elapsed", "request"]

    def __init__(self):
        self._content = b""
        self._consumed = False
        self.status_code = None
        self.headers = CaseInsensitiveDict()
        self.raw = None
        self.url = None
        self.encoding = None
        self.history = []
        self.reason = None
        self.cookies = {}
        self.elapsed = datetime.timedelta(0)
        self.request = None
        self.connection = None

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    def __repr__(self):
        return f"<Response [{self.status_code}]>"

    def __bool__(self):
        return self.ok

    def __iter__(self):
        return self.iter_content(128)

    @property
    def ok(self):
        try:
            self.raise_for_status()
        except HTTPError:
            return False
        return True

    @property
    def is_redirect(self):
        return "location" in self.headers and self.status_code in core.REDIRECT_CODES

    @property
    def is_permanent_redirect(self):
        return "location" in self.headers and self.status_code in (301, 308)

    @property
    def content(self):
        return self._content

    @property
    def apparent_encoding(self):
        try:
            self._content.decode("utf-8")
            return "utf-8"
        except UnicodeDecodeError:
            return "ISO-8859-1"

    @property
    def text(self):
        if not self._content:
            return ""
        encoding = self.encoding or self.apparent_encoding
        try:
            return str(self._content, encoding, errors="replace")
        except (LookupError, TypeError):
            return str(self._content, errors="replace")

    def json(self, **kwargs):
        try:
            if self.encoding is None and self._content[:3] == b"\xef\xbb\xbf":
                return _json.loads(self._content.decode("utf-8-sig"), **kwargs)
            return _json.loads(self.text if self.encoding else self._content, **kwargs)
        except _json.JSONDecodeError as e:
            raise JSONDecodeError(e.msg, e.doc, e.pos) from None

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

    def iter_content(self, chunk_size=1, decode_unicode=False):
        if self._consumed and not self._content:
            raise StreamConsumedError()
        data = self._content
        size = chunk_size or len(data) or 1
        if decode_unicode:
            text = self.text
            for i in range(0, len(text), size):
                yield text[i : i + size]
            return
        for i in range(0, len(data), size):
            yield data[i : i + size]

    def iter_lines(self, chunk_size=512, decode_unicode=False, delimiter=None):
        body = self.text if decode_unicode else self._content
        lines = body.split(delimiter) if delimiter else body.splitlines()
        yield from lines

    def raise_for_status(self):
        reason = self.reason or ""
        if 400 <= self.status_code < 500:
            msg = f"{self.status_code} Client Error: {reason} for url: {self.url}"
        elif 500 <= self.status_code < 600:
            msg = f"{self.status_code} Server Error: {reason} for url: {self.url}"
        else:
            return
        raise HTTPError(msg, response=self)

    def close(self):
        self._consumed = True
