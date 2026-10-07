"""Differential test: bashkit's requests/httpx modules vs upstream.

Runs every scenario twice on the host CPython, once with bashkit's modules
(guest/requests, guest/httpx.py) over a fake `_bashkit.http`, once with the
real `requests` and `httpx` packages whose transport is patched to call the
same fake server. Both sides must print the same output. This catches API
drift; the bridge itself is covered by
crates/bashkit/tests/integration/cpython_http_tests.rs.

Usage: python3 differential.py   (needs `pip install requests httpx`)
"""

import json
import os
import subprocess
import sys

GUEST = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# --- the fake server, shared by both sides ---------------------------------

SERVER = r'''
import gzip, json
from urllib.parse import urlsplit

HOST_OWNED = {"host", "content-length", "transfer-encoding", "connection",
              "keep-alive", "upgrade", "te", "trailer", "proxy-connection",
              "proxy-authorization"}

class Timeout(Exception):
    pass

class Denied(Exception):
    pass

def route(method, url, headers, body):
    """What the bashkit host + a test server would answer."""
    headers = [(k, v) for k, v in headers if k.lower() not in HOST_OWNED]
    p = urlsplit(url)
    if p.hostname == "denied.test":
        raise Denied("access denied: URL not in allowlist")
    path = p.path
    status, rheaders, rbody = _route(method, p, path, headers, body)
    return status, rheaders, (b"" if method == "HEAD" else rbody)

def _route(method, p, path, headers, body):
    if path == "/json":
        return 200, [("Content-Type", "application/json")], json.dumps({"ok": True, "q": p.query}).encode()
    if path == "/echo":
        return 200, [("Content-Type", "text/plain; charset=utf-8")], method.encode() + b" " + (body or b"")
    if path == "/headers":
        seen = {k.lower(): v for k, v in headers if k.lower() not in ("user-agent", "accept-encoding", "accept")}
        return 200, [("Content-Type", "application/json")], json.dumps(seen, sort_keys=True).encode()
    if path == "/gzip":
        return 200, [("Content-Encoding", "gzip"), ("Content-Type", "text/plain")], gzip.compress(b"zipped")
    if path.startswith("/status/"):
        return int(path.split("/")[2]), [("Content-Type", "text/plain")], b"status body"
    if path == "/redirect":
        return 302, [("Location", "/json")], b""
    if path == "/see-other":
        return 303, [("Location", "/echo")], b""
    if path == "/temp":
        return 307, [("Location", "/echo")], b""
    if path == "/loop":
        return 302, [("Location", "/loop")], b""
    if path == "/cross":
        return 302, [("Location", "http://other.test/headers")], b""
    if path == "/redirect-out":
        return 302, [("Location", "http://denied.test/x")], b""
    if path == "/cookie":
        return 200, [("Set-Cookie", "sid=abc; Path=/"), ("Set-Cookie", "t=1")], b""
    if path == "/latin":
        return 200, [("Content-Type", "text/plain")], b"caf\xe9"
    if path == "/lines":
        return 200, [("Content-Type", "text/plain; charset=utf-8")], b"a\nb\nc"
    if path == "/link":
        return 200, [("Link", '<https://api.test/p2>; rel="next"')], b""
    if path == "/slow":
        raise Timeout("timed out")
    return 200, [("Content-Type", "text/plain")], b"hello"
'''

BASHKIT_SIDE = r'''
import sys, types
sys.path.insert(0, GUEST)
server = types.ModuleType("server"); exec(SERVER, server.__dict__)
fake = types.ModuleType("_bashkit")
def http(method, url, headers, body, timeout):
    try:
        return server.route(method, url, headers, body)
    except server.Timeout as e:
        raise TimeoutError(str(e))
    except server.Denied as e:
        raise ConnectionError(str(e))
fake.http = http
sys.modules["_bashkit"] = fake
'''

UPSTREAM_SIDE = r'''
import http.client, io, sys, types
from http import HTTPStatus
server = types.ModuleType("server"); exec(SERVER, server.__dict__)
import requests, requests.adapters, urllib3
import httpx

def _requests_send(self, request, stream=False, timeout=None, verify=True, cert=None, proxies=None):
    headers = list(request.headers.items())
    body = request.body.encode() if isinstance(request.body, str) else request.body
    try:
        status, rheaders, rbody = server.route(request.method, request.url, headers, body)
    except server.Timeout as e:
        raise requests.ReadTimeout(str(e), request=request)
    except server.Denied as e:
        raise requests.ConnectionError(str(e), request=request)
    hd = urllib3.HTTPHeaderDict()
    for k, v in rheaders:
        hd.add(k, v)
    resp = urllib3.HTTPResponse(body=io.BytesIO(rbody), headers=hd, status=status,
                                reason=HTTPStatus(status).phrase, preload_content=False,
                                decode_content=True, request_method=request.method)
    # What http.client would expose, so requests can read Set-Cookie.
    msg = http.client.HTTPMessage()
    for k, v in rheaders:
        msg[k] = v
    resp._original_response = types.SimpleNamespace(msg=msg, info=lambda: msg, isclosed=lambda: True, close=lambda: None)
    return self.build_response(request, resp)

requests.adapters.HTTPAdapter.send = _requests_send

def _httpx_handle(self, request):
    # httpcore's check, which this patch bypasses.
    if request.url.scheme not in ("http", "https"):
        raise httpx.UnsupportedProtocol("unsupported protocol", request=request)
    try:
        status, rheaders, rbody = server.route(request.method, str(request.url),
                                               list(request.headers.multi_items()), request.read())
    except server.Timeout as e:
        raise httpx.ReadTimeout(str(e), request=request)
    except server.Denied as e:
        raise httpx.ConnectError(str(e), request=request)
    return httpx.Response(status, headers=rheaders, stream=httpx.ByteStream(rbody), request=request)

async def _httpx_handle_async(self, request):
    return _httpx_handle(self, request)

httpx.HTTPTransport.handle_request = _httpx_handle
httpx.AsyncHTTPTransport.handle_async_request = _httpx_handle_async
'''

# --- scenarios ------------------------------------------------------------

SCENARIOS = {
    "requests_basics": r'''
import requests
r = requests.get("https://api.test/json", params={"q": "a b", "n": [1, 2]})
print(r.status_code, r.ok, r.reason, r.headers["content-type"], r.encoding, r.json(), r.url)
print(requests.post("http://api.test/echo", json={"k": 1, "u": "é"}).text)
print(requests.post("http://api.test/echo", data={"a": "1 2", "b": ["x", "y"]}).text)
print(requests.put("http://api.test/echo", data=b"raw").text, requests.patch("http://api.test/echo", data="s").text)
print(requests.delete("http://api.test/echo").text)
r = requests.head("http://api.test/json"); print(r.status_code, repr(r.text))
print(requests.get("http://api.test/headers", headers={"X-A": "1"}, auth=("u", "p")).json())
print(requests.get("http://api.test/headers", cookies={"c": "1"}).json())
''',
    "requests_errors": r'''
import requests
for code in (404, 500):
    r = requests.get(f"http://api.test/status/{code}")
    print(r.status_code, r.ok, bool(r))
    try:
        r.raise_for_status()
    except requests.HTTPError as e:
        print(type(e).__name__, e, e.response.status_code)
for url in ("api.test/x", "ftp://api.test/x", "http:///x"):
    try:
        requests.get(url)
    except requests.RequestException as e:
        print(type(e).__name__, isinstance(e, ValueError))
try:
    requests.get("http://api.test/slow", timeout=1)
except requests.Timeout as e:
    print(type(e).__name__, isinstance(e, requests.ConnectionError))
try:
    requests.get("http://denied.test/")
except requests.ConnectionError as e:
    print(type(e).__name__)
try:
    requests.get("http://api.test/plain").json()
except requests.JSONDecodeError as e:
    print(type(e).__name__, isinstance(e, ValueError))
print(issubclass(requests.ReadTimeout, requests.Timeout), issubclass(requests.ConnectTimeout, requests.ConnectionError))
''',
    "requests_redirects": r'''
import requests
r = requests.get("http://api.test/redirect")
print(r.status_code, r.url, [h.status_code for h in r.history], r.history[0].is_redirect)
r = requests.get("http://api.test/redirect", allow_redirects=False)
print(r.status_code, r.is_redirect, r.headers["location"])
print(repr(requests.post("http://api.test/see-other", data="x").text))
print(requests.post("http://api.test/temp", data="x").text)
try:
    requests.get("http://api.test/loop")
except requests.TooManyRedirects as e:
    print(type(e).__name__)
print(requests.get("http://api.test/cross", auth=("u", "p")).json())
try:
    requests.get("http://api.test/redirect-out")
except requests.ConnectionError as e:
    print(type(e).__name__)
''',
    "requests_session_and_body": r'''
import requests
s = requests.Session()
s.headers.update({"X-S": "1"})
s.params = {"p": "1"}
s.get("http://api.test/cookie")
print(sorted(s.cookies.items()), s.cookies.get("sid"))
print(s.get("http://api.test/headers").json())
print(requests.get("http://api.test/json").json(), s.get("http://api.test/json").json())
print(requests.get("http://api.test/gzip").text)
r = requests.get("http://api.test/latin"); print(r.encoding, r.text)
r = requests.get("http://api.test/lines")
print(list(r.iter_lines()), list(r.iter_lines(decode_unicode=True)), list(r.iter_content(2)))
print(requests.get("http://api.test/link").links)
print(requests.codes.ok, requests.codes.not_found, requests.codes["teapot"])
seen = []
requests.get("http://api.test/plain", hooks={"response": lambda r, *a, **k: seen.append(r.status_code)})
print(seen)
with requests.Session() as s2:
    print(s2.get("http://api.test/plain").text)
''',
    "httpx_basics": r'''
import httpx
r = httpx.get("https://api.test/json", params={"q": "a b"})
print(r, r.status_code, r.reason_phrase, r.http_version, r.headers["content-type"], r.json(), r.url, r.is_success)
print(httpx.post("http://api.test/echo", json={"k": 1, "u": "é"}).text)
print(httpx.post("http://api.test/echo", data={"a": "1 2"}).text)
print(httpx.post("http://api.test/echo", content=b"raw").text, httpx.put("http://api.test/echo", content="s").text)
print(httpx.get("http://api.test/headers", headers={"X-A": "1"}, auth=("u", "p")).json())
print(httpx.get("http://api.test/gzip").text)
r = httpx.get("http://api.test/lines")
print(list(r.iter_lines()), list(r.iter_bytes(2)), r.encoding)
print(httpx.get("http://api.test/link").links)
print(httpx.codes.OK, httpx.codes.NOT_FOUND, httpx.Timeout(10.0), httpx.Timeout(5.0, connect=1.0))
u = httpx.URL("https://a.test/p?x=1&y=2")
print(u.host, u.path, u.query, u.params["y"], u.scheme, u.join("/q"), u.copy_merge_params({"z": 3}))
h = httpx.Headers([("A", "1"), ("a", "2")])
print(h["a"], h.get_list("A"), list(h.multi_items()))
q = httpx.QueryParams({"a": ["1", "2"], "b": "x"})
print(str(q), q.get_list("a"), q["a"])
''',
    "httpx_errors_and_redirects": r'''
import httpx
r = httpx.get("http://api.test/redirect")
print(r.status_code, r.is_redirect, r.has_redirect_location, r.next_request.url)
for code in (404, 500, 302):
    path = "redirect" if code == 302 else f"status/{code}"
    try:
        httpx.get(f"http://api.test/{path}").raise_for_status()
    except httpx.HTTPStatusError as e:
        print(type(e).__name__, e.response.status_code, str(e).splitlines()[0])
with httpx.Client(follow_redirects=True) as c:
    r = c.get("http://api.test/redirect")
    print(r.status_code, r.url, [h.status_code for h in r.history])
    print(repr(c.post("http://api.test/see-other", content=b"x").text))
    print(c.post("http://api.test/temp", content=b"x").text)
    print(c.get("http://api.test/cross", auth=("u", "p")).json())
    try:
        c.get("http://api.test/loop")
    except httpx.TooManyRedirects as e:
        print(type(e).__name__)
for url in ("api.test/x", "ftp://api.test/x"):
    try:
        httpx.get(url)
    except httpx.HTTPError as e:
        print(type(e).__name__)
try:
    httpx.get("http://api.test/slow")
except httpx.TimeoutException as e:
    print(type(e).__name__, isinstance(e, httpx.TransportError))
try:
    httpx.get("http://denied.test/")
except httpx.ConnectError as e:
    print(type(e).__name__, e.request.url)
''',
    "httpx_client_and_async": r'''
import asyncio, httpx
with httpx.Client(base_url="http://api.test/", headers={"X-C": "1"}, params={"p": "1"}) as c:
    c.get("/cookie")
    print(dict(c.cookies), c.base_url)
    print(c.get("headers").json())
    print(c.get("json").json())
    with c.stream("GET", "/lines") as r:
        print(list(r.iter_lines()))
seen = []
with httpx.Client(event_hooks={"request": [lambda r: seen.append(r.method)],
                               "response": [lambda r: seen.append(r.status_code)]}) as c:
    c.get("http://api.test/plain")
print(seen)
def handler(request):
    return httpx.Response(201, json={"path": request.url.path, "m": request.method})
with httpx.Client(transport=httpx.MockTransport(handler)) as c:
    r = c.post("https://api.test/items")
    print(r.status_code, r.json(), r.headers["content-type"])
async def main():
    async with httpx.AsyncClient() as c:
        rs = await asyncio.gather(c.get("http://api.test/json"), c.post("http://api.test/echo", data={"a": "b"}))
        print([r.status_code for r in rs], rs[1].text)
        async with c.stream("GET", "http://api.test/lines") as r:
            print([line async for line in r.aiter_lines()])
asyncio.run(main())
''',
    "requests_misc": r'''
import io, datetime, requests
r = requests.get("http://api.test/json", timeout=(1, 2))
print(repr(r), isinstance(r.elapsed, datetime.timedelta), r.request.method, r.request.url)
s = requests.Session()
p = s.prepare_request(requests.Request("POST", "http://api.test/echo", json={"a": 1}, headers={"X": "1"}))
print(p.method, p.url, p.body, p.headers["Content-Type"], p.headers["X"])
print(s.send(p).text)
print(requests.post("http://api.test/echo", data="plain text").text)
r = requests.post("http://api.test/echo", files={"f": io.BytesIO(b"FILEDATA")}, data={"k": "v"})
print("FILEDATA" in r.text, 'name="k"' in r.text, 'filename="f"' in r.text)
print(requests.get("http://api.test/headers", headers={"X-Drop": None, "X-Keep": "1"}).json())
print(requests.get("http://api.test/json", params="raw=1").url)
print(requests.get("http://api.test/json", params={"none": None, "t": True}).url)
print(requests.structures.CaseInsensitiveDict({"A": 1})["a"], requests.exceptions.HTTPError.__mro__[1].__name__)
''',
    "httpx_misc": r'''
import httpx
r = httpx.get("http://api.test/json")
print(r.request.method, r.request.url, r.request.headers["host"] if "host" in r.request.headers else "-", r.elapsed.total_seconds() >= 0)
r = httpx.Response(200, text="hi")
print(r.text, r.headers["content-type"], r.encoding)
req = httpx.Request("POST", "https://api.test/x", json={"a": 1})
print(req.method, req.url, req.content, req.headers["content-type"], req.headers["content-length"])
c = httpx.Client(timeout=httpx.Timeout(3.0))
print(c.timeout, c.follow_redirects, c.headers["user-agent"].startswith("python-httpx/"))
print(c.build_request("GET", "http://api.test/p", params={"a": 1}).url)
print(httpx.post("http://api.test/echo", files={"f": ("n.txt", b"DATA", "text/plain")}).text.count("DATA"))
cookies = httpx.Cookies({"a": "1"}); cookies.set("b", "2", domain="api.test")
print(sorted(cookies.items()), cookies.get("b"))
print(httpx.get("http://api.test/headers", cookies={"c": "1"}).json())
''',
}


def run(side, code):
    prelude = (f"GUEST = {GUEST!r}\nSERVER = {SERVER!r}\n"
               + (BASHKIT_SIDE if side == "bashkit" else UPSTREAM_SIDE))
    proc = subprocess.run([sys.executable, "-I", "-c", prelude + code],
                          capture_output=True, text=True, timeout=60)
    out = proc.stdout
    if proc.returncode:
        out += "EXIT %d: %s" % (proc.returncode, proc.stderr.strip().splitlines()[-1:])
    return out


def main():
    failed = 0
    for name, code in SCENARIOS.items():
        ours, theirs = run("bashkit", code), run("upstream", code)
        if ours == theirs:
            print(f"ok   {name}")
            continue
        failed += 1
        print(f"FAIL {name}")
        a, b = ours.splitlines(), theirs.splitlines()
        for i in range(max(len(a), len(b))):
            x = a[i] if i < len(a) else "<missing>"
            y = b[i] if i < len(b) else "<missing>"
            if x != y:
                print(f"  line {i + 1}\n    bashkit:  {x}\n    upstream: {y}")
    print(json.dumps({"scenarios": len(SCENARIOS), "failed": failed}))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
