// Agent loop for scripting tool evals.
//
// Two modes:
// - Scripted: all mock tools composed into a single ScriptedTool. LLM writes
//   bash scripts to orchestrate them.
// - Baseline: each mock tool exposed as a separate LLM tool. LLM calls them
//   one at a time via individual tool_use blocks.

use anyhow::{Context, Result};
use bashkit::{ScriptedTool, Tool, ToolArgs, ToolDef, ToolRequest};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::provider::{ContentBlock, Message, Provider, Role, ToolDefinition};
use crate::scripting_dataset::{MockBehavior, ScriptingEvalTask};

/// Trace from a scripting tool eval run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptingTrace {
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ScriptingToolCall>,
    pub tool_call_count: usize,
    pub turns: usize,
    pub natural_stop: bool,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub duration_ms: u64,
    /// Whether this was run in baseline mode (individual tools) vs scripted mode.
    pub baseline: bool,
    /// Total bytes of raw tool output across all calls.
    pub raw_tool_output_bytes: usize,
    /// Total bytes of tool output returned to the LLM (after formatting).
    pub tool_output_sent_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptingToolCall {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: String,
    pub exit_code: i32,
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

type MockCallback = Arc<dyn Fn(&ToolArgs) -> std::result::Result<String, String> + Send + Sync>;

/// Build a mock callback from a MockBehavior.
fn make_mock_callback(mock: MockBehavior) -> MockCallback {
    match mock {
        MockBehavior::Static(response) => Arc::new(move |_args: &ToolArgs| Ok(response.clone())),
        MockBehavior::ByParam {
            param,
            responses,
            default,
        } => Arc::new(move |args: &ToolArgs| {
            let key = args
                .params
                .get(&param)
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
            if let Some(resp) = responses.get(&key) {
                Ok(resp.clone())
            } else if let Some(ref def) = default {
                Ok(def.clone())
            } else {
                Err(format!("no mock response for {}={}", param, key))
            }
        }),
    }
}

/// Run scripting eval in **scripted** mode: single ScriptedTool wrapping all mock tools.
pub async fn run_scripted_agent(
    provider: &dyn Provider,
    task: &ScriptingEvalTask,
    max_turns: usize,
) -> Result<ScriptingTrace> {
    let mut builder = ScriptedTool::builder(&task.id);

    for mock_tool in &task.tools {
        let def =
            if mock_tool.schema.is_object() && !mock_tool.schema.as_object().unwrap().is_empty() {
                ToolDef::new(&mock_tool.name, &mock_tool.description)
                    .with_schema(mock_tool.schema.clone())
            } else {
                ToolDef::new(&mock_tool.name, &mock_tool.description)
            };
        let callback = make_mock_callback(mock_tool.mock.clone());
        builder = builder.tool(def, move |args: &ToolArgs| callback(args));
    }

    let mut scripted_tool = builder.short_description("Scripted tool eval").build();

    let tool_def = ToolDefinition {
        name: scripted_tool.name().to_string(),
        description: scripted_tool.description(),
        input_schema: scripted_tool.input_schema(),
    };

    let system = task
        .system
        .clone()
        .unwrap_or_else(|| scripted_tool.system_prompt());

    let mut messages = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: task.prompt.clone(),
        }],
    }];

    let mut all_tool_calls = Vec::new();
    let mut natural_stop = false;
    let mut total_input_tokens = 0u32;
    let mut total_output_tokens = 0u32;
    let mut turns = 0usize;
    let mut raw_tool_output_bytes = 0usize;
    let mut tool_output_sent_bytes = 0usize;
    let start = std::time::Instant::now();

    for _turn in 0..max_turns {
        let response = provider
            .chat(&messages, std::slice::from_ref(&tool_def), &system)
            .await
            .context("provider chat failed")?;

        turns += 1;
        total_input_tokens += response.input_tokens;
        total_output_tokens += response.output_tokens;
        messages.push(response.message.clone());

        if response.stop {
            natural_stop = true;
            break;
        }

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

            let resp = scripted_tool
                .execute(ToolRequest {
                    commands: commands.to_string(),
                })
                .await;

            let raw_bytes = resp.stdout.len() + resp.stderr.len();
            raw_tool_output_bytes += raw_bytes;

            let content = format_tool_output(&resp.stdout, &resp.stderr, resp.exit_code);
            tool_output_sent_bytes += content.len();

            all_tool_calls.push(ScriptingToolCall {
                tool_name: scripted_tool.name().to_string(),
                input: (*input).clone(),
                output: resp.stdout,
                exit_code: resp.exit_code,
            });

            result_blocks.push(ContentBlock::ToolResult {
                tool_use_id: (*id).clone(),
                content,
                is_error: resp.exit_code != 0,
            });
        }

        messages.push(Message {
            role: Role::ToolResult,
            content: result_blocks,
        });
    }

    Ok(ScriptingTrace {
        messages,
        tool_call_count: all_tool_calls.len(),
        tool_calls: all_tool_calls,
        turns,
        natural_stop,
        total_input_tokens,
        total_output_tokens,
        duration_ms: start.elapsed().as_millis() as u64,
        baseline: false,
        raw_tool_output_bytes,
        tool_output_sent_bytes,
    })
}

/// Run scripting eval in **baseline** mode: each mock tool as a separate LLM tool.
pub async fn run_baseline_agent(
    provider: &dyn Provider,
    task: &ScriptingEvalTask,
    max_turns: usize,
) -> Result<ScriptingTrace> {
    let callbacks: HashMap<String, MockCallback> = task
        .tools
        .iter()
        .map(|t| (t.name.clone(), make_mock_callback(t.mock.clone())))
        .collect();

    let tool_defs: Vec<ToolDefinition> = task
        .tools
        .iter()
        .map(|t| {
            let schema = if t.schema.is_object() && !t.schema.as_object().unwrap().is_empty() {
                t.schema.clone()
            } else {
                serde_json::json!({
                    "type": "object",
                    "properties": {}
                })
            };
            ToolDefinition {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: schema,
            }
        })
        .collect();

    let system = task.system.clone().unwrap_or_else(|| {
        let mut s = String::from("You have access to the following tools:\n\n");
        for t in &task.tools {
            s.push_str(&format!("- {}: {}\n", t.name, t.description));
        }
        s.push_str("\nCall tools with the appropriate parameters to accomplish the task.");
        s
    });

    let mut messages = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: task.prompt.clone(),
        }],
    }];

    let mut all_tool_calls = Vec::new();
    let mut natural_stop = false;
    let mut total_input_tokens = 0u32;
    let mut total_output_tokens = 0u32;
    let mut turns = 0usize;
    let mut raw_tool_output_bytes = 0usize;
    let mut tool_output_sent_bytes = 0usize;
    let start = std::time::Instant::now();

    for _turn in 0..max_turns {
        let response = provider
            .chat(&messages, &tool_defs, &system)
            .await
            .context("provider chat failed")?;

        turns += 1;
        total_input_tokens += response.input_tokens;
        total_output_tokens += response.output_tokens;
        messages.push(response.message.clone());

        if response.stop {
            natural_stop = true;
            break;
        }

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
        for (id, name, input) in &tool_uses {
            let args = ToolArgs {
                params: (*input).clone(),
                stdin: None,
            };

            let (stdout, stderr, exit_code) = match callbacks.get(name.as_str()) {
                Some(cb) => match cb(&args) {
                    Ok(out) => (out, String::new(), 0),
                    Err(msg) => (String::new(), msg, 1),
                },
                None => (String::new(), format!("unknown tool: {}", name), 1),
            };

            let raw_bytes = stdout.len() + stderr.len();
            raw_tool_output_bytes += raw_bytes;

            let content = format_tool_output(&stdout, &stderr, exit_code);
            tool_output_sent_bytes += content.len();

            all_tool_calls.push(ScriptingToolCall {
                tool_name: name.to_string(),
                input: (*input).clone(),
                output: stdout,
                exit_code,
            });

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

    Ok(ScriptingTrace {
        messages,
        tool_call_count: all_tool_calls.len(),
        tool_calls: all_tool_calls,
        turns,
        natural_stop,
        total_input_tokens,
        total_output_tokens,
        duration_ms: start.elapsed().as_millis() as u64,
        baseline: true,
        raw_tool_output_bytes,
        tool_output_sent_bytes,
    })
}
