//! Native HTTP transport using `reqwest`.
//!
//! This submodule is compiled only on non-WASM targets and provides the
//! native `send_request` implementation backed by `reqwest` with a
//! private-IP filtering DNS resolver.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use reqwest::dns::{Name, Resolve, Resolving};

use super::{HttpClient, Method, Response};
use crate::error::{Error, Result};
use crate::network::allowlist::is_private_ip;

// ---------------------------------------------------------------------------
// DNS resolver that rejects private IPs at connect time.
// ---------------------------------------------------------------------------

/// THREAT[TM-NET-002 TOCTOU]: DNS resolver wrapper that rejects any
/// hostname whose addresses include a private/reserved IP at connect time.
struct PrivateIpFilteringResolver;

impl Resolve for PrivateIpFilteringResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let host = name.as_str().to_string();
            let lookup_target = format!("{}:0", host);
            let resolved = tokio::net::lookup_host(lookup_target.as_str())
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

            let addrs: Vec<SocketAddr> = resolved.collect();
            let mut filtered: Vec<SocketAddr> = Vec::with_capacity(addrs.len());
            for addr in addrs {
                if !is_private_ip(&addr.ip()) {
                    filtered.push(addr);
                }
            }

            if filtered.is_empty() {
                let msg = format!(
                    "access denied: '{}' resolves only to private/reserved IPs (SSRF protection)",
                    host
                );
                return Err(msg.into());
            }

            let iter: Box<dyn Iterator<Item = SocketAddr> + Send> = Box::new(filtered.into_iter());
            Ok(iter)
        })
    }
}

// ---------------------------------------------------------------------------
// Method extension
// ---------------------------------------------------------------------------

impl Method {
    pub(crate) fn as_reqwest(self) -> reqwest::Method {
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

// ---------------------------------------------------------------------------
// HttpClient native transport
// ---------------------------------------------------------------------------

impl HttpClient {
    fn client(&self) -> Result<&Client> {
        let block_private = self.allowlist.is_blocking_private_ips();
        let client = self
            .client
            .get_or_init(|| build_client(self.default_timeout, None, block_private));
        client
            .as_ref()
            .map_err(|err| Error::Internal(format!("failed to build HTTP client: {err}")))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn send_request(
        &self,
        method: Method,
        url: &str,
        body: Option<&[u8]>,
        headers: &[(String, String)],
        signing_headers: Vec<(String, String)>,
        timeout: Duration,
        connect_timeout: Option<Duration>,
    ) -> Result<Response> {
        // Use the custom timeout client if any timeout is specified, otherwise use default client
        let client = if timeout != self.default_timeout || connect_timeout.is_some() {
            let connect_timeout =
                connect_timeout.unwrap_or_else(|| std::cmp::min(timeout, Duration::from_secs(10)));
            build_client(
                timeout,
                Some(connect_timeout),
                self.allowlist.is_blocking_private_ips(),
            )
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
}

// ---------------------------------------------------------------------------
// Client builder and crypto provider
// ---------------------------------------------------------------------------

/// Install the rustls `ring` crypto provider as the process-wide default.
fn install_default_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn build_client(
    timeout: Duration,
    connect_timeout: Option<Duration>,
    block_private_ips: bool,
) -> std::result::Result<Client, String> {
    install_default_crypto_provider();
    let mut builder = Client::builder()
        .timeout(timeout)
        .connect_timeout(connect_timeout.unwrap_or(Duration::from_secs(10)))
        .user_agent("bashkit/0.1.2")
        // Disable automatic redirects to prevent allowlist bypass via redirect
        .redirect(reqwest::redirect::Policy::none())
        // Disable automatic decompression to prevent zip bomb attacks
        .no_gzip()
        .no_brotli()
        .no_deflate()
        // THREAT[TM-NET-015]: Ignore host proxy env vars
        .no_proxy();

    // THREAT[TM-NET-002 TOCTOU): install a DNS resolver that filters private IPs
    if block_private_ips {
        builder = builder.dns_resolver(Arc::new(PrivateIpFilteringResolver));
    }

    builder.build().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;

    use super::super::{
        DEFAULT_MAX_RESPONSE_BYTES, HttpHandler, NetworkAllowlist,
    };

    struct SlowHandler {
        delay: StdDuration,
    }

    #[async_trait::async_trait]
    impl HttpHandler for SlowHandler {
        async fn request(
            &self,
            _method: &str,
            _url: &str,
            _body: Option<&[u8]>,
            _headers: &[(String, String)],
        ) -> std::result::Result<Response, String> {
            tokio::time::sleep(self.delay).await;
            Ok(Response {
                status: 200,
                headers: vec![],
                body: b"ok".to_vec(),
            })
        }
    }

    #[test]
    fn test_default_client_initializes_on_first_use() {
        let client = HttpClient::new(NetworkAllowlist::allow_all());
        assert!(client.client.get().is_none());

        client.client().expect("client");

        assert!(client.client.get().is_some());
    }

    #[test]
    fn test_build_client_uses_no_proxy() {
        let client = build_client(Duration::from_secs(30), None, true);
        assert!(client.is_ok(), "build_client should succeed with no_proxy");
    }

    #[test]
    fn test_build_client_installs_ring_crypto_provider() {
        let _ = build_client(Duration::from_secs(30), None, true);
        let second_install = rustls::crypto::ring::default_provider().install_default();
        assert!(
            second_install.is_err(),
            "build_client must install a default crypto provider"
        );
    }

    #[test]
    fn test_install_default_crypto_provider_is_idempotent() {
        install_default_crypto_provider();
        install_default_crypto_provider();
        install_default_crypto_provider();
    }

    #[tokio::test]
    async fn test_private_ip_filtering_resolver_rejects_loopback() {
        let resolver = PrivateIpFilteringResolver;
        let name: Name = "localhost".parse().expect("valid DNS name");
        let result = resolver.resolve(name).await;
        assert!(
            result.is_err(),
            "localhost must be rejected by the private-IP-filtering resolver"
        );
        let err = result.err().unwrap().to_string();
        assert!(
            err.contains("private/reserved"),
            "error must mention SSRF protection, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_private_ip_filtering_resolver_filters_private_from_mixed() {
        use std::net::{IpAddr, Ipv4Addr};
        let public: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)), 0);
        let private: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 0);
        let metadata: SocketAddr =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)), 0);
        let mixed = vec![public, private, metadata];
        let kept: Vec<SocketAddr> = mixed
            .into_iter()
            .filter(|a| !is_private_ip(&a.ip()))
            .collect();
        assert_eq!(kept, vec![public]);
    }

    #[tokio::test]
    async fn test_default_client_rejects_loopback_via_resolver() {
        let allowlist = NetworkAllowlist::new().allow("http://localhost");
        let client = HttpClient::new(allowlist);
        let result = client.get("http://localhost").await;
        assert!(
            result.is_err(),
            "request to a loopback hostname must be refused"
        );
        let msg = result.err().unwrap().to_string();
        assert!(
            msg.contains("private")
                || msg.contains("SSRF")
                || msg.contains("reserved")
                || msg.contains("access denied"),
            "expected SSRF-protection error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_custom_handler_enforces_request_timeout() {
        let mut client = HttpClient::with_config(
            NetworkAllowlist::allow_all(),
            Duration::from_secs(30),
            DEFAULT_MAX_RESPONSE_BYTES,
        );
        client.set_handler(Box::new(SlowHandler {
            delay: StdDuration::from_millis(1200),
        }));

        let result = client
            .request_with_timeouts(Method::Get, "https://example.com", None, &[], Some(1), None)
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("operation timed out")
        );
    }
}
