//! Pluggable HTTP transport for the curl/wget/http builtins.
//!
//! Design decision (see `specs/http-transport.md`): bashkit owns HTTP
//! *policy* — URL allowlist, DNS/private-IP SSRF precheck, `before_http` /
//! `after_http` hooks, credential injection, bot-auth signing, and response
//! size caps — while an injected [`HttpTransport`] owns *connectivity*.
//! Embedding hosts implement the trait to route every request the sandbox
//! makes through their own outbound boundary (an egress service, a corporate
//! proxy, an audit/cache layer, or a test double). The shape mirrors
//! fetchkit's `HttpTransport` so a host can back both libraries with one
//! egress implementation.
//!
//! When no transport is injected, [`HttpClient`](super::HttpClient) uses its
//! built-in reqwest transport with connect-time private-IP filtering.

use std::net::IpAddr;
use std::time::Duration;

use super::client::{Method, Response};

/// Outbound HTTP request handed to an [`HttpTransport`].
///
/// Built by [`HttpClient`](super::HttpClient) after every in-sandbox policy
/// step has already run: the URL allowlist check, the DNS/private-IP SSRF
/// precheck, `before_http` hooks (including credential injection), and
/// bot-auth signing. `headers` therefore already carries injected credential
/// headers and `Signature`/`Signature-Input`/`Signature-Agent` signing
/// headers — a transport only moves bytes, it does not re-run policy.
///
/// Intentionally no `Debug` derive: `headers` can carry `Authorization`
/// values and signing material (TM-LOG-001 — no accidental secret logging).
#[non_exhaustive]
#[derive(Clone)]
pub struct HttpTransportRequest {
    /// HTTP method.
    pub method: Method,
    /// Absolute request URL (already allowlist-validated).
    pub url: String,
    /// Request headers, including bot-auth signing headers and injected
    /// credentials.
    pub headers: Vec<(String, String)>,
    /// Optional request body.
    pub body: Option<Vec<u8>>,
    /// Effective overall deadline for the request (curl `--max-time`,
    /// clamped to bashkit's timeout bounds, defaulting to the client
    /// timeout). Always present — bashkit *also* enforces this deadline
    /// around the transport call, so a transport that ignores it is still
    /// cut off; a transport that forwards it (e.g. to an egress request
    /// timeout) gives callers cleaner error reporting.
    pub timeout: Duration,
    /// Connect-phase deadline (curl `--connect-timeout`), when the script
    /// requested one. Transports that cannot separate connect from transfer
    /// may fold it into the overall deadline or ignore it.
    pub connect_timeout: Option<Duration>,
    /// IP addresses the SSRF precheck resolved and validated for the URL
    /// host (resolve-then-check). Empty when no resolution happened: the
    /// allowlist does not block private IPs, the host is an IP literal
    /// (then the literal itself was validated), or DNS failed on the
    /// documented fail-open path.
    ///
    /// Transports that dial the network themselves SHOULD connect to one of
    /// these addresses, or re-resolve and re-apply private-IP filtering
    /// (see [`HttpTransport`] docs). Host-boundary transports SHOULD forward
    /// them to the host (e.g. everruns `EgressRequest.pinned_addrs`) so the
    /// boundary can close the DNS-rebind TOCTOU window.
    pub pinned_addrs: Vec<IpAddr>,
    /// Response body cap in effect for this request. bashkit rejects larger
    /// responses after the transport returns; a well-behaved transport stops
    /// reading at this size and returns [`HttpTransportError::TooLarge`]
    /// instead of buffering an unbounded body.
    pub max_response_bytes: usize,
}

/// Typed failure returned by an [`HttpTransport`].
///
/// Variants map onto distinct curl/wget failure modes so host-side policy
/// decisions surface as the right script-visible exit codes:
///
/// | Variant | curl stderr prefix | curl exit code |
/// |---------|--------------------|----------------|
/// | [`Denied`](Self::Denied) | `access denied:` | 7 |
/// | [`Timeout`](Self::Timeout) | `operation timed out` | 28 |
/// | [`TooLarge`](Self::TooLarge) | `response too large:` | 63 |
/// | [`Transport`](Self::Transport) | message as-is | 1 |
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpTransportError {
    /// The host boundary refused the request as a matter of policy
    /// (network access list, deployment allowlist, tenant rules, ...).
    Denied(String),
    /// The request exceeded its deadline inside the transport.
    Timeout,
    /// The response exceeded [`HttpTransportRequest::max_response_bytes`]
    /// (or a stricter transport-side cap).
    TooLarge(String),
    /// Any other transport failure: connect error, TLS failure, protocol
    /// error, unreachable host, ...
    Transport(String),
}

impl std::fmt::Display for HttpTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Prefixes are load-bearing: curl/wget map them to exit codes
            // (see `curl_network_error_result`). Keep in sync.
            HttpTransportError::Denied(msg) => write!(f, "access denied: {msg}"),
            HttpTransportError::Timeout => write!(f, "operation timed out"),
            HttpTransportError::TooLarge(msg) => write!(f, "response too large: {msg}"),
            HttpTransportError::Transport(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for HttpTransportError {}

/// Pluggable transport for all outbound HTTP made by sandboxed scripts.
///
/// Implement this to direct `curl`/`wget`/`http` traffic through a
/// host-owned path — an egress service, proxy, audit log, cache, or mock.
/// Inject it with `BashBuilder::http_transport` (or
/// [`HttpClient::set_transport`](super::HttpClient::set_transport)); the
/// same `Arc` can be shared across many `Bash` instances.
///
/// Every policy step runs in bashkit *before* `execute` is called — see
/// [`HttpTransportRequest`] — and the redirect loop in curl/wget issues one
/// `execute` call per hop, so redirect targets are re-validated and
/// re-signed like fetchkit's per-hop transport calls.
///
/// # SSRF responsibility (TM-NET-023)
///
/// **Custom transports DO NOT inherit the built-in reqwest transport's
/// connect-time private-IP filter.** bashkit's DNS precheck is best-effort:
/// there is a rebind window between the precheck and the moment the
/// transport opens its own socket. A transport that performs real network
/// I/O MUST either connect only to [`HttpTransportRequest::pinned_addrs`]
/// (when non-empty), re-resolve and re-apply private-IP filtering itself
/// (`bashkit::network::allowlist::is_private_ip` is the same classifier the
/// built-in transport uses), or constrain its egress at a lower layer (the
/// typical host-boundary case). Transports that only consult fixtures or
/// in-memory state have no exposure here.
///
/// # Example
///
/// ```
/// use bashkit::{HttpTransport, HttpTransportError, HttpTransportRequest, HttpResponse};
///
/// /// Routes sandbox HTTP through a host-owned egress boundary.
/// struct EgressTransport;
///
/// #[async_trait::async_trait]
/// impl HttpTransport for EgressTransport {
///     async fn execute(
///         &self,
///         request: HttpTransportRequest,
///     ) -> Result<HttpResponse, HttpTransportError> {
///         // Forward method/url/headers/body/timeout/pinned_addrs to the
///         // host egress client here; map its policy denials to `Denied`.
///         if request.url.starts_with("https://blocked.internal") {
///             return Err(HttpTransportError::Denied(request.url));
///         }
///         Ok(HttpResponse { status: 200, headers: vec![], body: b"ok".to_vec() })
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait HttpTransport: Send + Sync {
    /// Execute one HTTP request and return the buffered response.
    async fn execute(
        &self,
        request: HttpTransportRequest,
    ) -> std::result::Result<Response, HttpTransportError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_prefixes_match_curl_exit_code_contract() {
        // curl_network_error_result keys off these prefixes; a silent change
        // here would flip script-visible exit codes.
        assert_eq!(
            HttpTransportError::Denied("policy".into()).to_string(),
            "access denied: policy"
        );
        assert_eq!(
            HttpTransportError::Timeout.to_string(),
            "operation timed out"
        );
        assert_eq!(
            HttpTransportError::TooLarge("5 bytes over".into()).to_string(),
            "response too large: 5 bytes over"
        );
        assert_eq!(
            HttpTransportError::Transport("connection refused".into()).to_string(),
            "connection refused"
        );
    }
}
