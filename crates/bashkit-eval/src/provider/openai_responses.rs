// OpenAI Responses API provider
// POST https://api.openai.com/v1/responses
// Uses input items (message, function_call, function_call_output) instead of chat messages
// Tool definitions are flat: {type, name, description, parameters}
// Multi-turn via manual input chaining (no previous_response_id dependency)

use anyhow::{Context, Result};
use async_trait::async_trait;

use super::{ContentBlock, Message, Provider, ProviderResponse, Role, ToolDefinition};

pub struct OpenAiResponsesProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl OpenAiResponsesProvider {
    pub fn new(model: &str) -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY env var not set")?;
        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            model: model.to_string(),
        })
    }

    /// Convert internal Message list to Responses API input items array.
    /// User messages → {role: "user", content: "..."}
    /// Assistant messages → message items + function_call items
    /// ToolResult messages → function_call_output items
    fn build_input_items(&self, messages: &[Message]) -> Vec<serde_json::Value> {
        let mut items: Vec<serde_json::Value> = Vec::new();

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
                    items.push(serde_json::json!({
                        "role": "user",
                        "content": text
                    }));
                }
                Role::Assistant => {
                    // Text content → message output item
                    let text_parts: Vec<serde_json::Value> = m
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(serde_json::json!({
                                "type": "output_text",
                                "text": text
                            })),
                            _ => None,
                        })
                        .collect();

                    if !text_parts.is_empty() {
                        items.push(serde_json::json!({
                            "type": "message",
                            "role": "assistant",
                            "content": text_parts
                        }));
                    }

                    // Tool use blocks → function_call items
                    for b in &m.content {
                        if let ContentBlock::ToolUse { id, name, input } = b {
                            items.push(serde_json::json!({
                                "type": "function_call",
                                "call_id": id,
                                "name": name,
                                "arguments": serde_json::to_string(input).unwrap_or_default()
                            }));
                        }
                    }
                }
                Role::ToolResult => {
                    for b in &m.content {
                        if let ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } = b
                        {
                            items.push(serde_json::json!({
                                "type": "function_call_output",
                                "call_id": tool_use_id,
                                "output": content
                            }));
                        }
                    }
                }
            }
        }

        items
    }

    fn build_request_body(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
    ) -> serde_json::Value {
        let input = self.build_input_items(messages);

        // Responses API uses flat tool definitions (no nested "function" key)
        let api_tools: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.model,
            "instructions": system,
            "input": input,
            "tools": api_tools,
            "store": false
        });

        // Codex models support reasoning effort
        if self.model.contains("codex") {
            body["reasoning"] = serde_json::json!({"effort": "high"});
        }

        body
    }

    fn parse_response(&self, body: serde_json::Value) -> Result<ProviderResponse> {
        let status = body["status"].as_str().unwrap_or("completed");

        let input_tokens = body["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32;
        let output_tokens = body["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;

        let output = body["output"]
            .as_array()
            .context("no output array in Responses API response")?;

        let mut blocks = Vec::new();
        let mut has_function_calls = false;

        for item in output {
            let item_type = item["type"].as_str().unwrap_or("");

            match item_type {
                "message" => {
                    // Extract text from content array
                    if let Some(content) = item["content"].as_array() {
                        for part in content {
                            if part["type"].as_str() == Some("output_text") {
                                if let Some(text) = part["text"].as_str() {
                                    if !text.is_empty() {
                                        blocks.push(ContentBlock::Text {
                                            text: text.to_string(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                "function_call" => {
                    has_function_calls = true;
                    let call_id = item["call_id"].as_str().unwrap_or("").to_string();
                    let name = item["name"].as_str().unwrap_or("").to_string();
                    let args_str = item["arguments"].as_str().unwrap_or("{}");
                    let input: serde_json::Value =
                        serde_json::from_str(args_str).unwrap_or(serde_json::json!({}));
                    blocks.push(ContentBlock::ToolUse {
                        id: call_id,
                        name,
                        input,
                    });
                }
                _ => {
                    // reasoning, other item types — skip
                }
            }
        }

        // Stop when status is completed/failed and no function calls pending
        let stop = !has_function_calls || status == "failed" || status == "cancelled";

        Ok(ProviderResponse {
            message: Message {
                role: Role::Assistant,
                content: blocks,
            },
            stop,
            input_tokens,
            output_tokens,
        })
    }
}

#[async_trait]
impl Provider for OpenAiResponsesProvider {
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
                .post("https://api.openai.com/v1/responses")
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .context("failed to send request to OpenAI Responses API")?;

            let status = resp.status();
            let resp_body: serde_json::Value = resp
                .json()
                .await
                .context("failed to parse OpenAI Responses API response")?;

            if status.is_success() {
                return self.parse_response(resp_body);
            }

            let error_msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");

            // Retry on 429 (rate limit) and 5xx (server errors)
            let retryable = status.as_u16() == 429 || status.is_server_error();
            if retryable {
                if let Some(&delay) = delays.get(attempt) {
                    eprintln!(
                        "  [retry] OpenAI Responses {} — waiting {}s (attempt {}/{})",
                        status,
                        delay,
                        attempt + 1,
                        delays.len()
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                    continue;
                }
            }

            anyhow::bail!("OpenAI Responses API error ({}): {}", status, error_msg);
        }

        unreachable!()
    }

    fn name(&self) -> &str {
        "openresponses"
    }

    fn model(&self) -> &str {
        &self.model
    }
}
