//! HTTP client for secure network access.
//!
//! Provides a sandboxed HTTP client that respects the allowlist with
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

use reqwest::Client;
use std::time::Duration;

use super::allowlist::{NetworkAllowlist, UrlMatch};
use crate::error::{Error, Result};

/// Default maximum response body size (10 MB)
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

/// Default request timeout (30 seconds)
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Maximum allowed timeout (10 minutes) - prevents resource exhaustion from very long timeouts
pub const MAX_TIMEOUT_SECS: u64 = 600;

/// Minimum allowed timeout (1 second) - prevents instant timeouts that waste resources
pub const MIN_TIMEOUT_SECS: u64 = 1;

/// HTTP client with allowlist-based access control.
///
/// # Security Features
///
/// - URL allowlist enforcement
/// - Response size limits to prevent memory exhaustion
/// - Configurable timeouts to prevent hanging
/// - No automatic redirect following (to prevent allowlist bypass)
pub struct HttpClient {
    client: Client,
    allowlist: NetworkAllowlist,
    /// Maximum response body size in bytes
    max_response_bytes: usize,
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
        let client = Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(10)) // Separate connect timeout
            .user_agent("bashkit/0.1.0")
            // Disable automatic redirects to prevent allowlist bypass via redirect
            // Scripts can follow redirects manually if needed
            .redirect(reqwest::redirect::Policy::none())
            // Disable automatic decompression to prevent zip bomb attacks
            // and match real curl behavior (which requires --compressed flag)
            // With decompression enabled, a 10KB gzip could expand to 10GB
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .build()
            .expect("failed to build HTTP client");

        Self {
            client,
            allowlist,
            max_response_bytes,
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

        // Build request
        let mut request = self.client.request(method.as_reqwest(), url);

        // Add custom headers
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }

        if let Some(body_data) = body {
            request = request.body(body_data.to_vec());
        }

        // Send request
        let response = request
            .send()
            .await
            .map_err(|e| Error::Network(format!("request failed: {}", e)))?;

        // Extract response data
        let status = response.status().as_u16();
        let resp_headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        // Check Content-Length header to fail fast on large responses
        if let Some(content_length) = response.content_length() {
            if content_length as usize > self.max_response_bytes {
                return Err(Error::Network(format!(
                    "response too large: {} bytes (max: {} bytes)",
                    content_length, self.max_response_bytes
                )));
            }
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
        use futures::StreamExt;

        let mut body = Vec::new();
        let mut stream = response.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result
                .map_err(|e| Error::Network(format!("failed to read response chunk: {}", e)))?;

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
            Client::builder()
                .timeout(timeout)
                .connect_timeout(connect_timeout)
                .user_agent("bashkit/0.1.0")
                .redirect(reqwest::redirect::Policy::none())
                .no_gzip()
                .no_brotli()
                .no_deflate()
                .build()
                .map_err(|e| Error::Network(format!("failed to create client: {}", e)))?
        } else {
            self.client.clone()
        };

        // Build request
        let mut request = client.request(method.as_reqwest(), url);

        // Add custom headers
        for (name, value) in headers {
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
                Error::Network(format!("request failed: {}", e))
            }
        })?;

        // Extract response data
        let status = response.status().as_u16();
        let resp_headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        // Check Content-Length header to fail fast on large responses
        if let Some(content_length) = response.content_length() {
            if content_length as usize > self.max_response_bytes {
                return Err(Error::Network(format!(
                    "response too large: {} bytes (max: {} bytes)",
                    content_length, self.max_response_bytes
                )));
            }
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_blocked_by_empty_allowlist() {
        let client = HttpClient::new(NetworkAllowlist::new());

        let result = client.get("https://example.com").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("access denied"));
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

    // Note: Integration tests that actually make network requests
    // should be in a separate test file and marked with #[ignore]
    // to avoid network dependencies in unit tests.
}
