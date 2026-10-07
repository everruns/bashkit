"""requests.sessions (bashkit): Session and the request loop.

Every exchange goes through `_bashkit.http`, so the host's egress policy
applies to each request and each redirect hop.
"""

import builtins
import datetime
import time
from collections.abc import MutableMapping

import _bashkit_webcore as core

from . import __version__
from .adapters import HTTPAdapter
from .exceptions import (
    ConnectionError,
    InvalidHeader,
    InvalidSchema,
    InvalidURL,
    MissingSchema,
    ReadTimeout,
    TooManyRedirects,
)
from .models import PreparedRequest, Request, Response
from .structures import CaseInsensitiveDict


def default_user_agent(name="python-requests"):
    return f"{name}/{__version__}"


def default_headers():
    return CaseInsensitiveDict({
        "User-Agent": default_user_agent(),
        "Accept-Encoding": "gzip, deflate",
        "Accept": "*/*",
    })


class RequestsCookieJar(MutableMapping):
    """Dict-like cookie jar. Cookies from responses are kept per host and
    sent back only to that host; cookies set by hand go to every host."""

    def __init__(self):
        self._jar = core.CookieJar()

    def __getitem__(self, name):
        return self._jar.as_dict()[name]

    def __setitem__(self, name, value):
        self._jar.set(name, value)

    def __delitem__(self, name):
        found = False
        for cookies in self._jar._hosts.values():
            if name in cookies:
                del cookies[name]
                found = True
        if not found:
            raise KeyError(name)

    def __iter__(self):
        return iter(self._jar.as_dict())

    def __len__(self):
        return len(self._jar.as_dict())

    def set(self, name, value, domain=None, **kwargs):
        if value is None:
            self.pop(name, None)
        else:
            self._jar.set(name, value, (domain or "").lstrip(".").lower())

    def get_dict(self, domain=None, path=None):
        if domain:
            return dict(self._jar._hosts.get(domain.lstrip(".").lower(), {}))
        return self._jar.as_dict()

    def __repr__(self):
        return f"<RequestsCookieJar{self._jar.as_dict()}>"


def _timeout_seconds(timeout):
    if isinstance(timeout, tuple):
        parts = [t for t in timeout if t is not None]
        return sum(parts) if parts else None
    return timeout


def _check_url(url):
    try:
        core.split_url(url)
    except ValueError as e:
        if str(e) == "missing":
            raise MissingSchema(
                f"Invalid URL {url!r}: No scheme supplied. Perhaps you meant https://{url}?"
            ) from None
        if str(e) == "scheme":
            raise InvalidSchema(f"No connection adapters were found for {url!r}") from None
        raise InvalidURL(f"Invalid URL {url!r}: No host supplied") from None


class Session:
    __attrs__ = ["headers", "cookies", "auth", "proxies", "hooks", "params",
                 "verify", "cert", "adapters", "stream", "trust_env", "max_redirects"]

    def __init__(self):
        self.headers = default_headers()
        self.auth = None
        self.proxies = {}
        self.hooks = {"response": []}
        self.params = {}
        self.stream = False
        self.verify = True
        self.cert = None
        self.max_redirects = core.MAX_REDIRECTS
        self.trust_env = True
        self.cookies = RequestsCookieJar()
        self.adapters = {"https://": HTTPAdapter(), "http://": HTTPAdapter()}

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    def close(self):
        pass

    def mount(self, prefix, adapter):
        self.adapters[prefix] = adapter

    def get_adapter(self, url):
        for prefix, adapter in self.adapters.items():
            if url.lower().startswith(prefix.lower()):
                return adapter
        raise InvalidSchema(f"No connection adapters were found for {url!r}")

    def prepare_request(self, request):
        headers = CaseInsensitiveDict(self.headers)
        for k, v in core.header_items(request.headers):
            headers[k] = v
        for k, v in (request.headers or {}).items() if hasattr(request.headers, "items") else []:
            if v is None:
                headers.pop(k, None)
        params = dict(self.params)
        if request.params and hasattr(request.params, "items"):
            params.update(request.params)
            params = {k: v for k, v in params.items() if v is not None}
        elif request.params:
            params = request.params
        auth = request.auth if request.auth is not None else self.auth
        p = PreparedRequest()
        p.prepare(method=request.method, url=request.url, headers=headers,
                  files=request.files, data=request.data, params=params,
                  auth=auth, json=request.json)
        p._cookies = dict(request.cookies or {})
        hooks = request.hooks.get("response", []) if request.hooks else []
        p.hooks = {"response": list(self.hooks.get("response", [])) + (
            list(hooks) if isinstance(hooks, (list, tuple)) else [hooks])}
        return p

    def request(self, method, url, params=None, data=None, headers=None, cookies=None,
                files=None, auth=None, timeout=None, allow_redirects=True, proxies=None,
                hooks=None, stream=None, verify=None, cert=None, json=None):
        req = Request(method=method.upper(), url=url, headers=headers, files=files,
                      data=data or {}, json=json, params=params or {}, auth=auth,
                      cookies=cookies, hooks=hooks)
        prep = self.prepare_request(req)
        return self.send(prep, timeout=timeout, allow_redirects=allow_redirects)

    def send(self, request, timeout=None, allow_redirects=True, **kwargs):
        timeout = _timeout_seconds(timeout)
        extra_cookies = getattr(request, "_cookies", None)
        history = []
        prep = request
        while True:
            _check_url(prep.url)
            resp = self._exchange(prep, timeout, extra_cookies)
            for hook in prep.hooks.get("response", []):
                r = hook(resp)
                if r is not None:
                    resp = r
            target = core.redirect_target(prep.method, resp.status_code, prep.url,
                                          list(resp.headers.items()))
            if not allow_redirects or target is None:
                resp.history = history
                if not allow_redirects and target is not None:
                    nxt = prep.copy()
                    nxt.method, nxt.url = target[0], target[1]
                    resp._next = nxt
                return resp
            history.append(resp)
            if len(history) > self.max_redirects:
                raise TooManyRedirects(f"Exceeded {self.max_redirects} redirects.", response=resp)
            method, new_url, keep_body = target
            nxt = prep.copy()
            nxt.method, nxt.url = method, new_url
            if not keep_body:
                nxt.body = None
                for h in ("Content-Type", "Content-Length", "Transfer-Encoding"):
                    nxt.headers.pop(h, None)
            if not core.same_host(prep.url, new_url):
                nxt.headers.pop("Authorization", None)
            nxt.headers.pop("Cookie", None)
            prep = nxt

    def _exchange(self, prep, timeout, extra_cookies):
        _, host = core.split_url(prep.url)
        headers = [(k, v) for k, v in prep.headers.items() if k.lower() != "cookie"]
        explicit = prep.headers.get("Cookie")
        cookie = explicit or self.cookies._jar.header_for(host, extra_cookies)
        if cookie:
            headers.append(("Cookie", cookie))
        started = time.monotonic()
        try:
            status, rheaders, body = core.send(prep.method, prep.url, headers, prep.body, timeout)
        except TimeoutError as e:
            raise ReadTimeout(str(e), request=prep) from None
        except builtins.ConnectionError as e:
            raise ConnectionError(str(e), request=prep) from None
        except ValueError as e:
            msg = str(e)
            if "header" in msg.lower():
                raise InvalidHeader(msg, request=prep) from None
            raise InvalidURL(msg, request=prep) from None
        self.cookies._jar.update_from(host, rheaders)
        resp = Response()
        resp.status_code = status
        resp.reason = core.reason_phrase(status)
        resp.headers = CaseInsensitiveDict()
        for k, v in rheaders:
            if k in resp.headers and k.lower() != "set-cookie":
                resp.headers[k] = f"{resp.headers[k]}, {v}"
            else:
                resp.headers[k] = v
        try:
            resp._content = core.decode_body(rheaders, body) if prep.method != "HEAD" else b""
        except Exception as e:
            from .exceptions import ContentDecodingError

            raise ContentDecodingError(f"Received response with content-encoding, but failed to decode it: {e}",
                                       response=resp) from None
        charset = core.charset_of(rheaders)
        ctype = core.content_type_of(rheaders)
        if charset:
            resp.encoding = charset
        elif ctype.startswith("text/"):
            resp.encoding = "ISO-8859-1"
        elif ctype == "application/json":
            resp.encoding = "utf-8"
        resp.url = prep.url
        resp.request = prep
        resp.cookies = core.parse_set_cookies(rheaders)
        resp.elapsed = datetime.timedelta(seconds=time.monotonic() - started)
        return resp

    def get(self, url, **kwargs):
        kwargs.setdefault("allow_redirects", True)
        return self.request("GET", url, **kwargs)

    def options(self, url, **kwargs):
        kwargs.setdefault("allow_redirects", True)
        return self.request("OPTIONS", url, **kwargs)

    def head(self, url, **kwargs):
        kwargs.setdefault("allow_redirects", False)
        return self.request("HEAD", url, **kwargs)

    def post(self, url, data=None, json=None, **kwargs):
        return self.request("POST", url, data=data, json=json, **kwargs)

    def put(self, url, data=None, **kwargs):
        return self.request("PUT", url, data=data, **kwargs)

    def patch(self, url, data=None, **kwargs):
        return self.request("PATCH", url, data=data, **kwargs)

    def delete(self, url, **kwargs):
        return self.request("DELETE", url, **kwargs)


def session():
    return Session()
