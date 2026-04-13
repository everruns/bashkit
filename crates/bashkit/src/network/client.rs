//! HTTP client for secure network access.
//!
//! Provides a virtual HTTP client that respects the allowlist with
//! security mitigations for common HTTP attacks.
//!
//! # Security Mitigations
//!
//! This module mitigates the following threats (see `specs/006-threat-model.md`):
//!
//! - **TM-NET-008**: Large response DoS → `max_response_bytes` limit (10MB default)
//! - **TM-NET-009**: Connection hang → connect timeout (10s)
//! - **TM-NET-010**: Slowloris attack → read timeout (30s)
//! - **TM-NET-011**: Redirect bypass → `Policy::none()` disables auto-redirect
//! - **TM-NET-012**: Chunked encoding bomb → streaming size check
//! - **TM-NET-013**: Gzip/compression bomb → auto-decompression disabled
//! - **TM-NET-014**: DNS rebind via redirect → manual redirect requires allowlist check
//! - **TM-NET-015**: Host proxy leakage → `.no_proxy()` ignores host `HTTP_PROXY`/`HTTPS_PROXY`

use reqwest::Client;
use std::sync::OnceLock;
use std::time::Duration;

use super::allowlist::{NetworkAllowlist, UrlMatch, is_private_ip};
use crate::error::{Error, Result};

/// Default maximum response body size (10 MB)
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

/// Default request timeout (30 seconds)
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Maximum allowed timeout (10 minutes) - prevents resource exhaustion from very long timeouts
pub const MAX_TIMEOUT_SECS: u64 = 600;

/// Minimum allowed timeout (1 second) - prevents instant timeouts that waste resources
pub const MIN_TIMEOUT_SECS: u64 = 1;

/// Trait for custom HTTP request handling.
///
/// Embedders can implement this trait to intercept, proxy, log, cache,
/// or mock HTTP requests made by scripts running in the sandbox.
///
/// The allowlist check happens _before_ the handler is called, so the
/// security boundary stays in bashkit.
///
/// # Default
///
/// When no custom handler is set, `HttpClient` uses `reqwest` directly.
#[async_trait::async_trait]
pub trait HttpHandler: Send + Sync {
    /// Handle an HTTP request and return a response.
    ///
    /// Called after the URL has been validated against the allowlist.
    async fn request(
        &self,
        method: &str,
        url: &str,
        body: Option<&[u8]>,
        headers: &[(String, String)],
    ) -> std::result::Result<Response, String>;
}

/// HTTP client with allowlist-based access control.
///
/// # Security Features
///
/// - URL allowlist enforcement
/// - Response size limits to prevent memory exhaustion
/// - Configurable timeouts to prevent hanging
/// - No automatic redirect following (to prevent allowlist bypass)
pub struct HttpClient {
    client: OnceLock<std::result::Result<Client, String>>,
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
    fn as_reqwest(self) -> reqwest::Method {
        match self {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Put => reqwest::Method::PUT,
            Method::Delete => reqwest::Method::DELETE,
            Method::Head => reqwest::Method::HEAD,
            Method::Patch => reqwest::Method::PATCH,
        }
    }

    fn as_str(self) -> &'static str {
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
            client: OnceLock::new(),
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
    /// boundary stays in bashkit. The default reqwest-based handler is used
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

    /// Produce bot-auth signing headers for the given URL.
    /// Non-blocking: signing failures return an empty vec (request sent unsigned).
    #[cfg(feature = "bot-auth")]
    fn bot_auth_headers(&self, url: &str) -> Vec<(String, String)> {
        let Some(ref bot_auth) = self.bot_auth else {
            return Vec::new();
        };
        let Ok(parsed) = url::Url::parse(url) else {
            return Vec::new();
        };
        let Some(authority) = parsed.host_str() else {
            return Vec::new();
        };
        match bot_auth.sign_request(authority) {
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

    fn client(&self) -> Result<&Client> {
        let client = self
            .client
            .get_or_init(|| build_client(self.default_timeout, None));
        client
            .as_ref()
            .map_err(|err| Error::Internal(format!("failed to build HTTP client: {err}")))
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

    /// THREAT[TM-NET-002/004]: Pre-resolve DNS and block private IPs.
    async fn check_private_ip(&self, url: &str) -> Result<()> {
        let parsed = match url::Url::parse(url) {
            Ok(p) => p,
            Err(_) => return Ok(()),
        };
        let Some(host) = parsed.host_str() else {
            return Ok(());
        };
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            if is_private_ip(&ip) {
                return Err(Error::Network(format!(
                    "access denied: {} is a private IP (SSRF protection)",
                    host
                )));
            }
        } else {
            let port = parsed
                .port()
                .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });
            let addr = format!("{}:{}", host, port);
            if let Ok(addrs) = tokio::net::lookup_host(&addr).await {
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
        // Check allowlist BEFORE making any network request
        match self.allowlist.check(url) {
            UrlMatch::Allowed => {}
            UrlMatch::Blocked { reason } => {
                return Err(Error::Network(format!("access denied: {}", reason)));
            }
            UrlMatch::Invalid { reason } => {
                return Err(Error::Network(format!("invalid URL: {}", reason)));
            }
        }

        // THREAT[TM-NET-002/004]: Pre-resolve DNS and block private IPs
        // to prevent SSRF via DNS rebinding.
        if self.allowlist.is_blocking_private_ips() {
            self.check_private_ip(url).await?;
        }

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

        // Compute bot-auth signing headers (transparent, non-blocking)
        #[cfg(feature = "bot-auth")]
        let signing_headers = self.bot_auth_headers(url);
        #[cfg(not(feature = "bot-auth"))]
        let signing_headers: Vec<(String, String)> = Vec::new();

        // Delegate to custom handler if set
        if let Some(handler) = &self.handler {
            let method_str = method.as_str();
            let result = if signing_headers.is_empty() {
                handler
                    .request(method_str, url, body, headers)
                    .await
                    .map_err(Error::Network)
            } else {
                let mut all_headers: Vec<(String, String)> = headers.to_vec();
                all_headers.extend(signing_headers);
                handler
                    .request(method_str, url, body, &all_headers)
                    .await
                    .map_err(Error::Network)
            };
            if let Ok(ref resp) = result {
                self.fire_after_http(crate::hooks::HttpResponseEvent {
                    url: url.to_string(),
                    status: resp.status,
                    headers: resp.headers.clone(),
                });
            }
            return result;
        }

        // Build request
        let mut request = self.client()?.request(method.as_reqwest(), url);

        // Add custom headers
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }

        // Add bot-auth signing headers
        for (name, value) in &signing_headers {
            request = request.header(name.as_str(), value.as_str());
        }

        if let Some(body_data) = body {
            request = request.body(body_data.to_vec());
        }

        // Send request
        let response = request
            .send()
            .await
            .map_err(|e| Error::network_sanitized("request failed", &e))?;

        // Extract response data
        let status = response.status().as_u16();
        let resp_headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        // Fire after_http hooks
        self.fire_after_http(crate::hooks::HttpResponseEvent {
            url: url.to_string(),
            status,
            headers: resp_headers.clone(),
        });

        // Check Content-Length header to fail fast on large responses
        if let Some(content_length) = response.content_length()
            && usize::try_from(content_length).unwrap_or(usize::MAX) > self.max_response_bytes
        {
            return Err(Error::Network(format!(
                "response too large: {} bytes (max: {} bytes)",
                content_length, self.max_response_bytes
            )));
        }

        // Read body with size limit enforcement
        // We stream the response to avoid loading huge responses into memory
        let body = self.read_body_with_limit(response).await?;

        Ok(Response {
            status,
            headers: resp_headers,
            body,
        })
    }

    /// Read response body with size limit enforcement.
    ///
    /// This streams the response to avoid allocating memory for oversized responses.
    async fn read_body_with_limit(&self, response: reqwest::Response) -> Result<Vec<u8>> {
        use futures_util::StreamExt;

        let mut body = Vec::new();
        let mut stream = response.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result
                .map_err(|e| Error::network_sanitized("failed to read response chunk", &e))?;

            // Check if adding this chunk would exceed the limit
            if body.len() + chunk.len() > self.max_response_bytes {
                return Err(Error::Network(format!(
                    "response too large: exceeded {} bytes limit",
                    self.max_response_bytes
                )));
            }

            body.extend_from_slice(&chunk);
        }

        Ok(body)
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
        // Check allowlist BEFORE making any network request
        match self.allowlist.check(url) {
            UrlMatch::Allowed => {}
            UrlMatch::Blocked { reason } => {
                return Err(Error::Network(format!("access denied: {}", reason)));
            }
            UrlMatch::Invalid { reason } => {
                return Err(Error::Network(format!("invalid URL: {}", reason)));
            }
        }

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

        // Compute bot-auth signing headers (transparent, non-blocking)
        #[cfg(feature = "bot-auth")]
        let signing_headers = self.bot_auth_headers(url);
        #[cfg(not(feature = "bot-auth"))]
        let signing_headers: Vec<(String, String)> = Vec::new();

        // Delegate to custom handler if set (timeouts are the handler's responsibility)
        if let Some(handler) = &self.handler {
            let method_str = method.as_str();
            let result = if signing_headers.is_empty() {
                handler
                    .request(method_str, url, body, headers)
                    .await
                    .map_err(Error::Network)
            } else {
                let mut all_headers: Vec<(String, String)> = headers.to_vec();
                all_headers.extend(signing_headers);
                handler
                    .request(method_str, url, body, &all_headers)
                    .await
                    .map_err(Error::Network)
            };
            if let Ok(ref resp) = result {
                self.fire_after_http(crate::hooks::HttpResponseEvent {
                    url: url.to_string(),
                    status: resp.status,
                    headers: resp.headers.clone(),
                });
            }
            return result;
        }

        // Use the custom timeout client if any timeout is specified, otherwise use default client
        let client = if timeout_secs.is_some() || connect_timeout_secs.is_some() {
            // Clamp timeout values to safe range [MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS]
            let clamp_timeout = |secs: u64| secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS);

            let timeout = timeout_secs.map_or(Duration::from_secs(DEFAULT_TIMEOUT_SECS), |s| {
                Duration::from_secs(clamp_timeout(s))
            });
            // Connect timeout: use explicit connect_timeout, or derive from overall timeout, or use default 10s
            let connect_timeout = connect_timeout_secs.map_or_else(
                || std::cmp::min(timeout, Duration::from_secs(10)),
                |s| Duration::from_secs(clamp_timeout(s)),
            );
            build_client(timeout, Some(connect_timeout))
                .map_err(|e| Error::network_sanitized("failed to create client", &e))?
        } else {
            self.client()?.clone()
        };

        // Build request
        let mut request = client.request(method.as_reqwest(), url);

        // Add custom headers
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }

        // Add bot-auth signing headers
        for (name, value) in &signing_headers {
            request = request.header(name.as_str(), value.as_str());
        }

        if let Some(body_data) = body {
            request = request.body(body_data.to_vec());
        }

        // Send request
        let response = request.send().await.map_err(|e| {
            // Check if this was a timeout error
            if e.is_timeout() {
                Error::Network("operation timed out".to_string())
            } else {
                Error::network_sanitized("request failed", &e)
            }
        })?;

        // Extract response data
        let status = response.status().as_u16();
        let resp_headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        // Fire after_http hooks
        self.fire_after_http(crate::hooks::HttpResponseEvent {
            url: url.to_string(),
            status,
            headers: resp_headers.clone(),
        });

        // Check Content-Length header to fail fast on large responses
        if let Some(content_length) = response.content_length()
            && usize::try_from(content_length).unwrap_or(usize::MAX) > self.max_response_bytes
        {
            return Err(Error::Network(format!(
                "response too large: {} bytes (max: {} bytes)",
                content_length, self.max_response_bytes
            )));
        }

        // Read body with size limit enforcement
        let body = self.read_body_with_limit(response).await?;

        Ok(Response {
            status,
            headers: resp_headers,
            body,
        })
    }
}

fn build_client(
    timeout: Duration,
    connect_timeout: Option<Duration>,
) -> std::result::Result<Client, String> {
    Client::builder()
        .timeout(timeout)
        .connect_timeout(connect_timeout.unwrap_or(Duration::from_secs(10)))
        .user_agent("bashkit/0.1.2")
        // Disable automatic redirects to prevent allowlist bypass via redirect
        // Scripts can follow redirects manually if needed
        .redirect(reqwest::redirect::Policy::none())
        // Disable automatic decompression to prevent zip bomb attacks
        // and match real curl behavior (which requires --compressed flag)
        // With decompression enabled, a 10KB gzip could expand to 10GB
        .no_gzip()
        .no_brotli()
        .no_deflate()
        // THREAT[TM-NET-015]: Ignore host proxy env vars (HTTP_PROXY, HTTPS_PROXY, ALL_PROXY)
        // to prevent sandboxed HTTP traffic from being redirected through a host proxy
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_blocked_by_empty_allowlist() {
        let client = HttpClient::new(NetworkAllowlist::new());
        assert!(client.client.get().is_none());

        let result = client.get("https://example.com").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("access denied"));
        assert!(client.client.get().is_none());
    }

    #[test]
    fn test_default_client_initializes_on_first_use() {
        let client = HttpClient::new(NetworkAllowlist::allow_all());
        assert!(client.client.get().is_none());

        client.client().expect("client");

        assert!(client.client.get().is_some());
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

        // Should use default client (not blocked by allowlist here, but blocked.com not actually accessible)
        // This just verifies the code path with None timeout works
        let result = client
            .request_with_timeout(Method::Get, "https://blocked.example.com", None, &[], None)
            .await;
        // Should fail with access denied (not in allowlist)
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("access denied"));
    }

    #[tokio::test]
    async fn test_request_with_timeout_validates_url() {
        let allowlist = NetworkAllowlist::new().allow("https://allowed.com");
        let client = HttpClient::new(allowlist);

        // Test with invalid URL
        let result = client
            .request_with_timeout(Method::Get, "not-a-url", None, &[], Some(10))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_request_with_timeouts_both_params() {
        let client = HttpClient::new(NetworkAllowlist::new());

        // Both timeouts specified - should still check allowlist first
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

        // Only connect timeout specified
        let result = client
            .request_with_timeouts(Method::Get, "https://example.com", None, &[], None, Some(5))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("access denied"));
    }

    #[test]
    fn test_u64_to_usize_no_truncation() {
        // On 64-bit: fits fine. On 32-bit: saturates to usize::MAX rather than truncating.
        let large: u64 = 5_368_709_120; // 5GB
        let result = usize::try_from(large).unwrap_or(usize::MAX);
        // Should never silently become a smaller value
        assert!(result >= large.min(usize::MAX as u64) as usize);
    }

    #[test]
    fn test_build_client_uses_no_proxy() {
        // Verify build_client succeeds — the .no_proxy() call ensures
        // host HTTP_PROXY/HTTPS_PROXY env vars are ignored (TM-NET-015).
        let client = build_client(Duration::from_secs(30), None);
        assert!(client.is_ok(), "build_client should succeed with no_proxy");
    }

    // Note: Integration tests that actually make network requests
    // should be in a separate test file and marked with #[ignore]
    // to avoid network dependencies in unit tests.
}
