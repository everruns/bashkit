//! `bashkit.http_request` host import: HTTP from the CPython guest through
//! bashkit's egress pipeline.
//!
//! Decisions (see `knowledge/runtimes/cpython-wasm.md`, TM-PY-CPY-003):
//! - Requests leave the guest at the HTTP level, never as sockets. TLS runs on
//!   the host, and every request goes through [`HttpClient`]: allowlist, SSRF
//!   precheck, `before_http` hooks (credential injection), signing, the
//!   embedder's transport, the response cap and `after_http` hooks. Python
//!   traffic is indistinguishable from `curl` at the egress boundary.
//! - No network configured (or no `http_client` feature) means every request
//!   fails with "network access not configured".
//! - The request is a length-prefixed binary record the guest's C module
//!   builds; the host validates it before dispatch (method, URL and header
//!   size, header name/value syntax) and drops headers the host owns (`Host`,
//!   framing, hop-by-hop), so a guest cannot smuggle a second request or
//!   aim at a virtual host the allowlist never checked.
//! - Redirects are not followed here: each hop is a new guest request and
//!   is re-checked, like `curl`.
//! - The response comes back in two steps (`http_request` reports its size,
//!   `http_take` copies it), so the guest allocates exactly once and the
//!   copy counts against its own memory limit.
//! - A per-call request cap bounds request floods.

#[cfg(feature = "http_client")]
use std::sync::Arc;
use std::time::Duration;

use wasmtime::{Caller, Linker};

use super::wasi::{GuestState, read_bytes, write_bytes, write_u32};

const MODULE: &str = "bashkit";

/// `http_request` / `http_take` status codes (mirrored in `bashkit_main.c`).
const OK: i32 = 0;
const NETWORK_ERROR: i32 = 1;
#[cfg_attr(not(feature = "http_client"), allow(dead_code))]
const TIMED_OUT: i32 = 2;
const INVALID_REQUEST: i32 = 3;
const FAULT: i32 = 4;

const MAX_URL_LEN: usize = 8192;
const MAX_HEADERS: usize = 128;
const MAX_HEADER_BYTES: usize = 64 * 1024;
/// Largest request body the guest may send (same order as the default
/// response cap).
const MAX_REQUEST_BODY: usize = 16 * 1024 * 1024;
/// Longest error text handed back to the guest (TM-INF-022).
const MAX_ERROR_LEN: usize = 512;

/// Headers the host (or reqwest) sets itself; guest values are dropped.
const HOST_OWNED_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "upgrade",
    "te",
    "trailer",
    "proxy-connection",
    "proxy-authorization",
];

/// Per-call HTTP state of one guest instance.
pub(crate) struct HttpState {
    #[cfg(feature = "http_client")]
    client: Option<Arc<crate::network::HttpClient>>,
    requests: usize,
    max_requests: usize,
    /// Encoded response (or error text) waiting for `http_take`.
    pending: Option<Vec<u8>>,
}

impl HttpState {
    pub(crate) fn new(
        #[cfg(feature = "http_client")] client: Option<Arc<crate::network::HttpClient>>,
        max_requests: usize,
    ) -> Self {
        Self {
            #[cfg(feature = "http_client")]
            client,
            requests: 0,
            max_requests,
            pending: None,
        }
    }
}

/// A validated guest request.
#[derive(Debug, PartialEq)]
pub(crate) struct Request {
    pub(crate) method: String,
    pub(crate) url: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, max: usize, what: &str) -> Result<&'a [u8], String> {
        let len = self.u32()? as usize;
        if len > max {
            return Err(format!("{what} too long"));
        }
        if self.buf.len() < len {
            return Err("truncated request".to_string());
        }
        let (head, rest) = self.buf.split_at(len);
        self.buf = rest;
        Ok(head)
    }

    fn str(&mut self, max: usize, what: &str) -> Result<&'a str, String> {
        std::str::from_utf8(self.bytes(max, what)?).map_err(|_| format!("{what} is not UTF-8"))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let Some((head, rest)) = self.buf.split_first_chunk::<4>() else {
            return Err("truncated request".to_string());
        };
        self.buf = rest;
        Ok(u32::from_le_bytes(*head))
    }
}

fn is_token(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Decode and validate `method, url, headers, body` (u32 little-endian
/// lengths and counts).
pub(crate) fn decode_request(buf: &[u8]) -> Result<Request, String> {
    let mut r = Reader { buf };
    let method = r.str(16, "method")?.to_ascii_uppercase();
    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "HEAD" | "PATCH"
    ) {
        return Err(format!("unsupported HTTP method: {method}"));
    }
    let url = r.str(MAX_URL_LEN, "URL")?.to_string();
    if url.bytes().any(|b| b.is_ascii_control() || b == b' ') {
        return Err("URL contains control characters or spaces".to_string());
    }
    let count = r.u32()? as usize;
    if count > MAX_HEADERS {
        return Err("too many headers".to_string());
    }
    let mut headers = Vec::with_capacity(count);
    let mut header_bytes = 0usize;
    for _ in 0..count {
        let name = r.str(MAX_HEADER_BYTES, "header name")?;
        let value = r.str(MAX_HEADER_BYTES, "header value")?;
        header_bytes += name.len() + value.len();
        if header_bytes > MAX_HEADER_BYTES {
            return Err("headers too large".to_string());
        }
        if !is_token(name) {
            return Err("invalid header name".to_string());
        }
        if value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
            return Err("invalid header value".to_string());
        }
        let lower = name.to_ascii_lowercase();
        if HOST_OWNED_HEADERS.contains(&lower.as_str()) {
            continue;
        }
        headers.push((name.to_string(), value.trim().to_string()));
    }
    let body = r.bytes(MAX_REQUEST_BODY, "request body")?.to_vec();
    if !r.buf.is_empty() {
        return Err("trailing bytes in request".to_string());
    }
    Ok(Request {
        method,
        url,
        headers,
        body,
    })
}

/// Decode `buf` and assert what the host guarantees about anything it
/// dispatches (fuzzing entry point, TM-PY-CPY-003).
pub(crate) fn check_request_invariants(buf: &[u8]) {
    let Ok(req) = decode_request(buf) else {
        return;
    };
    assert!(matches!(
        req.method.as_str(),
        "GET" | "POST" | "PUT" | "DELETE" | "HEAD" | "PATCH"
    ));
    assert!(req.url.len() <= MAX_URL_LEN);
    assert!(!req.url.bytes().any(|b| b.is_ascii_control() || b == b' '));
    assert!(req.headers.len() <= MAX_HEADERS);
    for (name, value) in &req.headers {
        assert!(is_token(name), "header name {name:?}"); // debug-ok: fuzz assertion
        assert!(!value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0));
        assert!(!HOST_OWNED_HEADERS.contains(&name.to_ascii_lowercase().as_str()));
    }
    assert!(req.body.len() <= MAX_REQUEST_BODY);
}

/// Encode `status, headers, body` for the guest.
#[cfg_attr(not(feature = "http_client"), allow(dead_code))]
pub(crate) fn encode_response(status: u16, headers: &[(String, String)], body: &[u8]) -> Vec<u8> {
    let size = 12
        + headers
            .iter()
            .map(|(k, v)| 8 + k.len() + v.len())
            .sum::<usize>()
        + body.len();
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(&u32::from(status).to_le_bytes());
    out.extend_from_slice(&(headers.len() as u32).to_le_bytes());
    for (k, v) in headers {
        out.extend_from_slice(&(k.len() as u32).to_le_bytes());
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(&(v.len() as u32).to_le_bytes());
        out.extend_from_slice(v.as_bytes());
    }
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    out
}

fn error_text(msg: &str) -> Vec<u8> {
    let mut msg = msg.to_string();
    msg.truncate(msg.floor_char_boundary(MAX_ERROR_LEN));
    msg.into_bytes()
}

/// Run one request; `Err((code, message))` on failure.
async fn send(
    state: &HttpState,
    request: Request,
    timeout: Duration,
) -> Result<Vec<u8>, (i32, String)> {
    #[cfg(feature = "http_client")]
    if let Some(client) = &state.client {
        use crate::network::Method;
        let method = match request.method.as_str() {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "HEAD" => Method::Head,
            _ => Method::Patch,
        };
        let body = (!request.body.is_empty()
            || matches!(method, Method::Post | Method::Put | Method::Patch))
        .then_some(request.body.as_slice());
        let secs = timeout.as_secs_f64().ceil() as u64;
        let fut = client.request_with_timeouts(
            method,
            &request.url,
            body,
            &request.headers,
            Some(secs),
            None,
        );
        return match tokio::time::timeout(timeout, fut).await {
            Err(_) => Err((TIMED_OUT, "timed out".to_string())),
            Ok(Ok(resp)) => Ok(encode_response(resp.status, &resp.headers, &resp.body)),
            Ok(Err(crate::Error::Network(msg))) if msg == "operation timed out" => {
                Err((TIMED_OUT, "timed out".to_string()))
            }
            Ok(Err(crate::Error::Network(msg))) => Err((NETWORK_ERROR, msg)),
            Ok(Err(e)) => Err((NETWORK_ERROR, e.to_string())),
        };
    }
    let _ = (state, request, timeout);
    Err((NETWORK_ERROR, "network access not configured".to_string()))
}

async fn http_request(
    c: &mut Caller<'_, GuestState>,
    ptr: i32,
    len: i32,
    timeout_ms: i64,
    out_len: i32,
) -> i32 {
    let raw = match read_bytes(c, ptr, len) {
        Ok(raw) => raw,
        Err(_) => return FAULT,
    };
    let (code, payload) = match decode_request(&raw) {
        Err(msg) => (INVALID_REQUEST, error_text(&msg)),
        Ok(request) => {
            let state = c.data_mut();
            state.http.pending = None;
            if state.http.requests >= state.http.max_requests {
                (
                    NETWORK_ERROR,
                    error_text(&format!(
                        "too many HTTP requests (limit {} per call)",
                        state.http.max_requests
                    )),
                )
            } else {
                state.http.requests += 1;
                // The call deadline always wins over the request's own timeout.
                let remaining = state.remaining();
                let timeout = if timeout_ms < 0 {
                    remaining
                } else {
                    remaining.min(Duration::from_millis(timeout_ms as u64))
                };
                match send(&state.http, request, timeout).await {
                    Ok(body) => (OK, body),
                    Err((code, msg)) => (code, error_text(&msg)),
                }
            }
        }
    };
    let Ok(size) = u32::try_from(payload.len()) else {
        return FAULT;
    };
    if write_u32(c, out_len, size).is_err() {
        return FAULT;
    }
    c.data_mut().http.pending = Some(payload);
    code
}

pub(crate) fn add_to_linker(linker: &mut Linker<GuestState>) -> wasmtime::Result<()> {
    linker.func_wrap_async(
        MODULE,
        "http_request",
        |mut c: Caller<'_, GuestState>, (ptr, len, timeout_ms, out_len): (i32, i32, i64, i32)| {
            Box::new(async move { http_request(&mut c, ptr, len, timeout_ms, out_len).await })
        },
    )?;
    linker.func_wrap(
        MODULE,
        "http_take",
        |mut c: Caller<'_, GuestState>, buf: i32, len: i32| -> i32 {
            let Some(pending) = c.data_mut().http.pending.take() else {
                return FAULT;
            };
            if pending.len() != len as u32 as usize || write_bytes(&mut c, buf, &pending).is_err() {
                return FAULT;
            }
            OK
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(method: &str, url: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut put = |b: &[u8]| {
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend_from_slice(b);
        };
        put(method.as_bytes());
        put(url.as_bytes());
        let mut rest = Vec::new();
        rest.extend_from_slice(&(headers.len() as u32).to_le_bytes());
        for (k, v) in headers {
            rest.extend_from_slice(&(k.len() as u32).to_le_bytes());
            rest.extend_from_slice(k.as_bytes());
            rest.extend_from_slice(&(v.len() as u32).to_le_bytes());
            rest.extend_from_slice(v.as_bytes());
        }
        rest.extend_from_slice(&(body.len() as u32).to_le_bytes());
        rest.extend_from_slice(body);
        out.extend_from_slice(&rest);
        out
    }

    #[test]
    fn decodes_a_request() {
        let raw = encode(
            "post",
            "https://example.com/x",
            &[("Accept", " text/plain "), ("X-A", "1")],
            b"hi",
        );
        let req = decode_request(&raw).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.url, "https://example.com/x");
        assert_eq!(
            req.headers,
            vec![
                ("Accept".to_string(), "text/plain".to_string()),
                ("X-A".to_string(), "1".to_string())
            ]
        );
        assert_eq!(req.body, b"hi");
    }

    #[test]
    fn drops_host_owned_headers() {
        let raw = encode(
            "GET",
            "http://a.test/",
            &[
                ("Host", "internal.test"),
                ("Content-Length", "0"),
                ("transfer-encoding", "chunked"),
                ("Connection", "keep-alive"),
                ("Proxy-Authorization", "x"),
                ("X-Keep", "y"),
            ],
            b"",
        );
        let req = decode_request(&raw).unwrap();
        assert_eq!(req.headers, vec![("X-Keep".to_string(), "y".to_string())]);
    }

    #[test]
    fn rejects_smuggling_and_bad_input() {
        let cases: Vec<Vec<u8>> = vec![
            encode("GET", "http://a.test/", &[("X", "a\r\nHost: b")], b""),
            encode("GET", "http://a.test/", &[("X Y", "a")], b""),
            encode("GET", "http://a.test/", &[("", "a")], b""),
            encode("GET", "http://a.test/ HTTP/1.1\r\n", &[], b""),
            encode("CONNECT", "http://a.test/", &[], b""),
            encode("GET\r\n", "http://a.test/", &[], b""),
            encode("GET", &"x".repeat(MAX_URL_LEN + 1), &[], b""),
            vec![1, 2, 3],
            Vec::new(),
        ];
        for raw in cases {
            assert!(decode_request(&raw).is_err(), "{raw:?}"); // debug-ok: test assert
        }
        let mut trailing = encode("GET", "http://a.test/", &[], b"");
        trailing.push(0);
        assert!(decode_request(&trailing).is_err());
        let many: Vec<(String, String)> = (0..=MAX_HEADERS)
            .map(|i| (format!("X-{i}"), "v".to_string()))
            .collect();
        let refs: Vec<(&str, &str)> = many.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert!(decode_request(&encode("GET", "http://a.test/", &refs, b"")).is_err());
    }

    #[test]
    fn truncated_lengths_never_panic() {
        let raw = encode("GET", "http://a.test/", &[("A", "b")], b"body");
        for cut in 0..=raw.len() {
            check_request_invariants(&raw[..cut]);
        }
        // A length prefix far past the buffer.
        let mut raw = Vec::new();
        raw.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_request(&raw).is_err());
    }

    #[test]
    fn encodes_a_response() {
        let out = encode_response(201, &[("a".to_string(), "b".to_string())], b"xyz");
        assert_eq!(&out[..4], &201u32.to_le_bytes());
        assert_eq!(&out[4..8], &1u32.to_le_bytes());
        assert_eq!(out.len(), 4 + 4 + (4 + 1 + 4 + 1) + 4 + 3);
        assert!(out.ends_with(b"xyz"));
    }
}
