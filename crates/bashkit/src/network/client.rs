//! HTTP client for secure network access.
//!
//! Provides a virtual HTTP client that respects the allowlist with
//! security mitigations for common HTTP attacks.
//!
//! # Platform Support
//!
//! - **Native** (default): Uses `reqwest` with a private-IP-filtering DNS
//!   resolver, streaming response bodies, and per-request timeout clients.
//!   See `client::native` for the native transport implementation.
//! - **WASM** (`target_family = "wasm")`: Uses the browser's `fetch` API
//!   via `web_sys` and `wasm_bindgen_futures`. DNS checks are limited to
//!   literal-IP blocking because the browser does not expose raw socket or
//!   DNS APIs. Timeouts are enforced with `AbortController`.
//!   See `client_wasm` for the WASM transport implementation.
//!
//! # Security Mitigations
//!
//! This module mitigates the following threats (see `specs/threat-model.md`):
//!
//! - **TM-NET-008**: Large response DoS → `max_response_bytes` limit (10MB default)
//! - **TM-NET-009**: Connection hang → connect timeout (10s) *(native only)*
//! - **TM-NET-010**: Slowloris attack → read timeout (30s)
//! - **TM-NET-011**: Redirect bypass → no auto-redirect on native; browser CORS
//!   and same-origin policy provide additional defense on WASM.
//! - **TM-NET-012**: Chunked encoding bomb → streaming size check *(native)* /
//!   `Content-Length` pre-check + array-buffer limit *(WASM)*
//! - **TM-NET-013**: Gzip/compression bomb → auto-decompression disabled *(native)*
//! - **TM-NET-014**: DNS rebind via redirect → manual redirect requires allowlist check
//! - **TM-NET-015**: Host proxy leakage → `.no_proxy()` ignores host `HTTP_PROXY`/`HTTPS_PROXY` *(native)*
//! - **TM-NET-002 (TOCTOU)**: DNS rebinding between pre-resolve check and actual connect →
//!   private-IP filtering installed as reqwest's DNS resolver on native. WASM relies on
//!   the browser's same-origin policy and the allowlist pre-check.

#[cfg(not(target_family = "wasm"))]
use std::sync::OnceLock;
use std::time::Duration;

use super::allowlist::{NetworkAllowlist, UrlMatch, is_private_ip};
use crate::error::{Error, Result};

#[cfg(not(target_family = "wasm"))]
mod native;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default maximum response body size (10 MB)
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

/// Default request timeout (30 seconds)
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Maximum allowed timeout (10 minutes) - prevents resource exhaustion from very long timeouts
pub const MAX_TIMEOUT_SECS: u64 = 600;

/// Minimum allowed timeout (1 second) - prevents instant timeouts that waste resources
pub const MIN_TIMEOUT_SECS: u64 = 1;

// ---------------------------------------------------------------------------
// HttpHandler trait
// ---------------------------------------------------------------------------

/// Trait for custom HTTP request handling.
///
/// Embedders can implement this trait to intercept, proxy, log, cache,
/// or mock HTTP requests made by scripts running in the sandbox.
///
/// The allowlist check and the DNS / private-IP precheck both run
/// _before_ the handler is called, and the precheck fails closed on
/// DNS lookup errors (#1570). The security boundary stays in bashkit
/// for allowlist policy.
///
/// # Default
///
/// When no custom handler is set, `HttpClient` uses the platform
/// default (`reqwest` on native, `fetch` on WASM).
///
/// # SSRF responsibility for handlers (TM-NET-023, #1570)
///
/// **Custom HTTP handlers DO NOT inherit the platform's connect-time IP
/// filter.** The DNS precheck bashkit runs is best-effort and is
/// vulnerable to a rebind window between the precheck and the moment
/// the handler opens its own socket. If a handler performs real network
/// I/O (proxies, custom transports, sidecar HTTP clients) it MUST
/// re-resolve the host and re-apply private-IP filtering itself before
/// connecting, or constrain its egress at a lower layer. The internal
/// classifier `bashkit::network::allowlist::is_private_ip` (re-exported
/// at `bashkit::network::is_private_ip` when used from inside this
/// crate) is the same one the default native path uses. Handlers that
/// only consult fixtures or in-memory state (mocks, test doubles) have
/// no exposure here.
#[async_trait::async_trait]
pub trait HttpHandler: Send + Sync {
    /// Handle an HTTP request and return a response.
    ///
    /// Called after the URL has been validated against the allowlist
    /// and the DNS / private-IP precheck. See the trait-level
    /// documentation for SSRF responsibilities of network-capable
    /// handlers.
    async fn request(
        &self,
        method: &str,
        url: &str,
        body: Option<&[u8]>,
        headers: &[(String, String)],
    ) -> std::result::Result<Response, String>;
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

/// HTTP response
#[derive(Debug)]
pub struct Response {
    /// HTTP status code
    pub status: u16,
    /// Response headers (key-value pairs)
    pub headers: Vec<(String, String)>,
    /// Response body
    pub body: Vec<u8>,
}

impl Response {
    /// Get the body as a UTF-8 string (lossy)
    pub fn body_string(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Check if the response was successful (2xx status)
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

// ---------------------------------------------------------------------------
// Method
// ---------------------------------------------------------------------------

/// HTTP request method
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Head,
    Patch,
}

impl Method {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Patch => "PATCH",
        }
    }
}

// ---------------------------------------------------------------------------
// HttpClient
// ---------------------------------------------------------------------------

/// HTTP client with allowlist-based access control.
///
/// # Security Features
///
/// - URL allowlist enforcement
/// - Response size limits to prevent memory exhaustion
/// - Configurable timeouts to prevent hanging
/// - No automatic redirect following (to prevent allowlist bypass)
pub struct HttpClient {
    #[cfg(not(target_family = "wasm"))]
    client: OnceLock<std::result::Result<reqwest::Client, String>>,
    #[cfg(target_family = "wasm")]
    _wasm_marker: std::marker::PhantomData<()>,
    allowlist: NetworkAllowlist,
    default_timeout: Duration,
    /// Maximum response body size in bytes
    max_response_bytes: usize,
    /// Optional custom HTTP handler for request interception
    handler: Option<Box<dyn HttpHandler>>,
    /// Optional bot-auth config for transparent request signing
    #[cfg(feature = "bot-auth")]
    bot_auth: Option<super::bot_auth::BotAuthConfig>,
    /// Interceptor hooks fired before each HTTP request
    before_http: Vec<crate::hooks::Interceptor<crate::hooks::HttpRequestEvent>>,
    /// Interceptor hooks fired after each HTTP response
    after_http: Vec<crate::hooks::Interceptor<crate::hooks::HttpResponseEvent>>,
}

impl HttpClient {
    /// Create a new HTTP client with the given allowlist.
    ///
    /// Uses default security settings:
    /// - 30 second timeout
    /// - 10 MB max response size
    /// - No automatic redirects
    pub fn new(allowlist: NetworkAllowlist) -> Self {
        Self::with_config(
            allowlist,
            Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            DEFAULT_MAX_RESPONSE_BYTES,
        )
    }

    /// Create a client with custom timeout.
    pub fn with_timeout(allowlist: NetworkAllowlist, timeout: Duration) -> Self {
        Self::with_config(allowlist, timeout, DEFAULT_MAX_RESPONSE_BYTES)
    }

    /// Create a client with full configuration.
    ///
    /// # Arguments
    ///
    /// * `allowlist` - URL patterns to allow
    /// * `timeout` - Request timeout duration
    /// * `max_response_bytes` - Maximum response body size (prevents memory exhaustion)
    pub fn with_config(
        allowlist: NetworkAllowlist,
        timeout: Duration,
        max_response_bytes: usize,
    ) -> Self {
        Self {
            #[cfg(not(target_family = "wasm"))]
            client: OnceLock::new(),
            #[cfg(target_family = "wasm")]
            _wasm_marker: std::marker::PhantomData,
            allowlist,
            default_timeout: timeout,
            max_response_bytes,
            handler: None,
            #[cfg(feature = "bot-auth")]
            bot_auth: None,
            before_http: Vec::new(),
            after_http: Vec::new(),
        }
    }

    /// Set a custom HTTP handler for request interception.
    ///
    /// The handler is called after the URL allowlist check, so the security
    /// boundary stays in bashkit. The default platform handler is used
    /// when no custom handler is set.
    pub fn set_handler(&mut self, handler: Box<dyn HttpHandler>) {
        self.handler = Some(handler);
    }

    /// Enable bot-auth request signing.
    ///
    /// When set, all outbound HTTP requests are transparently signed with
    /// Ed25519 per RFC 9421 / web-bot-auth profile. No CLI arguments needed.
    /// Signing failures are non-blocking — the request is sent unsigned.
    #[cfg(feature = "bot-auth")]
    pub fn set_bot_auth(&mut self, config: super::bot_auth::BotAuthConfig) {
        self.bot_auth = Some(config);
    }

    /// Produce bot-auth signing headers for the given request.
    /// Non-blocking: signing failures return an empty vec (request sent unsigned).
    #[cfg(feature = "bot-auth")]
    fn bot_auth_headers(&self, method: Method, url: &str) -> Vec<(String, String)> {
        let Some(ref bot_auth) = self.bot_auth else {
            return Vec::new();
        };
        let Ok(parsed) = url::Url::parse(url) else {
            return Vec::new();
        };
        match bot_auth.sign_request(method.as_str(), parsed.as_str()) {
            Ok(headers) => {
                let mut result = vec![
                    ("signature".to_string(), headers.signature),
                    ("signature-input".to_string(), headers.signature_input),
                ];
                if let Some(fqdn) = headers.signature_agent {
                    result.push(("signature-agent".to_string(), fqdn));
                }
                result
            }
            Err(_e) => {
                // Non-blocking: signing failure must not prevent the request
                Vec::new()
            }
        }
    }

    /// Set `before_http` interceptor hooks.
    ///
    /// Hooks fire before each HTTP request (after allowlist check).
    /// They can inspect, modify, or cancel the request.
    pub fn set_before_http(
        &mut self,
        hooks: Vec<crate::hooks::Interceptor<crate::hooks::HttpRequestEvent>>,
    ) {
        self.before_http = hooks;
    }

    /// Set `after_http` interceptor hooks.
    ///
    /// Hooks fire after each HTTP response is received.
    /// They can inspect or modify the response metadata.
    pub fn set_after_http(
        &mut self,
        hooks: Vec<crate::hooks::Interceptor<crate::hooks::HttpResponseEvent>>,
    ) {
        self.after_http = hooks;
    }

    /// Fire `before_http` hooks. Returns the (possibly modified) event,
    /// or `None` if a hook cancelled the request.
    fn fire_before_http(
        &self,
        event: crate::hooks::HttpRequestEvent,
    ) -> Option<crate::hooks::HttpRequestEvent> {
        if self.before_http.is_empty() {
            return Some(event);
        }
        let mut current = event;
        for hook in &self.before_http {
            match hook(current) {
                crate::hooks::HookAction::Continue(e) => current = e,
                crate::hooks::HookAction::Cancel(_) => return None,
            }
        }
        Some(current)
    }

    /// Fire `after_http` hooks (observational).
    fn fire_after_http(&self, event: crate::hooks::HttpResponseEvent) {
        if self.after_http.is_empty() {
            return;
        }
        let mut current = event;
        for hook in &self.after_http {
            match hook(current) {
                crate::hooks::HookAction::Continue(e) => current = e,
                crate::hooks::HookAction::Cancel(_) => return,
            }
        }
    }

    /// Make a GET request.
    pub async fn get(&self, url: &str) -> Result<Response> {
        self.request(Method::Get, url, None).await
    }

    /// Make a POST request with optional body.
    pub async fn post(&self, url: &str, body: Option<&[u8]>) -> Result<Response> {
        self.request(Method::Post, url, body).await
    }

    /// Make a PUT request with optional body.
    pub async fn put(&self, url: &str, body: Option<&[u8]>) -> Result<Response> {
        self.request(Method::Put, url, body).await
    }

    /// Make a DELETE request.
    pub async fn delete(&self, url: &str) -> Result<Response> {
        self.request(Method::Delete, url, None).await
    }

    /// Make an HTTP request.
    pub async fn request(
        &self,
        method: Method,
        url: &str,
        body: Option<&[u8]>,
    ) -> Result<Response> {
        self.request_with_headers(method, url, body, &[]).await
    }

    fn check_allowlist(&self, url: &str) -> Result<()> {
        match self.allowlist.check(url) {
            UrlMatch::Allowed => Ok(()),
            UrlMatch::Blocked { reason } => {
                Err(Error::Network(format!("access denied: {}", reason)))
            }
            UrlMatch::Invalid { reason } => Err(Error::Network(format!("invalid URL: {}", reason))),
        }
    }

    /// Validate a URL against the same allowlist and private-IP policy used before requests.
    pub(crate) async fn validate_url(&self, url: &str) -> Result<()> {
        self.enforce_url_security(url).await
    }

    pub(crate) async fn enforce_url_security(&self, url: &str) -> Result<()> {
        self.check_allowlist(url)?;
        if self.allowlist.is_blocking_private_ips() {
            self.check_private_ip(url).await?;
        }
        Ok(())
    }

    /// THREAT[TM-NET-002/004/023]: Pre-resolve DNS and block private IPs.
    ///
    /// On native this performs a full DNS lookup and blocks private resolves.
    /// On WASM only literal IPs are checked; hostname resolution is deferred
    /// to the browser, which applies same-origin policy and CORS.
    pub(crate) async fn check_private_ip(&self, url: &str) -> Result<()> {
        let parsed = url::Url::parse(url)
            .map_err(|e| Error::Network(format!("invalid URL for SSRF precheck: {e}")))?;
        let Some(host) = parsed.host_str() else {
            return Err(Error::Network(
                "access denied: URL has no host (SSRF protection)".to_string(),
            ));
        };
        // Strip brackets from IPv6 literals so they parse correctly.
        let ip_str = host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(host);
        if let Ok(ip) = ip_str.parse::<std::net::IpAddr>() {
            if is_private_ip(&ip) {
                return Err(Error::Network(format!(
                    "access denied: {} is a private IP (SSRF protection)",
                    host
                )));
            }
            return Ok(());
        }

        // Native: perform DNS precheck.
        #[cfg(not(target_family = "wasm"))]
        {
            let port = parsed
                .port()
                .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });
            let addr = format!("{}:{}", host, port);
            let Ok(addrs) = tokio::net::lookup_host(&addr).await else {
                // DNS lookup failed — fall through. See the function-level
                // doc for why this stays fail-open.
                return Ok(());
            };
            for a in addrs {
                if is_private_ip(&a.ip()) {
                    return Err(Error::Network(format!(
                        "access denied: {} resolves to private IP {} (SSRF protection)",
                        host,
                        a.ip()
                    )));
                }
            }
        }

        Ok(())
    }

    /// Make an HTTP request with custom headers.
    ///
    /// # Security
    ///
    /// - URL is validated against the allowlist before making the request
    /// - Response body is limited to `max_response_bytes` to prevent memory exhaustion
    /// - Redirects are not automatically followed (to prevent allowlist bypass)
    pub async fn request_with_headers(
        &self,
        method: Method,
        url: &str,
        body: Option<&[u8]>,
        headers: &[(String, String)],
    ) -> Result<Response> {
        // Check allowlist + private IP policy BEFORE making any network request.
        self.enforce_url_security(url).await?;

        // Fire before_http hooks — may modify URL/headers or cancel the request.
        // Hooks fire AFTER the allowlist check so the security boundary stays in bashkit.
        let (url, headers) = if !self.before_http.is_empty() {
            let event = crate::hooks::HttpRequestEvent {
                method: method.as_str().to_string(),
                url: url.to_string(),
                headers: headers.to_vec(),
            };
            match self.fire_before_http(event) {
                Some(modified) => (
                    std::borrow::Cow::Owned(modified.url),
                    std::borrow::Cow::Owned(modified.headers),
                ),
                None => {
                    return Err(Error::Network("cancelled by before_http hook".to_string()));
                }
            }
        } else {
            (
                std::borrow::Cow::Borrowed(url),
                std::borrow::Cow::Borrowed(headers),
            )
        };
        let url: &str = &url;
        let headers: &[(String, String)] = &headers;
        // Re-check security after hooks in case URL was rewritten.
        self.enforce_url_security(url).await?;

        // Compute bot-auth signing headers (transparent, non-blocking)
        #[cfg(feature = "bot-auth")]
        let signing_headers = self.bot_auth_headers(method, url);
        #[cfg(not(feature = "bot-auth"))]
        let signing_headers: Vec<(String, String)> = Vec::new();

        // Delegate to custom handler if set
        if let Some(handler) = &self.handler {
            let method_str = method.as_str();
            let mut all_headers: Vec<(String, String)> = headers.to_vec();
            all_headers.extend(signing_headers);
            #[cfg(not(target_family = "wasm"))]
            let response = tokio::time::timeout(
                self.default_timeout,
                handler.request(method_str, url, body, &all_headers),
            )
            .await
            .map_err(|_| Error::Network("operation timed out".to_string()))?
            .map_err(Error::Network)?;
            #[cfg(target_family = "wasm")]
            let response = handler
                .request(method_str, url, body, &all_headers)
                .await
                .map_err(Error::Network)?;
            if response.body.len() > self.max_response_bytes {
                return Err(Error::Network(format!(
                    "response too large: {} bytes (max: {} bytes)",
                    response.body.len(),
                    self.max_response_bytes
                )));
            }
            self.fire_after_http(crate::hooks::HttpResponseEvent {
                url: url.to_string(),
                status: response.status,
                headers: response.headers.clone(),
            });
            return Ok(response);
        }

        self.send_request(
            method,
            url,
            body,
            headers,
            signing_headers,
            self.default_timeout,
            None,
        )
        .await
    }

    /// Make a HEAD request to get headers without body.
    pub async fn head(&self, url: &str) -> Result<Response> {
        self.request(Method::Head, url, None).await
    }

    /// Get the maximum response size in bytes.
    pub fn max_response_bytes(&self) -> usize {
        self.max_response_bytes
    }

    /// Make an HTTP request with custom headers and per-request timeout.
    ///
    /// This creates a temporary client with the specified timeout for this request only.
    /// If timeout_secs is None, uses the default client timeout.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP method
    /// * `url` - Request URL
    /// * `body` - Optional request body
    /// * `headers` - Custom headers
    /// * `timeout_secs` - Overall request timeout in seconds (curl --max-time)
    ///
    /// # Security
    ///
    /// - URL is validated against the allowlist before making the request
    /// - Response body is limited to `max_response_bytes` to prevent memory exhaustion
    /// - Redirects are not automatically followed (to prevent allowlist bypass)
    pub async fn request_with_timeout(
        &self,
        method: Method,
        url: &str,
        body: Option<&[u8]>,
        headers: &[(String, String)],
        timeout_secs: Option<u64>,
    ) -> Result<Response> {
        self.request_with_timeouts(method, url, body, headers, timeout_secs, None)
            .await
    }

    /// Make an HTTP request with custom headers and separate connect/request timeouts.
    ///
    /// This creates a temporary client with the specified timeouts for this request only.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP method
    /// * `url` - Request URL
    /// * `body` - Optional request body
    /// * `headers` - Custom headers
    /// * `timeout_secs` - Overall request timeout in seconds (curl --max-time)
    /// * `connect_timeout_secs` - Connection timeout in seconds (curl --connect-timeout)
    ///
    /// # Security
    ///
    /// - URL is validated against the allowlist before making the request
    /// - Response body is limited to `max_response_bytes` to prevent memory exhaustion
    /// - Redirects are not automatically followed (to prevent allowlist bypass)
    pub async fn request_with_timeouts(
        &self,
        method: Method,
        url: &str,
        body: Option<&[u8]>,
        headers: &[(String, String)],
        timeout_secs: Option<u64>,
        connect_timeout_secs: Option<u64>,
    ) -> Result<Response> {
        // Check allowlist + private IP policy BEFORE making any network request.
        self.enforce_url_security(url).await?;

        // Fire before_http hooks — may modify URL/headers or cancel the request
        let (url, headers) = if !self.before_http.is_empty() {
            let event = crate::hooks::HttpRequestEvent {
                method: method.as_str().to_string(),
                url: url.to_string(),
                headers: headers.to_vec(),
            };
            match self.fire_before_http(event) {
                Some(modified) => (
                    std::borrow::Cow::Owned(modified.url),
                    std::borrow::Cow::Owned(modified.headers),
                ),
                None => {
                    return Err(Error::Network("cancelled by before_http hook".to_string()));
                }
            }
        } else {
            (
                std::borrow::Cow::Borrowed(url),
                std::borrow::Cow::Borrowed(headers),
            )
        };
        let url: &str = &url;
        let headers: &[(String, String)] = &headers;
        // Re-check security after hooks in case URL was rewritten.
        self.enforce_url_security(url).await?;

        // Compute bot-auth signing headers (transparent, non-blocking)
        #[cfg(feature = "bot-auth")]
        let signing_headers = self.bot_auth_headers(method, url);
        #[cfg(not(feature = "bot-auth"))]
        let signing_headers: Vec<(String, String)> = Vec::new();

        // Clamp timeout values to safe range [MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS]
        let clamp_timeout = |secs: u64| secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS);
        let request_timeout = timeout_secs.map_or(Duration::from_secs(DEFAULT_TIMEOUT_SECS), |s| {
            Duration::from_secs(clamp_timeout(s))
        });

        // Delegate to custom handler if set
        if let Some(handler) = &self.handler {
            let method_str = method.as_str();
            let mut all_headers: Vec<(String, String)> = headers.to_vec();
            all_headers.extend(signing_headers);
            #[cfg(not(target_family = "wasm"))]
            let response = tokio::time::timeout(
                request_timeout,
                handler.request(method_str, url, body, &all_headers),
            )
            .await
            .map_err(|_| Error::Network("operation timed out".to_string()))?
            .map_err(Error::Network)?;
            #[cfg(target_family = "wasm")]
            let response = handler
                .request(method_str, url, body, &all_headers)
                .await
                .map_err(Error::Network)?;
            if response.body.len() > self.max_response_bytes {
                return Err(Error::Network(format!(
                    "response too large: {} bytes (max: {} bytes)",
                    response.body.len(),
                    self.max_response_bytes
                )));
            }
            self.fire_after_http(crate::hooks::HttpResponseEvent {
                url: url.to_string(),
                status: response.status,
                headers: response.headers.clone(),
            });
            return Ok(response);
        }

        self.send_request(
            method,
            url,
            body,
            headers,
            signing_headers,
            request_timeout,
            connect_timeout_secs.map(|s| Duration::from_secs(clamp_timeout(s))),
        )
        .await
    }

    // -----------------------------------------------------------------------
    // Platform-specific transport
    // -----------------------------------------------------------------------

    #[cfg(target_family = "wasm")]
    pub(crate) async fn send_request(
        &self,
        method: Method,
        url: &str,
        body: Option<&[u8]>,
        headers: &[(String, String)],
        signing_headers: Vec<(String, String)>,
        timeout: Duration,
        _connect_timeout: Option<Duration>,
    ) -> Result<Response> {
        let response = crate::network::client_wasm::send_request(
            self.max_response_bytes,
            method,
            url,
            body,
            headers,
            signing_headers,
            timeout,
        )
        .await?;

        // Fire after_http hooks
        self.fire_after_http(crate::hooks::HttpResponseEvent {
            url: url.to_string(),
            status: response.status,
            headers: response.headers.clone(),
        });

        Ok(response)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticHandler {
        response: Response,
    }

    #[async_trait::async_trait]
    impl HttpHandler for StaticHandler {
        async fn request(
            &self,
            _method: &str,
            _url: &str,
            _body: Option<&[u8]>,
            _headers: &[(String, String)],
        ) -> std::result::Result<Response, String> {
            Ok(Response {
                status: self.response.status,
                headers: self.response.headers.clone(),
                body: self.response.body.clone(),
            })
        }
    }

    #[tokio::test]
    async fn test_blocked_by_empty_allowlist() {
        let client = HttpClient::new(NetworkAllowlist::new());
        #[cfg(not(target_family = "wasm"))]
        assert!(client.client.get().is_none());

        let result = client.get("https://example.com").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("access denied"));
        #[cfg(not(target_family = "wasm"))]
        assert!(client.client.get().is_none());
    }

    #[tokio::test]
    async fn test_blocked_by_allowlist() {
        let allowlist = NetworkAllowlist::new().allow("https://allowed.com");
        let client = HttpClient::new(allowlist);

        let result = client.get("https://blocked.com").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("access denied"));
    }

    #[tokio::test]
    async fn test_request_with_timeout_blocked_by_allowlist() {
        let client = HttpClient::new(NetworkAllowlist::new());

        let result = client
            .request_with_timeout(Method::Get, "https://example.com", None, &[], Some(5))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("access denied"));
    }

    #[tokio::test]
    async fn test_request_with_timeout_none_uses_default() {
        let allowlist = NetworkAllowlist::new().allow("https://blocked.com");
        let client = HttpClient::new(allowlist);

        let result = client
            .request_with_timeout(Method::Get, "https://blocked.example.com", None, &[], None)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("access denied"));
    }

    #[tokio::test]
    async fn test_request_with_timeout_validates_url() {
        let allowlist = NetworkAllowlist::new().allow("https://allowed.com");
        let client = HttpClient::new(allowlist);

        let result = client
            .request_with_timeout(Method::Get, "not-a-url", None, &[], Some(10))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_request_with_timeouts_both_params() {
        let client = HttpClient::new(NetworkAllowlist::new());

        let result = client
            .request_with_timeouts(
                Method::Get,
                "https://example.com",
                None,
                &[],
                Some(30),
                Some(10),
            )
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("access denied"));
    }

    #[tokio::test]
    async fn test_request_with_timeouts_connect_only() {
        let client = HttpClient::new(NetworkAllowlist::new());

        let result = client
            .request_with_timeouts(Method::Get, "https://example.com", None, &[], None, Some(5))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("access denied"));
    }

    #[test]
    fn test_u64_to_usize_no_truncation() {
        let large: u64 = 5_368_709_120;
        let result = usize::try_from(large).unwrap_or(usize::MAX);
        assert!(result >= large.min(usize::MAX as u64) as usize);
    }

    #[tokio::test]
    async fn test_check_private_ip_fails_closed_on_invalid_url() {
        let client = HttpClient::new(NetworkAllowlist::allow_all());
        let result = client.check_private_ip("definitely::not::a::url").await;
        assert!(result.is_err(), "malformed URL must trip the SSRF precheck");
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("invalid URL") || msg.contains("SSRF"),
            "expected SSRF-precheck error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_check_private_ip_fails_closed_on_no_host() {
        let client = HttpClient::new(NetworkAllowlist::allow_all());
        let result = client.check_private_ip("file:///etc/passwd").await;
        assert!(result.is_err(), "host-less URL must trip the precheck");
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("no host") || msg.contains("SSRF"),
            "expected SSRF-precheck error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_check_private_ip_blocks_literal_private_ip() {
        let client = HttpClient::new(NetworkAllowlist::allow_all());
        let result = client.check_private_ip("http://10.0.0.1/").await;
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("private IP") || msg.contains("SSRF"),
            "expected SSRF-protection error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_check_private_ip_blocks_metadata_via_v4_mapped_v6() {
        let client = HttpClient::new(NetworkAllowlist::allow_all());
        let result = client
            .check_private_ip("http://[::ffff:169.254.169.254]/")
            .await;
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("private IP") || msg.contains("SSRF"),
            "expected SSRF-protection error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_custom_handler_enforces_max_response_bytes() {
        let mut client =
            HttpClient::with_config(NetworkAllowlist::allow_all(), Duration::from_secs(30), 4);
        client.set_handler(Box::new(StaticHandler {
            response: Response {
                status: 200,
                headers: vec![],
                body: b"too-large".to_vec(),
            },
        }));

        let result = client.get("https://example.com").await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("response too large")
        );
    }

    #[tokio::test]
    async fn test_before_http_hook_cannot_bypass_allowlist_request_with_headers() {
        let allowlist = NetworkAllowlist::new().allow("https://allowed.com");
        let mut client = HttpClient::new(allowlist);
        client.set_handler(Box::new(StaticHandler {
            response: Response {
                status: 200,
                headers: vec![],
                body: b"ok".to_vec(),
            },
        }));
        client.set_before_http(vec![Box::new(|mut event| {
            event.url = "https://blocked.com".to_string();
            crate::hooks::HookAction::Continue(event)
        })]);

        let result = client
            .request_with_headers(Method::Get, "https://allowed.com", None, &[])
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("access denied"));
    }

    #[tokio::test]
    async fn test_before_http_hook_cannot_bypass_allowlist_request_with_timeouts() {
        let allowlist = NetworkAllowlist::new().allow("https://allowed.com");
        let mut client = HttpClient::new(allowlist);
        client.set_handler(Box::new(StaticHandler {
            response: Response {
                status: 200,
                headers: vec![],
                body: b"ok".to_vec(),
            },
        }));
        client.set_before_http(vec![Box::new(|mut event| {
            event.url = "https://blocked.com".to_string();
            crate::hooks::HookAction::Continue(event)
        })]);

        let result = client
            .request_with_timeouts(Method::Get, "https://allowed.com", None, &[], Some(5), None)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("access denied"));
    }

    // Note: Integration tests that actually make network requests
    // should be in a separate test file and marked with #[ignore]
    // to avoid network dependencies in unit tests.
}
