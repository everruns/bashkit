// Provider abstraction for LLM APIs
// Normalizes Anthropic Messages API and OpenAI Chat Completions API
// into common Message/ContentBlock types for the agent loop

pub mod anthropic;
pub mod openai;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub use anthropic::AnthropicProvider;
pub use openai::OpenAiProvider;

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
        _ => anyhow::bail!(
            "unknown provider: '{}'. Use 'anthropic' or 'openai'",
            provider_name
        ),
    }
}
