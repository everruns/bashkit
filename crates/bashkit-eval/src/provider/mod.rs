// Provider abstraction for LLM APIs
// Normalizes Anthropic Messages API, OpenAI Chat Completions API,
// and OpenAI Responses API into common Message/ContentBlock types for the agent loop

pub mod anthropic;
pub mod openai;
pub mod openai_responses;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use std::sync::OnceLock;

pub use anthropic::AnthropicProvider;
pub use openai::OpenAiProvider;
pub use openai_responses::OpenAiResponsesProvider;

pub fn ensure_rustls_crypto_provider() -> anyhow::Result<()> {
    static INSTALL_RESULT: OnceLock<anyhow::Result<()>> = OnceLock::new();

    let install_result =
        INSTALL_RESULT.get_or_init(|| {
            match rustls::crypto::ring::default_provider().install_default() {
                Ok(()) => Ok(()),
                Err(_) if rustls::crypto::CryptoProvider::get_default().is_some() => Ok(()),
                Err(_) => Err(anyhow::anyhow!(
                    "failed to install rustls ring crypto provider"
                )),
            }
        });

    install_result
        .as_ref()
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("rustls crypto provider unavailable: {e}"))
}

#[cfg(test)]
mod tests {
    use super::ensure_rustls_crypto_provider;

    #[test]
    fn ensure_rustls_provider_succeeds_when_already_installed() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let result = ensure_rustls_crypto_provider();
        assert!(result.is_ok());
    }

    use super::is_retryable_error;
    use reqwest::StatusCode;

    fn err_body(type_: &str, code: &str) -> serde_json::Value {
        serde_json::json!({"error": {"type": type_, "code": code, "message": "x"}})
    }

    #[test]
    fn quota_429_is_not_retryable() {
        // The insufficient_quota trap: a permanent 429 must fast-fail, not loop.
        let by_code = err_body("", "insufficient_quota");
        let by_type = err_body("insufficient_quota", "");
        assert!(!is_retryable_error(StatusCode::TOO_MANY_REQUESTS, &by_code));
        assert!(!is_retryable_error(StatusCode::TOO_MANY_REQUESTS, &by_type));
        assert!(!is_retryable_error(
            StatusCode::TOO_MANY_REQUESTS,
            &err_body("", "billing_hard_limit_reached")
        ));
    }

    #[test]
    fn rate_limit_429_is_retryable() {
        let rate = err_body("rate_limit_error", "rate_limit_exceeded");
        assert!(is_retryable_error(StatusCode::TOO_MANY_REQUESTS, &rate));
        // A bare 429 with no error body is treated as a transient rate limit.
        assert!(is_retryable_error(
            StatusCode::TOO_MANY_REQUESTS,
            &serde_json::json!({})
        ));
    }

    #[test]
    fn auth_errors_are_not_retryable() {
        let b = serde_json::json!({});
        assert!(!is_retryable_error(StatusCode::UNAUTHORIZED, &b));
        assert!(!is_retryable_error(StatusCode::FORBIDDEN, &b));
    }

    #[test]
    fn server_errors_are_retryable() {
        let b = serde_json::json!({});
        assert!(is_retryable_error(StatusCode::INTERNAL_SERVER_ERROR, &b));
        assert!(is_retryable_error(StatusCode::SERVICE_UNAVAILABLE, &b));
        // Anthropic's 529 "overloaded".
        assert!(is_retryable_error(StatusCode::from_u16(529).unwrap(), &b));
    }

    #[test]
    fn client_errors_are_not_retryable() {
        // e.g. 400/404 (bad model id) — surface immediately as an infra error.
        let b = serde_json::json!({});
        assert!(!is_retryable_error(StatusCode::BAD_REQUEST, &b));
        assert!(!is_retryable_error(StatusCode::NOT_FOUND, &b));
    }
}

/// Connect timeout for provider HTTP requests. A blocked/blackholed socket
/// fails here instead of hanging forever (the original `Client::new()` had no
/// timeouts).
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Total per-request timeout. Generous enough for slow high-effort reasoning
/// (codex via the Responses API) while still bounding a genuine hang.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Build the shared HTTP client used by every provider, with connect + total
/// timeouts so a single request can never stall a run indefinitely.
pub fn build_http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {e}"))
}

/// Decide whether a failed HTTP response is worth retrying.
///
/// Protects against the `insufficient_quota` trap: OpenAI returns HTTP 429 for
/// *both* transient rate limits (retry) and a permanently exhausted quota /
/// missing billing (do NOT retry — every attempt 429s, turning the run into a
/// long backoff hang). We classify a 429 as non-retryable when the body marks
/// it as a quota/billing error. Auth failures (401/403) are never retryable.
/// Transient: other 429s, 5xx, and Anthropic's 529 "overloaded".
pub fn is_retryable_error(status: reqwest::StatusCode, body: &serde_json::Value) -> bool {
    let code = status.as_u16();
    if code == 401 || code == 403 {
        return false;
    }
    if code == 429 {
        let err_type = body["error"]["type"].as_str().unwrap_or("");
        let err_code = body["error"]["code"].as_str().unwrap_or("");
        let permanent = ["insufficient_quota", "billing_hard_limit_reached"];
        if permanent.contains(&err_type) || permanent.contains(&err_code) {
            return false;
        }
        return true;
    }
    status.is_server_error() || code == 529
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    ToolResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub message: Message,
    pub stop: bool,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
    ) -> anyhow::Result<ProviderResponse>;

    fn name(&self) -> &str;
    fn model(&self) -> &str;
}

/// Create a provider from name + model
pub fn create_provider(provider_name: &str, model: &str) -> anyhow::Result<Box<dyn Provider>> {
    match provider_name {
        "anthropic" => Ok(Box::new(AnthropicProvider::new(model)?)),
        "openai" => Ok(Box::new(OpenAiProvider::new(model)?)),
        "openresponses" => Ok(Box::new(OpenAiResponsesProvider::new(model)?)),
        _ => anyhow::bail!(
            "unknown provider: '{}'. Use 'anthropic', 'openai', or 'openresponses'",
            provider_name
        ),
    }
}
