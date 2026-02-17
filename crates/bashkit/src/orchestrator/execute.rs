//! OrchestratorTool execution: Tool impl, builtin adapter, documentation helpers.

use super::{OrchestratorTool, ToolCallback};
use crate::builtins::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::tool::{Tool, ToolRequest, ToolResponse, ToolStatus, VERSION};
use crate::Bash;
use async_trait::async_trait;
use schemars::schema_for;
use std::sync::Arc;

// ============================================================================
// ToolBuiltinAdapter — wraps ToolCallback as a Builtin
// ============================================================================

/// Adapts a [`ToolCallback`] into a [`Builtin`] so the interpreter can execute it.
struct ToolBuiltinAdapter {
    callback: ToolCallback,
}

#[async_trait]
impl Builtin for ToolBuiltinAdapter {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        match (self.callback)(ctx.args, ctx.stdin) {
            Ok(stdout) => Ok(ExecResult::ok(stdout)),
            Err(msg) => Ok(ExecResult::err(msg, 1)),
        }
    }
}

// ============================================================================
// OrchestratorTool — internal helpers
// ============================================================================

impl OrchestratorTool {
    /// Create a fresh Bash instance with all tool builtins registered.
    fn create_bash(&self) -> Bash {
        let mut builder = Bash::builder();

        if let Some(ref limits) = self.limits {
            builder = builder.limits(limits.clone());
        }
        for (key, value) in &self.env_vars {
            builder = builder.env(key, value);
        }
        for tool in &self.tools {
            let name = tool.def.name.clone();
            let builtin: Box<dyn Builtin> = Box::new(ToolBuiltinAdapter {
                callback: Arc::clone(&tool.callback),
            });
            builder = builder.builtin(name, builtin);
        }

        builder.build()
    }

    fn build_description(&self) -> String {
        let mut desc = format!(
            "Scripted tool orchestrator. Available tool-commands: {}",
            self.tools
                .iter()
                .map(|t| t.def.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        desc.push_str(". Also supports standard bash builtins (echo, jq, grep, sed, awk, etc.).");
        desc
    }

    fn build_help(&self) -> String {
        let mut doc = format!(
            "{name}(1)                       Tool Commands                       {name}(1)\n\n\
             NAME\n\
             \x20      {name} - {short_desc}\n\n\
             SYNOPSIS\n\
             \x20      {{\"commands\": \"<bash script using tool commands>\"}}\n\n\
             DESCRIPTION\n\
             \x20      Executes a bash script with access to tool-specific commands\n\
             \x20      and standard bash builtins. Use pipes, variables, loops, and\n\
             \x20      conditionals to orchestrate multiple tool calls in one request.\n\n\
             TOOL COMMANDS\n",
            name = self.name,
            short_desc = self.short_desc,
        );

        for t in &self.tools {
            doc.push_str(&format!(
                "       {:<20} {}\n",
                t.def.name, t.def.description
            ));
        }

        doc.push_str(
            "\nBASH BUILTINS\n\
             \x20      echo, cat, grep, sed, awk, jq, head, tail, sort, uniq, cut, tr,\n\
             \x20      wc, printf, test, [, true, false, cd, pwd, ls, find, xargs\n\n\
             INPUT\n\
             \x20      commands    Bash script to execute\n\n\
             OUTPUT\n\
             \x20      stdout      Combined standard output\n\
             \x20      stderr      Errors from tool commands or bash\n\
             \x20      exit_code   0 on success\n\n\
             EXAMPLES\n\
             \x20      Single tool call:\n\
             \x20          {{\"commands\": \"get_user 42\"}}\n\n\
             \x20      Pipeline:\n\
             \x20          {{\"commands\": \"get_user 42 | jq '.name'\"}}\n\n\
             \x20      Multi-step orchestration:\n\
             \x20          {{\"commands\": \"user=$(get_user 42)\\norders=$(get_orders 42)\\necho $user | jq -r '.name'\"}}\n",
        );

        doc
    }

    fn build_system_prompt(&self) -> String {
        let mut prompt = format!("# {}\n\n", self.name);
        prompt.push_str(&format!("{}\n\n", self.short_desc));

        prompt.push_str("Input: {\"commands\": \"<bash script>\"}\n");
        prompt.push_str("Output: {stdout, stderr, exit_code}\n\n");

        prompt.push_str("## Available tool commands\n\n");
        for t in &self.tools {
            prompt.push_str(&format!("- `{}`: {}\n", t.def.name, t.def.description));
            if let Some(obj) = t.def.input_schema.as_object() {
                if !obj.is_empty() {
                    prompt.push_str(&format!("  Schema: {}\n", t.def.input_schema));
                }
            }
        }

        prompt.push_str(
            "\n## Tips\n\n\
             - Pipe tool output through `jq` for JSON processing\n\
             - Use variables to pass data between tool calls\n\
             - Use `set -e` to stop on first error\n\
             - Standard builtins (echo, grep, sed, awk, etc.) are available\n",
        );

        prompt
    }
}

// ============================================================================
// Tool trait implementation
// ============================================================================

#[async_trait]
impl Tool for OrchestratorTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn short_description(&self) -> &str {
        &self.short_desc
    }

    fn description(&self) -> String {
        self.build_description()
    }

    fn help(&self) -> String {
        self.build_help()
    }

    fn system_prompt(&self) -> String {
        self.build_system_prompt()
    }

    fn input_schema(&self) -> serde_json::Value {
        let schema = schema_for!(ToolRequest);
        serde_json::to_value(schema).unwrap_or_default()
    }

    fn output_schema(&self) -> serde_json::Value {
        let schema = schema_for!(ToolResponse);
        serde_json::to_value(schema).unwrap_or_default()
    }

    fn version(&self) -> &str {
        VERSION
    }

    async fn execute(&mut self, req: ToolRequest) -> ToolResponse {
        if req.commands.is_empty() {
            return ToolResponse {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                error: None,
            };
        }

        let mut bash = self.create_bash();

        match bash.exec(&req.commands).await {
            Ok(result) => result.into(),
            Err(e) => ToolResponse {
                stdout: String::new(),
                stderr: e.to_string(),
                exit_code: 1,
                error: Some(format!("{:?}", e)),
            },
        }
    }

    async fn execute_with_status(
        &mut self,
        req: ToolRequest,
        mut status_callback: Box<dyn FnMut(ToolStatus) + Send>,
    ) -> ToolResponse {
        status_callback(ToolStatus::new("validate").with_percent(0.0));

        if req.commands.is_empty() {
            status_callback(ToolStatus::new("complete").with_percent(100.0));
            return ToolResponse {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                error: None,
            };
        }

        status_callback(ToolStatus::new("parse").with_percent(10.0));
        let mut bash = self.create_bash();
        status_callback(ToolStatus::new("execute").with_percent(20.0));

        let response = match bash.exec(&req.commands).await {
            Ok(result) => result.into(),
            Err(e) => ToolResponse {
                stdout: String::new(),
                stderr: e.to_string(),
                exit_code: 1,
                error: Some(format!("{:?}", e)),
            },
        };

        status_callback(ToolStatus::new("complete").with_percent(100.0));
        response
    }
}
