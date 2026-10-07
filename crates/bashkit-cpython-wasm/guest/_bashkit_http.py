"""HTTP for the bashkit CPython guest: stdlib adapters over `_bashkit.http`.

`_bashkit.http` hands one request to the host, which runs it through
bashkit's egress pipeline (allowlist, SSRF checks, credential injection,
signing, the embedder's transport, size caps). The guest has no sockets and
no `ssl`; TLS is the host's.

Decisions:
- http.client keeps its own request and response code; only the socket is
  replaced. A connection gets a bridge socket that collects the bytes
  http.client writes, sends the request through the host once it is
  complete, and serves the reply as an HTTP/1.1 byte stream that the real
  `HTTPResponse` parses. Status line, headers, chunked uploads, keep-alive
  and HEAD behave as in CPython. Where errors surface matches a real
  socket: a refused or denied request fails in `request()` (urllib wraps it
  in `URLError`, like a failed connect), while a timeout waiting for the
  reply surfaces from `getresponse()` (a read timeout, which urllib3 and
  requests report as `ReadTimeout`).
- `HTTPSConnection` is defined although `ssl` is missing (context arguments
  are accepted and ignored), so urllib.request registers its https handler.
- Proxies and CONNECT tunnels are not used: the bridge sends to the tunnel
  target directly, and egress policy is the embedder's.
- Applied by a footer appended to `http/client.py` at build time, so nothing
  runs for scripts that never import http.client.
"""

import io
import sys

import _bashkit

_DEFAULT_PORTS = {"http": 80, "https": 443}


class _NoTLSContext:
    """Stand-in for `ssl.SSLContext`: the host verifies TLS itself."""

    check_hostname = True
    verify_mode = 2  # ssl.CERT_REQUIRED
    post_handshake_auth = None

    def __getattr__(self, name):
        # load_verify_locations, set_alpn_protocols, load_cert_chain, ...
        return lambda *args, **kwargs: None


def _parse_request(data):
    """Split one complete request off `data`.

    Returns (method, target, headers, body, rest) or None if incomplete.
    """
    end = data.find(b"\r\n\r\n")
    if end < 0:
        return None
    head = bytes(data[:end]).decode("latin-1").split("\r\n")
    method, target, _ = head[0].split(" ", 2)
    headers = []
    length = 0
    chunked = False
    for line in head[1:]:
        name, _, value = line.partition(":")
        value = value.strip()
        lower = name.strip().lower()
        if lower == "content-length":
            length = int(value)
        elif lower == "transfer-encoding" and "chunked" in value.lower():
            chunked = True
        headers.append((name.strip(), value))
    pos = end + 4
    if not chunked:
        if len(data) - pos < length:
            return None
        return method, target, headers, bytes(data[pos : pos + length]), pos + length
    body = bytearray()
    while True:
        eol = data.find(b"\r\n", pos)
        if eol < 0:
            return None
        size = int(bytes(data[pos:eol]).split(b";", 1)[0], 16)
        pos = eol + 2
        if size == 0:
            # Skip trailers up to the blank line.
            while True:
                eol = data.find(b"\r\n", pos)
                if eol < 0:
                    return None
                blank = eol == pos
                pos = eol + 2
                if blank:
                    return method, target, headers, bytes(body), pos
        if len(data) - pos < size + 2:
            return None
        body += data[pos : pos + size]
        pos += size + 2


def _status_line(status):
    from http import HTTPStatus

    try:
        reason = HTTPStatus(status).phrase
    except ValueError:
        reason = ""
    return f"HTTP/1.1 {status} {reason}\r\n"


class _BridgeSocket:
    """What http.client sees as its socket."""

    def __init__(self, scheme, host, port, timeout):
        if ":" in host and not host.startswith("["):
            host = f"[{host}]"
        if port is None or port == _DEFAULT_PORTS[scheme]:
            self._base = f"{scheme}://{host}"
        else:
            self._base = f"{scheme}://{host}:{port}"
        self._timeout = timeout if isinstance(timeout, (int, float)) else None
        self._out = bytearray()
        self._replies = []

    def sendall(self, data):
        self._out += data
        while True:
            parsed = _parse_request(self._out)
            if parsed is None:
                return
            method, target, headers, body, used = parsed
            del self._out[:used]
            try:
                reply = self._exchange(method, target, headers, body)
            except TimeoutError as e:
                reply = e  # raised when the response is read
            self._replies.append(reply)

    def send(self, data):
        self.sendall(data)
        return len(data)

    def _exchange(self, method, target, headers, body):
        if target.startswith(("http://", "https://")):
            url = target  # absolute form (proxy-style request)
        else:
            url = self._base + target
        headers = [
            (k, v) for k, v in headers if k.lower() not in ("host", "transfer-encoding")
        ]
        status, reply_headers, reply_body = _bashkit.http(
            method, url, headers, body, self._timeout
        )
        lines = [_status_line(status)]
        for name, value in reply_headers:
            lower = name.lower()
            if lower == "transfer-encoding":
                continue
            if lower == "content-length" and method != "HEAD":
                continue
            lines.append(f"{name}: {value}\r\n")
        if method != "HEAD":
            lines.append(f"Content-Length: {len(reply_body)}\r\n")
        lines.append("\r\n")
        return "".join(lines).encode("utf-8") + reply_body

    def makefile(self, mode="r", *args, **kwargs):
        if "w" in mode:
            raise OSError("bridge socket is read-only")
        if not self._replies:
            raise ConnectionError("no HTTP request was sent")
        reply = self._replies.pop(0)
        if isinstance(reply, BaseException):
            raise reply
        return io.BytesIO(reply)

    def settimeout(self, timeout):
        self._timeout = timeout

    def setsockopt(self, *args):
        pass

    def close(self):
        self._out.clear()
        self._replies.clear()


def patch_http_client(ns):
    """Route `http.client` connections through the host. `ns` is the
    module's globals (called from the footer of http/client.py)."""
    base = ns["HTTPConnection"]

    def connect(self):
        if self._tunnel_host:
            host, port = self._tunnel_host, self._tunnel_port
        else:
            host, port = self.host, self.port
        sys.audit("http.client.connect", self, host, port)
        self.sock = _BridgeSocket(self._bashkit_scheme, host, port, self.timeout)

    base.connect = connect
    base._bashkit_scheme = "http"

    if "HTTPSConnection" not in ns:
        socket = ns["socket"]

        class HTTPSConnection(base):
            "HTTPS connection; TLS is terminated by the bashkit host."

            default_port = ns["HTTPS_PORT"]
            _bashkit_scheme = "https"

            def __init__(
                self,
                host,
                port=None,
                *,
                timeout=socket._GLOBAL_DEFAULT_TIMEOUT,
                source_address=None,
                context=None,
                blocksize=8192,
            ):
                super().__init__(host, port, timeout, source_address, blocksize=blocksize)
                self._context = context

        HTTPSConnection.__module__ = "http.client"
        ns["HTTPSConnection"] = HTTPSConnection
        ns["__all__"].append("HTTPSConnection")
        ns["_create_https_context"] = lambda http_version: _NoTLSContext()
