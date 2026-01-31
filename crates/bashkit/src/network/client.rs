//! HTTP client for secure network access
//!
//! Provides a sandboxed HTTP client that respects the allowlist.

use reqwest::Client;
use std::time::Duration;

use super::allowlist::{NetworkAllowlist, UrlMatch};
use crate::error::{Error, Result};

/// HTTP client with allowlist-based access control.
pub struct HttpClient {
    client: Client,
    allowlist: NetworkAllowlist,
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
    pub fn new(allowlist: NetworkAllowlist) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("bashkit/0.1.0")
            .build()
            .expect("failed to build HTTP client");

        Self { client, allowlist }
    }

    /// Create a client with custom timeout.
    pub fn with_timeout(allowlist: NetworkAllowlist, timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .user_agent("bashkit/0.1.0")
            .build()
            .expect("failed to build HTTP client");

        Self { client, allowlist }
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
        // Check allowlist
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
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let body = response
            .bytes()
            .await
            .map_err(|e| Error::Network(format!("failed to read response: {}", e)))?
            .to_vec();

        Ok(Response {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_blocked_by_empty_allowlist() {
        let client = HttpClient::new(NetworkAllowlist::new());

        let result = client.get("https://example.com").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("access denied"));
    }

    #[tokio::test]
    async fn test_blocked_by_allowlist() {
        let allowlist = NetworkAllowlist::new().allow("https://allowed.com");
        let client = HttpClient::new(allowlist);

        let result = client.get("https://blocked.com").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("access denied"));
    }

    // Note: Integration tests that actually make network requests
    // should be in a separate test file and marked with #[ignore]
    // to avoid network dependencies in unit tests.
}
