// Anthropic Messages API provider
// POST https://api.anthropic.com/v1/messages
// Tool use: content blocks with type "tool_use" / "tool_result"

use anyhow::{Context, Result};
use async_trait::async_trait;

use super::{ContentBlock, Message, Provider, ProviderResponse, Role, ToolDefinition};

pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl AnthropicProvider {
    pub fn new(model: &str) -> Result<Self> {
        let api_key =
            std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY env var not set")?;
        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            model: model.to_string(),
        })
    }

    fn build_request_body(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
    ) -> serde_json::Value {
        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::ToolResult => "user",
                };
                let content: Vec<serde_json::Value> = m
                    .content
                    .iter()
                    .map(|b| match b {
                        ContentBlock::Text { text } => {
                            serde_json::json!({"type": "text", "text": text})
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            serde_json::json!({
                                "type": "tool_use",
                                "id": id,
                                "name": name,
                                "input": input
                            })
                        }
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => {
                            serde_json::json!({
                                "type": "tool_result",
                                "tool_use_id": tool_use_id,
                                "content": content,
                                "is_error": is_error
                            })
                        }
                    })
                    .collect();
                serde_json::json!({"role": role, "content": content})
            })
            .collect();

        let api_tools: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema
                })
            })
            .collect();

        serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "system": system,
            "messages": api_messages,
            "tools": api_tools
        })
    }

    fn parse_response(&self, body: serde_json::Value) -> Result<ProviderResponse> {
        let stop_reason = body["stop_reason"]
            .as_str()
            .unwrap_or("end_turn")
            .to_string();

        let input_tokens = body["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32;
        let output_tokens = body["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;

        let content_arr = body["content"]
            .as_array()
            .context("missing content array in response")?;

        let mut blocks = Vec::new();
        for block in content_arr {
            let block_type = block["type"].as_str().unwrap_or("");
            match block_type {
                "text" => {
                    let text = block["text"].as_str().unwrap_or("").to_string();
                    blocks.push(ContentBlock::Text { text });
                }
                "tool_use" => {
                    let id = block["id"].as_str().unwrap_or("").to_string();
                    let name = block["name"].as_str().unwrap_or("").to_string();
                    let input = block["input"].clone();
                    blocks.push(ContentBlock::ToolUse { id, name, input });
                }
                _ => {}
            }
        }

        Ok(ProviderResponse {
            message: Message {
                role: Role::Assistant,
                content: blocks,
            },
            stop: stop_reason == "end_turn",
            input_tokens,
            output_tokens,
        })
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
    ) -> Result<ProviderResponse> {
        let body = self.build_request_body(messages, tools, system);

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("failed to send request to Anthropic API")?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp
            .json()
            .await
            .context("failed to parse Anthropic API response")?;

        if !status.is_success() {
            let error_msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            anyhow::bail!("Anthropic API error ({}): {}", status, error_msg);
        }

        self.parse_response(resp_body)
    }

    fn name(&self) -> &str {
        "anthropic"
    }

    fn model(&self) -> &str {
        &self.model
    }
}
