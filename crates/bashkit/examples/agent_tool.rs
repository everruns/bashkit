//! LLM Agent example using BashKit as a Bash tool
//!
//! Demonstrates a real AI agent using Claude to execute bash commands
//! in a sandboxed BashKit session.
//!
//! Run with: ANTHROPIC_API_KEY=your-key cargo run --example agent_tool --features http_client
//!
//! The agent will autonomously create folders and write poems using the Bash tool.

use bashkit::{Bash, InMemoryFs};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ============================================================================
// Anthropic API Types
// ============================================================================

#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: &'static str,
    max_tokens: u32,
    system: String,
    tools: Vec<Tool>,
    messages: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: MessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
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
    },
}

#[derive(Debug, Serialize)]
struct Tool {
    name: &'static str,
    description: &'static str,
    input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
}

// ============================================================================
// Agent Implementation
// ============================================================================

struct Agent {
    bash: Bash,
    client: reqwest::Client,
    api_key: String,
    messages: Vec<Message>,
}

impl Agent {
    fn new(api_key: String) -> Self {
        let fs = Arc::new(InMemoryFs::new());
        Self {
            bash: Bash::builder().fs(fs).build(),
            client: reqwest::Client::new(),
            api_key,
            messages: Vec::new(),
        }
    }

    fn bash_tool() -> Tool {
        Tool {
            name: "bash",
            description: "Execute bash commands in a sandboxed session. \
                         Variables and functions persist between calls. \
                         Available commands: echo, cat, printf, grep, sed, awk, jq, \
                         cd, pwd, test, for/while/if, functions, redirections (> >>).",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute_bash(&mut self, command: &str) -> String {
        match self.bash.exec(command).await {
            Ok(result) => {
                let mut output = String::new();
                if !result.stdout.is_empty() {
                    output.push_str(&result.stdout);
                }
                if !result.stderr.is_empty() {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str("stderr: ");
                    output.push_str(&result.stderr);
                }
                if result.exit_code != 0 {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(&format!("exit code: {}", result.exit_code));
                }
                if output.is_empty() {
                    "(command completed successfully)".to_string()
                } else {
                    output
                }
            }
            Err(e) => format!("error: {}", e),
        }
    }

    async fn call_claude(&self, messages: &[Message]) -> anyhow::Result<MessagesResponse> {
        let request = MessagesRequest {
            model: "claude-sonnet-4-20250514",
            max_tokens: 1024,
            system: "You are an agent with access to a sandboxed bash environment. \
                    Your task is to create a few text files with short poems about different topics. \
                    Use echo with redirection to create files (e.g., echo 'poem' > /topic.txt). \
                    After creating files, read them back with cat to verify. \
                    Be concise. When done, say DONE."
                .to_string(),
            tools: vec![Self::bash_tool()],
            messages: messages.to_vec(),
        };

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await?;
            anyhow::bail!("API error {}: {}", status, body);
        }

        Ok(response.json().await?)
    }

    async fn run(&mut self, task: &str) -> anyhow::Result<()> {
        println!("Task: {}\n", task);

        // Initial user message
        self.messages.push(Message {
            role: "user".to_string(),
            content: MessageContent::Text(task.to_string()),
        });

        let mut turn = 0;
        loop {
            turn += 1;
            println!("--- Turn {} ---", turn);

            let response = self.call_claude(&self.messages).await?;

            // Process response content
            let mut tool_uses = Vec::new();
            let mut has_text = false;

            for block in &response.content {
                match block {
                    ContentBlock::Text { text } => {
                        println!("Claude: {}", text);
                        has_text = true;
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        if name == "bash" {
                            if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                                println!("$ {}", cmd);
                                let result = self.execute_bash(cmd).await;
                                println!("{}", result);
                                tool_uses.push((id.clone(), result));
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Add assistant message
            self.messages.push(Message {
                role: "assistant".to_string(),
                content: MessageContent::Blocks(response.content),
            });

            // If there were tool uses, add results and continue
            if !tool_uses.is_empty() {
                let tool_results: Vec<ContentBlock> = tool_uses
                    .into_iter()
                    .map(|(id, result)| ContentBlock::ToolResult {
                        tool_use_id: id,
                        content: result,
                    })
                    .collect();

                self.messages.push(Message {
                    role: "user".to_string(),
                    content: MessageContent::Blocks(tool_results),
                });
            } else if has_text && response.stop_reason.as_deref() == Some("end_turn") {
                // No tool calls and natural stop - we're done
                break;
            }

            // Safety limit
            if turn >= 15 {
                println!("(reached turn limit)");
                break;
            }

            println!();
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== BashKit LLM Agent Example ===\n");

    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!("Error: ANTHROPIC_API_KEY environment variable not set");
        eprintln!(
            "Usage: ANTHROPIC_API_KEY=your-key cargo run --example agent_tool --features http_client"
        );
        std::process::exit(1);
    });

    let mut agent = Agent::new(api_key);

    agent
        .run("Create 3 short poems (4 lines each) about: nature, space, and ocean. Save each to a file, then display them all.")
        .await?;

    println!("\n=== Agent completed ===");
    Ok(())
}
