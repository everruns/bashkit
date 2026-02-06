// Agent loop: send messages → get response → execute tool calls → repeat
// Uses Bash directly (not BashTool) for persistent VFS across tool calls
// BashTool used only for input_schema/system_prompt/llmtext introspection

use anyhow::{Context, Result};
use bashkit::{Bash, BashTool, Tool};
use serde::{Deserialize, Serialize};

use crate::dataset::EvalTask;
use crate::provider::{ContentBlock, Message, Provider, Role, ToolDefinition};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub commands: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTrace {
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCallResult>,
    pub tool_call_count: usize,
    pub last_tool_response: Option<ToolCallResult>,
    pub natural_stop: bool,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
}

fn format_tool_output(stdout: &str, stderr: &str, exit_code: i32) -> String {
    let mut out = String::new();
    if !stdout.is_empty() {
        out.push_str(stdout);
    }
    if !stderr.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("STDERR: {}", stderr));
    }
    if exit_code != 0 {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("Exit code: {}", exit_code));
    }
    if out.is_empty() {
        out.push_str("(no output)");
    }
    out
}

/// Run the agent loop for a single task.
/// Returns (trace, bash) — bash kept for VFS inspection by scorer.
pub async fn run_agent_loop(
    provider: &dyn Provider,
    task: &EvalTask,
    max_turns: usize,
) -> Result<(AgentTrace, Bash)> {
    // Build Bash with pre-populated files
    let mut builder = Bash::builder().username("eval").hostname("bashkit-eval");

    for (path, content) in &task.files {
        builder = builder.mount_text(path, content);
    }
    let mut bash = builder.build();

    // Get tool definition from BashTool introspection
    let tool = BashTool::default();
    let tool_def = ToolDefinition {
        name: "bash".to_string(),
        description: tool.description(),
        input_schema: tool.input_schema(),
    };

    // Compose system message
    let default_system = format!("{}\n\n{}", tool.system_prompt(), tool.llmtext());
    let system = task.system.as_deref().unwrap_or(&default_system);

    // Initialize conversation
    let mut messages = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: task.prompt.clone(),
        }],
    }];

    let mut all_tool_calls = Vec::new();
    let mut last_tool_response = None;
    let mut natural_stop = false;
    let mut total_input_tokens = 0u32;
    let mut total_output_tokens = 0u32;

    for _turn in 0..max_turns {
        let response = provider
            .chat(&messages, std::slice::from_ref(&tool_def), system)
            .await
            .context("provider chat failed")?;

        total_input_tokens += response.input_tokens;
        total_output_tokens += response.output_tokens;
        messages.push(response.message.clone());

        if response.stop {
            natural_stop = true;
            break;
        }

        // Extract tool_use blocks
        let tool_uses: Vec<_> = response
            .message
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => Some((id, name, input)),
                _ => None,
            })
            .collect();

        if tool_uses.is_empty() {
            natural_stop = true;
            break;
        }

        let mut result_blocks = Vec::new();
        for (id, _name, input) in &tool_uses {
            let commands = input["commands"]
                .as_str()
                .or_else(|| input["script"].as_str())
                .unwrap_or("");

            let (stdout, stderr, exit_code) = match bash.exec(commands).await {
                Ok(r) => (r.stdout, r.stderr, r.exit_code),
                Err(e) => (String::new(), e.to_string(), 1),
            };

            let tcr = ToolCallResult {
                commands: commands.to_string(),
                stdout: stdout.clone(),
                stderr: stderr.clone(),
                exit_code,
            };
            all_tool_calls.push(tcr.clone());
            last_tool_response = Some(tcr);

            let content = format_tool_output(&stdout, &stderr, exit_code);
            result_blocks.push(ContentBlock::ToolResult {
                tool_use_id: (*id).clone(),
                content,
                is_error: exit_code != 0,
            });
        }

        messages.push(Message {
            role: Role::ToolResult,
            content: result_blocks,
        });
    }

    Ok((
        AgentTrace {
            messages,
            tool_call_count: all_tool_calls.len(),
            tool_calls: all_tool_calls,
            last_tool_response,
            natural_stop,
            total_input_tokens,
            total_output_tokens,
        },
        bash,
    ))
}
