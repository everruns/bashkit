// OpenAI Chat Completions API provider
// POST https://api.openai.com/v1/chat/completions
// Tool use: tool_calls array + role "tool" messages
//
// Decision: OpenRouter speaks the same Chat Completions wire format, so it is
// a second endpoint + key on this provider, not a separate provider. It lets
// the eval run OpenAI and Gemini models when the direct OpenAI key has no
// credit (and Gemini's own host is unreachable). OpenRouter model ids carry
// the vendor prefix (`openai/gpt-5.6-sol`, `google/gemini-3.8-flash`).

use anyhow::{Context, Result};
use async_trait::async_trait;

use super::{
    ContentBlock, Message, Provider, ProviderResponse, Role, ToolDefinition, build_http_client,
    ensure_rustls_crypto_provider, is_retryable_error,
};

const OPENAI_CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";
const OPENROUTER_CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    endpoint: &'static str,
    /// Display name for errors/retry logs ("OpenAI" / "OpenRouter").
    name: &'static str,
}

impl OpenAiProvider {
    pub fn new(model: &str) -> Result<Self> {
        Self::with_endpoint(model, OPENAI_CHAT_URL, "OPENAI_API_KEY", "OpenAI")
    }

    /// OpenRouter's OpenAI-compatible endpoint; `model` is a vendor-prefixed
    /// OpenRouter id such as `openai/gpt-5.6-sol`.
    pub fn openrouter(model: &str) -> Result<Self> {
        Self::with_endpoint(
            model,
            OPENROUTER_CHAT_URL,
            "OPENROUTER_API_KEY",
            "OpenRouter",
        )
    }

    fn with_endpoint(
        model: &str,
        endpoint: &'static str,
        key_env: &str,
        name: &'static str,
    ) -> Result<Self> {
        ensure_rustls_crypto_provider()?;
        let api_key =
            std::env::var(key_env).with_context(|| format!("{key_env} env var not set"))?;
        Ok(Self {
            client: build_http_client()?,
            api_key,
            model: model.to_string(),
            endpoint,
            name,
        })
    }

    fn build_request_body(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
    ) -> serde_json::Value {
        let mut api_messages: Vec<serde_json::Value> = Vec::new();

        // System message first
        api_messages.push(serde_json::json!({
            "role": "system",
            "content": system
        }));

        for m in messages {
            match m.role {
                Role::User => {
                    let text = m
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    api_messages.push(serde_json::json!({"role": "user", "content": text}));
                }
                Role::Assistant => {
                    let mut tool_calls = Vec::new();
                    let mut text_parts = Vec::new();

                    for b in &m.content {
                        match b {
                            ContentBlock::Text { text } => text_parts.push(text.clone()),
                            ContentBlock::ToolUse { id, name, input } => {
                                tool_calls.push(serde_json::json!({
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": serde_json::to_string(input).unwrap_or_default()
                                    }
                                }));
                            }
                            _ => {}
                        }
                    }

                    let mut msg = serde_json::json!({"role": "assistant"});
                    if !text_parts.is_empty() {
                        msg["content"] = serde_json::Value::String(text_parts.join("\n"));
                    }
                    if !tool_calls.is_empty() {
                        msg["tool_calls"] = serde_json::Value::Array(tool_calls);
                    }
                    api_messages.push(msg);
                }
                Role::ToolResult => {
                    for b in &m.content {
                        if let ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } = b
                        {
                            api_messages.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tool_use_id,
                                "content": content
                            }));
                        }
                    }
                }
            }
        }

        let api_tools: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema
                    }
                })
            })
            .collect();

        serde_json::json!({
            "model": self.model,
            "messages": api_messages,
            "tools": api_tools
        })
    }

    fn parse_response(&self, body: serde_json::Value) -> Result<ProviderResponse> {
        let choice = body["choices"]
            .as_array()
            .and_then(|c| c.first())
            .context("no choices in OpenAI response")?;

        let finish_reason = choice["finish_reason"]
            .as_str()
            .unwrap_or("stop")
            .to_string();

        let input_tokens = body["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32;
        let output_tokens = body["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32;

        let message = &choice["message"];
        let mut blocks = Vec::new();

        // Text content
        if let Some(content) = message["content"].as_str()
            && !content.is_empty()
        {
            blocks.push(ContentBlock::Text {
                text: content.to_string(),
            });
        }

        // Tool calls
        if let Some(tool_calls) = message["tool_calls"].as_array() {
            for tc in tool_calls {
                let id = tc["id"].as_str().unwrap_or("").to_string();
                let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                let input: serde_json::Value =
                    serde_json::from_str(args_str).unwrap_or(serde_json::json!({}));
                blocks.push(ContentBlock::ToolUse { id, name, input });
            }
        }

        Ok(ProviderResponse {
            message: Message {
                role: Role::Assistant,
                content: blocks,
            },
            stop: finish_reason == "stop",
            input_tokens,
            output_tokens,
        })
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
    ) -> Result<ProviderResponse> {
        let body = self.build_request_body(messages, tools, system);
        let delays = [2, 4, 8, 16];

        for attempt in 0..=delays.len() {
            let resp = self
                .client
                .post(self.endpoint)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .with_context(|| format!("failed to send request to {} API", self.name))?;

            let status = resp.status();
            let resp_body: serde_json::Value = resp
                .json()
                .await
                .with_context(|| format!("failed to parse {} API response", self.name))?;

            if status.is_success() {
                return self.parse_response(resp_body);
            }

            let error_msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");

            // Retry transient errors only; fast-fail on quota/billing/auth so an
            // exhausted account doesn't turn into a long backoff hang.
            let retryable = is_retryable_error(status, &resp_body);
            if retryable && let Some(&delay) = delays.get(attempt) {
                eprintln!(
                    "  [retry] {} {} — waiting {}s (attempt {}/{})",
                    self.name,
                    status,
                    delay,
                    attempt + 1,
                    delays.len()
                );
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                continue;
            }

            anyhow::bail!("{} API error ({}): {}", self.name, status, error_msg);
        }

        unreachable!()
    }

    fn name(&self) -> &str {
        "openai"
    }

    fn model(&self) -> &str {
        &self.model
    }
}
