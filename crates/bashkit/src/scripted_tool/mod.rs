//! Scripted tool
//!
//! Compose tool definitions + callbacks into a single [`Tool`] that accepts bash
//! scripts. Each registered tool becomes a builtin command inside the interpreter,
//! so an LLM can orchestrate many operations in one call using pipes, variables,
//! loops, and conditionals.
//!
//! This module follows the same contract surface as [`crate::tool`]:
//!
//! - [`ScriptedToolBuilder::build`] -> immutable metadata object
//! - [`ScriptedToolBuilder::build_service`] -> `tower::Service<Value, Value>`
//! - [`Tool::execution`] -> validated, single-use [`crate::ToolExecution`]
//! - [`Tool::help`] -> Markdown docs
//! - [`Tool::system_prompt`] -> terse plain-text instructions
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │  ScriptedTool  (implements Tool)        │
//! │                                         │
//! │  ┌─────────┐ ┌─────────┐ ┌──────────┐  │
//! │  │get_user │ │get_order│ │inventory │  │
//! │  │(builtin)│ │(builtin)│ │(builtin) │  │
//! │  └─────────┘ └─────────┘ └──────────┘  │
//! │        ↑           ↑           ↑        │
//! │  bash script: pipes, vars, jq, loops    │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust
//! use bashkit::{ScriptedTool, Tool, ToolArgs, ToolDef};
//!
//! # tokio_test::block_on(async {
//! let tool = ScriptedTool::builder("api")
//!     .tool_fn(
//!         ToolDef::new("greet", "Greet a user")
//!             .with_schema(serde_json::json!({
//!                 "type": "object",
//!                 "properties": { "name": {"type": "string"} }
//!             })),
//!         |args: &ToolArgs| {
//!             let name = args.param_str("name").unwrap_or("world");
//!             Ok(format!("hello {name}\n"))
//!         },
//!     )
//!     .build();
//!
//! let output = tool
//!     .execution(serde_json::json!({"commands": "greet --name Alice"}))
//!     .expect("valid args")
//!     .execute()
//!     .await
//!     .expect("execution succeeds");
//!
//! assert_eq!(output.result["stdout"], "hello Alice\n");
//! assert!(tool.help().contains("## Tool Commands"));
//! # });
//! ```
//!
//! # Shared context across callbacks
//!
//! When multiple tool callbacks need shared resources (HTTP clients, auth tokens,
//! config), use the standard Rust closure-capture pattern with `Arc`:
//!
//! ```rust
//! use bashkit::{ScriptedTool, ToolArgs, ToolDef};
//! use std::sync::Arc;
//!
//! let api_key = Arc::new("sk-secret-key".to_string());
//! let base_url = Arc::new("https://api.example.com".to_string());
//!
//! let k = api_key.clone();
//! let u = base_url.clone();
//! let mut builder = ScriptedTool::builder("api");
//! builder = builder.tool_fn(
//!     ToolDef::new("get_user", "Fetch user by ID"),
//!     move |args: &ToolArgs| {
//!         let _key = &*k;   // shared API key
//!         let _url = &*u;   // shared base URL
//!         Ok(format!("{{\"id\":1}}\n"))
//!     },
//! );
//!
//! let k2 = api_key.clone();
//! let u2 = base_url.clone();
//! builder = builder.tool_fn(
//!     ToolDef::new("list_orders", "List orders"),
//!     move |_args: &ToolArgs| {
//!         let _key = &*k2;
//!         let _url = &*u2;
//!         Ok(format!("[]\n"))
//!     },
//! );
//! let _tool = builder.build();
//! ```
//!
//! For mutable shared state, use `Arc<Mutex<T>>`:
//!
//! ```rust
//! use bashkit::{ScriptedTool, ToolArgs, ToolDef};
//! use std::sync::{Arc, Mutex};
//!
//! let call_count = Arc::new(Mutex::new(0u64));
//! let c = call_count.clone();
//! let tool = ScriptedTool::builder("api")
//!     .tool_fn(
//!         ToolDef::new("tracked", "Counted call"),
//!         move |_args: &ToolArgs| {
//!             let mut count = c.lock().unwrap();
//!             *count += 1;
//!             Ok(format!("call #{count}\n"))
//!         },
//!     )
//!     .build();
//! ```
//!
//! # State across execute() calls
//!
//! Each `execute()` creates a fresh Bash interpreter — no state carries over.
//! This is a security feature (clean sandbox per call). The LLM carries state
//! between calls via its context window: it sees stdout from each call and can
//! pass relevant data from one call's output into the next call's script.
//!
//! For persistent state across calls via callbacks, use `Arc` in closures —
//! the same `Arc<ToolCallback>` instances are reused across `execute()` calls.

mod execute;
mod toolset;

pub use toolset::{DiscoverTool, DiscoveryMode, ScriptingToolSet, ScriptingToolSetBuilder};

// Re-export foundational types from tool_def (they used to live here).
pub use crate::tool_def::{
    AsyncToolCallback, AsyncToolExec, SyncToolExec, ToolArgs, ToolCallback, ToolDef, ToolImpl,
};

use crate::{ExecutionLimits, Tool, ToolService};
use schemars::schema_for;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// Sync or async callback for a registered tool.
#[derive(Clone)]
pub enum CallbackKind {
    /// Synchronous callback — blocks until complete.
    Sync(SyncToolExec),
    /// Asynchronous callback — `.await`ed inside the interpreter.
    Async(AsyncToolExec),
}

// ============================================================================
// Execution trace — inner scripted command/builtin usage
// ============================================================================

/// Kind of inner command invocation recorded during a `ScriptedTool` execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptedCommandKind {
    Tool,
    Help,
    Discover,
}

/// One builtin/tool invocation inside a scripted tool execute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptedCommandInvocation {
    pub name: String,
    pub kind: ScriptedCommandKind,
    pub args: Vec<String>,
    pub exit_code: i32,
}

/// Inner execution trace captured for the last `ScriptedTool::execute()` call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScriptedExecutionTrace {
    pub invocations: Vec<ScriptedCommandInvocation>,
}

// ============================================================================
// RegisteredTool — internal definition + callback pair
// ============================================================================

/// A registered tool: definition + callback.
#[derive(Clone)]
pub(crate) struct RegisteredTool {
    pub(crate) def: ToolDef,
    pub(crate) callback: CallbackKind,
    pub(crate) dry_run: Option<CallbackKind>,
}

impl RegisteredTool {
    /// Create from a [`ToolImpl`], converting its exec/exec_sync to a
    /// [`CallbackKind`]. Prefers async when available.
    pub(crate) fn from_tool_impl(tool: ToolImpl) -> Self {
        let callback = if let Some(async_cb) = tool.exec {
            CallbackKind::Async(async_cb)
        } else if let Some(sync_cb) = tool.exec_sync {
            CallbackKind::Sync(sync_cb)
        } else {
            // Schema-only ToolImpl — wrap as a sync callback that always errors.
            let name = tool.def.name.clone();
            CallbackKind::Sync(Arc::new(move |_| Err(format!("{name}: no exec defined"))))
        };
        Self {
            def: tool.def,
            callback,
            dry_run: None,
        }
    }
}

// ============================================================================
// ScriptedToolBuilder
// ============================================================================

/// Builder for [`ScriptedTool`].
///
/// ```rust
/// use bashkit::{ScriptedTool, ToolArgs, ToolDef};
///
/// let tool = ScriptedTool::builder("net")
///     .short_description("Network tools")
///     .tool_fn(
///         ToolDef::new("ping", "Ping a host")
///             .with_schema(serde_json::json!({
///                 "type": "object",
///                 "properties": { "host": {"type": "string"} }
///             })),
///         |args: &ToolArgs| {
///             Ok(format!("pong {}\n", args.param_str("host").unwrap_or("?")))
///         },
///     )
///     .build();
/// ```
pub struct ScriptedToolBuilder {
    name: String,
    locale: String,
    short_desc: Option<String>,
    tools: Vec<RegisteredTool>,
    limits: Option<ExecutionLimits>,
    env_vars: Vec<(String, String)>,
    compact_prompt: bool,
    /// When true, callback errors are replaced with a generic message to prevent
    /// leaking internal details (file paths, connection strings, stack traces).
    sanitize_errors: bool,
}

impl ScriptedToolBuilder {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            locale: "en-US".to_string(),
            short_desc: None,
            tools: Vec::new(),
            limits: None,
            env_vars: Vec::new(),
            compact_prompt: false,
            sanitize_errors: true,
        }
    }

    /// Set locale for descriptions, help, prompts, and user-facing errors.
    pub fn locale(mut self, locale: &str) -> Self {
        self.locale = locale.to_string();
        self
    }

    /// One-line description for tool listings.
    pub fn short_description(mut self, desc: impl Into<String>) -> Self {
        self.short_desc = Some(desc.into());
        self
    }

    /// Register a [`ToolImpl`] (definition + exec functions).
    ///
    /// This is the preferred registration method. The `ToolImpl` carries its own
    /// name, schema, and sync/async exec.
    pub fn tool(mut self, tool: ToolImpl) -> Self {
        self.tools.push(RegisteredTool::from_tool_impl(tool));
        self
    }

    /// Register a tool with its definition and synchronous exec function.
    ///
    /// Convenience shorthand — constructs a [`ToolImpl`] internally.
    /// The exec receives [`ToolArgs`] with `--key value` flags parsed into
    /// a JSON object, type-coerced per the schema.
    pub fn tool_fn(
        mut self,
        def: ToolDef,
        exec: impl Fn(&ToolArgs) -> Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        self.tools.push(RegisteredTool {
            def,
            callback: CallbackKind::Sync(Arc::new(exec)),
            dry_run: None,
        });
        self
    }

    /// Register a tool with its definition, exec function, and a custom
    /// `--dry-run` handler. When the tool is invoked with `--dry-run`,
    /// the custom handler runs instead of the regular callback.
    pub fn tool_with_dry_run(
        mut self,
        def: ToolDef,
        exec: impl Fn(&ToolArgs) -> Result<String, String> + Send + Sync + 'static,
        dry_run: impl Fn(&ToolArgs) -> Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        self.tools.push(RegisteredTool {
            def,
            callback: CallbackKind::Sync(Arc::new(exec)),
            dry_run: Some(CallbackKind::Sync(Arc::new(dry_run))),
        });
        self
    }

    /// Register a tool with its definition and **async** exec function.
    ///
    /// Convenience shorthand — constructs a [`ToolImpl`] internally.
    /// Same as [`tool_fn()`](Self::tool_fn) but returns a `Future`,
    /// allowing non-blocking I/O. Takes owned [`ToolArgs`] because the future
    /// may outlive the borrow.
    pub fn async_tool_fn<F, Fut>(mut self, def: ToolDef, exec: F) -> Self
    where
        F: Fn(ToolArgs) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<String, String>> + Send + 'static,
    {
        let cb: AsyncToolExec = Arc::new(move |args| Box::pin(exec(args)));
        self.tools.push(RegisteredTool {
            def,
            callback: CallbackKind::Async(cb),
            dry_run: None,
        });
        self
    }

    /// Set execution limits for the bash interpreter.
    pub fn limits(mut self, limits: ExecutionLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Add an environment variable visible inside scripts.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.push((key.into(), value.into()));
        self
    }

    /// Control whether callback error messages are sanitized before appearing in
    /// tool output. When `true` (the default), internal error details are replaced
    /// with a generic "callback failed" message to prevent leaking file paths,
    /// connection strings, or stack traces to LLM agents.
    // THREAT[TM-INF-030]: Prevent information disclosure through callback errors.
    pub fn sanitize_errors(mut self, sanitize: bool) -> Self {
        self.sanitize_errors = sanitize;
        self
    }

    /// Emit compact `system_prompt()` that omits full schemas and adds help tip.
    ///
    /// When enabled, `system_prompt()` lists only tool names + one-liners and
    /// instructs the LLM to use `help <tool>` / `help <tool> --json` for details.
    /// Default: `false` (full schemas in prompt, backward compatible).
    pub fn compact_prompt(mut self, compact: bool) -> Self {
        self.compact_prompt = compact;
        self
    }

    /// Build the [`ScriptedTool`].
    pub fn build(&self) -> ScriptedTool {
        let short_desc = self
            .short_desc
            .clone()
            .unwrap_or_else(|| format!("ScriptedTool: {}", self.name));
        let tool_names = self
            .tools
            .iter()
            .map(|tool| tool.def.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        ScriptedTool {
            name: self.name.clone(),
            locale: self.locale.clone(),
            display_name: self.name.clone(),
            short_desc,
            description: format!(
                "{}: {}",
                super::tool::localized(
                    self.locale.as_str(),
                    "Compose tool callbacks through bash scripts",
                    "Компонує виклики інструментів через bash-скрипти",
                ),
                tool_names
            ),
            tools: self.tools.clone(),
            limits: self.limits.clone(),
            env_vars: self.env_vars.clone(),
            compact_prompt: self.compact_prompt,
            sanitize_errors: self.sanitize_errors,
            last_execution_trace: Arc::new(Mutex::new(None)),
        }
    }

    /// Build a `tower::Service<Value, Response = Value, Error = ToolError>`.
    pub fn build_service(&self) -> ToolService {
        let tool = self.build();
        tower::util::BoxCloneService::new(tower::service_fn(move |args| {
            let tool = tool.clone();
            async move {
                let execution = tool.execution(args)?;
                let output = execution.execute().await?;
                Ok(output.result)
            }
        }))
    }

    /// Build an OpenAI-compatible tool definition.
    pub fn build_tool_definition(&self) -> serde_json::Value {
        let tool = self.build();
        serde_json::json!({
            "type": "function",
            "function": {
                "name": tool.name(),
                "description": tool.description(),
                "parameters": self.build_input_schema(),
            }
        })
    }

    /// Build the input schema without constructing the full tool.
    pub fn build_input_schema(&self) -> serde_json::Value {
        let schema = schema_for!(crate::tool::ToolRequest);
        serde_json::to_value(schema).unwrap_or_default()
    }

    /// Build the output schema for `ToolOutput::result`.
    pub fn build_output_schema(&self) -> serde_json::Value {
        let schema = schema_for!(crate::tool::ToolResponse);
        serde_json::to_value(schema).unwrap_or_default()
    }
}

// ============================================================================
// ScriptedTool
// ============================================================================

/// A [`Tool`] that orchestrates multiple tools via bash scripts.
///
/// Each registered tool (defined by [`ToolDef`] + callback) becomes a bash builtin.
/// The LLM sends a bash script that can pipe, loop, branch, and compose these
/// builtins together with standard utilities like `jq`, `grep`, `sed`, etc.
///
/// Arguments are passed as `--key value` flags and parsed into typed JSON
/// per the tool's `input_schema`.
///
/// Reusable — `execute()` can be called multiple times. Each call gets a fresh
/// Bash interpreter with the same set of tool builtins.
///
/// Create via [`ScriptedTool::builder`].
#[derive(Clone)]
pub struct ScriptedTool {
    pub(crate) name: String,
    pub(crate) locale: String,
    pub(crate) display_name: String,
    pub(crate) short_desc: String,
    pub(crate) description: String,
    pub(crate) tools: Vec<RegisteredTool>,
    pub(crate) limits: Option<ExecutionLimits>,
    pub(crate) env_vars: Vec<(String, String)>,
    pub(crate) compact_prompt: bool,
    pub(crate) sanitize_errors: bool,
    pub(crate) last_execution_trace: Arc<Mutex<Option<ScriptedExecutionTrace>>>,
}

impl ScriptedTool {
    /// Create a builder with the given tool name.
    pub fn builder(name: impl Into<String>) -> ScriptedToolBuilder {
        ScriptedToolBuilder::new(name)
    }

    /// Return and clear the trace from the most recent execute call.
    pub fn take_last_execution_trace(&self) -> Option<ScriptedExecutionTrace> {
        self.last_execution_trace
            .lock()
            .expect("scripted execution trace poisoned")
            .take()
    }

    pub(crate) fn store_last_execution_trace(&self, trace: ScriptedExecutionTrace) {
        *self
            .last_execution_trace
            .lock()
            .expect("scripted execution trace poisoned") = Some(trace);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{Tool, ToolRequest, VERSION};

    fn build_test_tool() -> ScriptedTool {
        ScriptedTool::builder("test_api")
            .short_description("Test API")
            .tool_fn(
                ToolDef::new("get_user", "Fetch user by id").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"}
                    }
                })),
                |args: &ToolArgs| {
                    let id = args.param_i64("id").ok_or("missing --id")?;
                    Ok(format!(
                        "{{\"id\":{id},\"name\":\"Alice\",\"email\":\"alice@example.com\"}}\n"
                    ))
                },
            )
            .tool_fn(
                ToolDef::new("get_orders", "List orders for user").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "user_id": {"type": "integer"}
                    }
                })),
                |args: &ToolArgs| {
                    let uid = args.param_i64("user_id").ok_or("missing --user_id")?;
                    Ok(format!(
                        "[{{\"order_id\":1,\"user_id\":{uid},\"total\":29.99}},\
                         {{\"order_id\":2,\"user_id\":{uid},\"total\":49.50}}]\n"
                    ))
                },
            )
            .tool_fn(
                ToolDef::new("fail_tool", "Always fails"),
                |_args: &ToolArgs| Err("service unavailable".to_string()),
            )
            .tool_fn(
                ToolDef::new("from_stdin", "Read from stdin, uppercase it"),
                |args: &ToolArgs| match args.stdin.as_deref() {
                    Some(input) => Ok(input.to_uppercase()),
                    None => Err("no stdin".to_string()),
                },
            )
            .build()
    }

    // -- Builder tests --

    #[test]
    fn test_builder_name_and_description() {
        let tool = build_test_tool();
        assert_eq!(tool.name(), "test_api");
        assert_eq!(tool.short_description(), "Test API");
    }

    #[test]
    fn test_builder_default_short_description() {
        let tool = ScriptedTool::builder("mytools")
            .tool_fn(ToolDef::new("noop", "No-op"), |_args: &ToolArgs| {
                Ok("ok\n".to_string())
            })
            .build();
        assert_eq!(tool.short_description(), "ScriptedTool: mytools");
    }

    #[test]
    fn test_description_lists_tools() {
        let tool = build_test_tool();
        let desc = tool.description();
        assert!(desc.contains("get_user"));
        assert!(desc.contains("get_orders"));
        assert!(desc.contains("fail_tool"));
        assert!(desc.contains("from_stdin"));
    }

    #[test]
    fn test_help_has_tool_commands_section() {
        let tool = build_test_tool();
        let help = tool.help();
        assert!(help.contains("## Tool Commands"));
        assert!(help.contains("get_user"));
        assert!(help.contains("Fetch user by id"));
    }

    #[test]
    fn test_system_prompt_lists_tools() {
        let tool = build_test_tool();
        let sp = tool.system_prompt();
        assert!(sp.starts_with("test_api:"));
        assert!(sp.contains("get_user"));
        assert!(sp.contains("get_orders"));
        assert!(sp.contains("--key value"));
    }

    #[test]
    fn test_system_prompt_includes_schema() {
        let tool = ScriptedTool::builder("schema_test")
            .tool_fn(
                ToolDef::new("get_user", "Fetch user by id").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"}
                    },
                    "required": ["id"]
                })),
                |_args: &ToolArgs| Ok("ok\n".to_string()),
            )
            .build();
        let sp = tool.system_prompt();
        assert!(
            sp.contains("--id <integer>"),
            "system prompt should show flags"
        );
    }

    #[test]
    fn test_schemas() {
        let tool = build_test_tool();
        let input = tool.input_schema();
        assert!(input["properties"]["commands"].is_object());
        let output = tool.output_schema();
        assert!(output["properties"]["stdout"].is_object());
    }

    #[test]
    fn test_builder_contract_helpers() {
        let builder = ScriptedTool::builder("test_api")
            .tool_fn(ToolDef::new("ping", "Ping"), |_args: &ToolArgs| {
                Ok("pong\n".to_string())
            });
        let definition = builder.build_tool_definition();
        let input_schema = builder.build_input_schema();
        let output_schema = builder.build_output_schema();

        assert_eq!(definition["type"], "function");
        assert_eq!(definition["function"]["name"], "test_api");
        assert_eq!(definition["function"]["parameters"], input_schema);
        assert!(output_schema["properties"]["stdout"].is_object());
    }

    #[tokio::test]
    async fn test_builder_service_executes() {
        use tower::ServiceExt;

        let service = ScriptedTool::builder("test_api")
            .tool_fn(ToolDef::new("ping", "Ping"), |_args: &ToolArgs| {
                Ok("pong\n".to_string())
            })
            .build_service();

        let result = service
            .oneshot(serde_json::json!({"commands": "ping"}))
            .await
            .unwrap_or_else(|err| panic!("service should execute: {err}"));

        assert_eq!(result["stdout"], "pong\n");
        assert_eq!(result["exit_code"], 0);
    }

    #[test]
    fn test_locale_localizes_description() {
        let tool = ScriptedTool::builder("ua_api")
            .locale("uk-UA")
            .tool_fn(ToolDef::new("ping", "Ping"), |_args: &ToolArgs| {
                Ok("pong\n".to_string())
            })
            .build();

        assert!(tool.description().contains("Компонує"));
        assert_eq!(tool.locale(), "uk-UA");
    }

    #[test]
    fn test_version() {
        let tool = build_test_tool();
        assert_eq!(tool.version(), VERSION);
    }

    // -- Execution tests --

    #[tokio::test]
    async fn test_execute_empty() {
        let tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: String::new(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_execute_single_tool() {
        let tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "get_user --id 42".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("\"name\":\"Alice\""));
        assert!(resp.stdout.contains("\"id\":42"));
    }

    #[tokio::test]
    async fn test_execute_key_equals_value() {
        let tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "get_user --id=42".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("\"id\":42"));
    }

    #[tokio::test]
    async fn test_execute_pipeline_with_jq() {
        let tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "get_user --id 42 | jq -r '.name'".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "Alice");
    }

    #[tokio::test]
    async fn test_execute_multi_step() {
        let tool = build_test_tool();
        let script = r#"
            user=$(get_user --id 1)
            name=$(echo "$user" | jq -r '.name')
            orders=$(get_orders --user_id 1)
            total=$(echo "$orders" | jq '[.[].total] | add')
            echo "User: $name, Total: $total"
        "#;
        let resp = tool
            .execute(ToolRequest {
                commands: script.to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "User: Alice, Total: 79.49");
    }

    #[tokio::test]
    async fn test_execute_tool_failure() {
        let tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "fail_tool".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_ne!(resp.exit_code, 0);
        assert!(resp.stderr.contains("callback failed"));
    }

    #[tokio::test]
    async fn test_execute_tool_failure_with_fallback() {
        let tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "fail_tool || echo 'fallback'".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("fallback"));
    }

    #[tokio::test]
    async fn test_execute_stdin_pipe() {
        let tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "echo hello | from_stdin".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "HELLO");
    }

    #[tokio::test]
    async fn test_execute_loop_over_tools() {
        let tool = build_test_tool();
        let script = r#"
            for uid in 1 2 3; do
                get_user --id $uid | jq -r '.name'
            done
        "#;
        let resp = tool
            .execute(ToolRequest {
                commands: script.to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "Alice\nAlice\nAlice");
    }

    #[tokio::test]
    async fn test_execute_conditional() {
        let tool = build_test_tool();
        let script = r#"
            user=$(get_user --id 5)
            name=$(echo "$user" | jq -r '.name')
            if [ "$name" = "Alice" ]; then
                echo "found alice"
            else
                echo "not alice"
            fi
        "#;
        let resp = tool
            .execute(ToolRequest {
                commands: script.to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "found alice");
    }

    #[tokio::test]
    async fn test_execute_with_env() {
        let tool = ScriptedTool::builder("env_test")
            .env("API_BASE", "https://api.example.com")
            .tool_fn(ToolDef::new("noop", "No-op"), |_args: &ToolArgs| {
                Ok("ok\n".to_string())
            })
            .build();

        let resp = tool
            .execute(ToolRequest {
                commands: "echo $API_BASE".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "https://api.example.com");
    }

    #[tokio::test]
    async fn test_execute_with_status_callback() {
        use std::sync::{Arc, Mutex};

        let tool = build_test_tool();
        let phases = Arc::new(Mutex::new(Vec::new()));
        let phases_clone = phases.clone();

        let resp = tool
            .execute_with_status(
                ToolRequest {
                    commands: "get_user --id 1".to_string(),
                    timeout_ms: None,
                },
                Box::new(move |status| {
                    phases_clone
                        .lock()
                        .expect("lock poisoned")
                        .push(status.phase.clone());
                }),
            )
            .await;

        assert_eq!(resp.exit_code, 0);
        let phases = phases.lock().expect("lock poisoned");
        assert!(phases.contains(&"validate".to_string()));
        assert!(phases.contains(&"execute".to_string()));
        assert!(phases.contains(&"complete".to_string()));
    }

    #[tokio::test]
    async fn test_multiple_execute_calls() {
        let tool = build_test_tool();

        let resp1 = tool
            .execute(ToolRequest {
                commands: "get_user --id 1 | jq -r '.name'".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp1.stdout.trim(), "Alice");

        let resp2 = tool
            .execute(ToolRequest {
                commands: "get_orders --user_id 1 | jq 'length'".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp2.stdout.trim(), "2");
    }

    #[tokio::test]
    async fn test_boolean_flag() {
        let tool = ScriptedTool::builder("bool_test")
            .tool_fn(
                ToolDef::new("search", "Search").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"},
                        "verbose": {"type": "boolean"}
                    }
                })),
                |args: &ToolArgs| {
                    let q = args.param_str("query").unwrap_or("");
                    let v = args.param_bool("verbose").unwrap_or(false);
                    Ok(format!("q={q} verbose={v}\n"))
                },
            )
            .build();

        let resp = tool
            .execute(ToolRequest {
                commands: "search --verbose --query hello".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "q=hello verbose=true");
    }

    #[tokio::test]
    async fn test_no_schema_treats_as_strings() {
        let tool = ScriptedTool::builder("str_test")
            .tool_fn(
                ToolDef::new("echo_args", "Echo params as JSON"),
                |args: &ToolArgs| Ok(format!("{}\n", args.params)),
            )
            .build();

        let resp = tool
            .execute(ToolRequest {
                commands: "echo_args --name Alice --count 3".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        let parsed: serde_json::Value =
            serde_json::from_str(resp.stdout.trim()).expect("stdout should be valid JSON");
        assert_eq!(parsed["name"], "Alice");
        assert_eq!(parsed["count"], "3"); // string, not int — no schema
    }

    // -- Shared context tests (#522) --

    #[tokio::test]
    async fn test_shared_arc_across_callbacks() {
        use std::sync::{Arc, Mutex};

        let shared = Arc::new("shared-token".to_string());
        let call_log = Arc::new(Mutex::new(Vec::<String>::new()));

        let s1 = shared.clone();
        let log1 = call_log.clone();
        let s2 = shared.clone();
        let log2 = call_log.clone();

        let tool = ScriptedTool::builder("ctx_test")
            .tool_fn(
                ToolDef::new("tool_a", "First tool"),
                move |_args: &ToolArgs| {
                    log1.lock().expect("lock").push(format!("a:{}", *s1));
                    Ok("a\n".to_string())
                },
            )
            .tool_fn(
                ToolDef::new("tool_b", "Second tool"),
                move |_args: &ToolArgs| {
                    log2.lock().expect("lock").push(format!("b:{}", *s2));
                    Ok("b\n".to_string())
                },
            )
            .build();

        let resp = tool
            .execute(ToolRequest {
                commands: "tool_a && tool_b".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        let log = call_log.lock().expect("lock");
        assert_eq!(*log, vec!["a:shared-token", "b:shared-token"]);
    }

    #[tokio::test]
    async fn test_mutable_shared_state_across_callbacks() {
        use std::sync::{Arc, Mutex};

        let counter = Arc::new(Mutex::new(0u64));
        let c = counter.clone();

        let tool = ScriptedTool::builder("mut_test")
            .tool_fn(
                ToolDef::new("increment", "Bump counter"),
                move |_args: &ToolArgs| {
                    let mut count = c.lock().expect("lock");
                    *count += 1;
                    Ok(format!("{count}\n"))
                },
            )
            .build();

        let resp = tool
            .execute(ToolRequest {
                commands: "increment; increment; increment".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(*counter.lock().expect("lock"), 3);
    }

    // -- Fresh interpreter isolation test (#524) --

    #[tokio::test]
    async fn test_fresh_interpreter_per_execute() {
        let tool = ScriptedTool::builder("isolation_test")
            .tool_fn(ToolDef::new("noop", "No-op"), |_args: &ToolArgs| {
                Ok("ok\n".to_string())
            })
            .build();

        // Set a variable in call 1
        let resp1 = tool
            .execute(ToolRequest {
                commands: "export MY_VAR=hello; echo $MY_VAR".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp1.stdout.trim(), "hello");

        // Variable should NOT persist to call 2
        let resp2 = tool
            .execute(ToolRequest {
                commands: "echo \">${MY_VAR}<\"".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp2.stdout.trim(), "><");
    }

    #[tokio::test]
    async fn test_arc_callback_persists_across_execute_calls() {
        use std::sync::{Arc, Mutex};

        let counter = Arc::new(Mutex::new(0u64));
        let c = counter.clone();

        let tool = ScriptedTool::builder("persist_test")
            .tool_fn(
                ToolDef::new("count", "Count calls"),
                move |_args: &ToolArgs| {
                    let mut n = c.lock().expect("lock");
                    *n += 1;
                    Ok(format!("{n}\n"))
                },
            )
            .build();

        // Call 1
        let resp1 = tool
            .execute(ToolRequest {
                commands: "count".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp1.stdout.trim(), "1");

        // Call 2 — counter persists via Arc
        let resp2 = tool
            .execute(ToolRequest {
                commands: "count".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp2.stdout.trim(), "2");
    }

    #[tokio::test]
    async fn test_execution_trace_records_help_discover_and_tool_invocations() {
        let tool = build_test_tool();

        let resp = tool
            .execute(ToolRequest {
                commands: "discover --search user\nhelp get_user\nget_user --id 42".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);

        let trace = tool
            .take_last_execution_trace()
            .expect("execution trace should be recorded");
        assert_eq!(trace.invocations.len(), 3);
        assert_eq!(trace.invocations[0].name, "discover");
        assert_eq!(trace.invocations[0].kind, ScriptedCommandKind::Discover);
        assert_eq!(trace.invocations[1].name, "help");
        assert_eq!(trace.invocations[1].kind, ScriptedCommandKind::Help);
        assert_eq!(trace.invocations[2].name, "get_user");
        assert_eq!(trace.invocations[2].kind, ScriptedCommandKind::Tool);
    }

    // -- Async callback tests --

    #[tokio::test]
    async fn test_async_tool_basic() {
        let tool = ScriptedTool::builder("async_api")
            .async_tool_fn(
                ToolDef::new("greet", "Greet async").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": { "name": {"type": "string"} }
                })),
                |args: ToolArgs| async move {
                    let name = args.param_str("name").unwrap_or("world").to_string();
                    Ok(format!("hello {name}\n"))
                },
            )
            .build();

        let resp = tool
            .execute(ToolRequest {
                commands: "greet --name Async".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "hello Async");
    }

    #[tokio::test]
    async fn test_mixed_sync_async_tools() {
        let tool = ScriptedTool::builder("mixed")
            .tool_fn(ToolDef::new("sync_ping", "Sync"), |_args: &ToolArgs| {
                Ok("sync-pong\n".to_string())
            })
            .async_tool_fn(
                ToolDef::new("async_ping", "Async"),
                |_args: ToolArgs| async move { Ok("async-pong\n".to_string()) },
            )
            .build();

        let resp = tool
            .execute(ToolRequest {
                commands: "sync_ping; async_ping".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("sync-pong"));
        assert!(resp.stdout.contains("async-pong"));
    }

    #[tokio::test]
    async fn test_async_tool_error_propagates() {
        let tool = ScriptedTool::builder("err_api")
            .sanitize_errors(false)
            .async_tool_fn(
                ToolDef::new("fail", "Always fails"),
                |_args: ToolArgs| async move { Err("async boom".to_string()) },
            )
            .build();

        let resp = tool
            .execute(ToolRequest {
                commands: "fail".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_ne!(resp.exit_code, 0);
        assert!(resp.stderr.contains("async boom"));
    }

    #[tokio::test]
    async fn test_async_tool_stdin_pipe() {
        let tool = ScriptedTool::builder("pipe_api")
            .async_tool_fn(
                ToolDef::new("upper", "Uppercase stdin"),
                |args: ToolArgs| async move { Ok(args.stdin.unwrap_or_default().to_uppercase()) },
            )
            .build();

        let resp = tool
            .execute(ToolRequest {
                commands: "echo hello | upper".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("HELLO"));
    }

    // -- ToolImpl registration --

    #[tokio::test]
    async fn test_tool_impl_in_scripted_tool() {
        let get_user = ToolImpl::new(ToolDef::new("get_user", "Fetch user by ID").with_schema(
            serde_json::json!({
                "type": "object",
                "properties": { "id": {"type": "integer"} },
                "required": ["id"]
            }),
        ))
        .with_exec_sync(|args| {
            let id = args.param_i64("id").ok_or("missing --id")?;
            Ok(format!("{{\"id\":{id},\"name\":\"Alice\"}}\n"))
        });

        let tool = ScriptedTool::builder("api")
            .short_description("Test API")
            .tool(get_user)
            .build();

        assert!(tool.system_prompt().contains("get_user"));
        assert!(tool.help().contains("get_user"));

        let resp = tool
            .execute(ToolRequest {
                commands: "get_user --id 42 | jq -r '.name'".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "Alice");
    }

    #[tokio::test]
    async fn test_tool_impl_async_exec_in_scripted_tool() {
        let greet = ToolImpl::new(ToolDef::new("greet", "Greet someone").with_schema(
            serde_json::json!({
                "type": "object",
                "properties": { "name": {"type": "string"} }
            }),
        ))
        .with_exec(|args| async move {
            let name = args.param_str("name").unwrap_or("world");
            Ok(format!("hello {name}\n"))
        });

        let tool = ScriptedTool::builder("api").tool(greet).build();

        let resp = tool
            .execute(ToolRequest {
                commands: "greet --name Bob".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "hello Bob");
    }

    #[tokio::test]
    async fn test_tool_impl_mixed_with_tool_fn() {
        let tool_impl = ToolImpl::new(ToolDef::new("impl_cmd", "From ToolImpl"))
            .with_exec_sync(|_args| Ok("from_impl\n".to_string()));

        let tool = ScriptedTool::builder("mixed")
            .tool(tool_impl)
            .tool_fn(ToolDef::new("fn_cmd", "From tool_fn"), |_args| {
                Ok("from_fn\n".to_string())
            })
            .build();

        let resp = tool
            .execute(ToolRequest {
                commands: "echo $(impl_cmd) $(fn_cmd)".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("from_impl"));
        assert!(resp.stdout.contains("from_fn"));
    }

    // -- Issue #1278: --help flag tests --

    #[tokio::test]
    async fn test_tool_help_flag_returns_help_text() {
        let tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "get_user --help".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(
            resp.stdout.contains("get_user"),
            "help should include tool name"
        );
        assert!(
            resp.stdout.contains("Fetch user by id"),
            "help should include description"
        );
        assert!(
            resp.stdout.contains("--id"),
            "help should include parameter flags"
        );
    }

    #[tokio::test]
    async fn test_tool_help_flag_does_not_invoke_callback() {
        let tool = build_test_tool();
        // fail_tool always returns an error, but --help should not invoke it
        let resp = tool
            .execute(ToolRequest {
                commands: "fail_tool --help".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(
            resp.exit_code, 0,
            "--help should succeed even for fail_tool"
        );
        assert!(
            resp.stdout.contains("Always fails"),
            "help should include description"
        );
    }

    #[tokio::test]
    async fn test_tool_help_flag_same_as_help_builtin() {
        let tool = build_test_tool();
        let help_output = tool
            .execute(ToolRequest {
                commands: "help get_user".to_string(),
                timeout_ms: None,
            })
            .await;
        let flag_output = tool
            .execute(ToolRequest {
                commands: "get_user --help".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(
            help_output.stdout, flag_output.stdout,
            "`--help` should produce same output as `help <tool>`"
        );
    }

    #[tokio::test]
    async fn test_tool_help_flag_stripped_from_args() {
        let tool = build_test_tool();
        // get_user --help --id 42 should not call the callback with --help in args
        let resp = tool
            .execute(ToolRequest {
                commands: "get_user --help --id 42".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        // Output should be help text, not the callback result
        assert!(resp.stdout.contains("Fetch user by id"));
        assert!(
            !resp.stdout.contains("Alice"),
            "callback should NOT be invoked"
        );
    }

    // -- Issue #1279: --dry-run flag tests --

    #[tokio::test]
    async fn test_dry_run_validates_args() {
        let tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "get_user --dry-run --id 42".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        let parsed: serde_json::Value =
            serde_json::from_str(resp.stdout.trim()).expect("stdout should be valid JSON");
        assert_eq!(parsed["dry_run"], true);
        assert_eq!(parsed["valid"], true);
        assert_eq!(parsed["params"]["id"], 42);
    }

    #[tokio::test]
    async fn test_dry_run_does_not_invoke_callback() {
        let tool = build_test_tool();
        // fail_tool always errors, but --dry-run should succeed
        let resp = tool
            .execute(ToolRequest {
                commands: "fail_tool --dry-run".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(
            resp.exit_code, 0,
            "--dry-run should not invoke the callback"
        );
    }

    #[tokio::test]
    async fn test_dry_run_help_precedence() {
        let tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "get_user --help --dry-run".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        // Should return help text, not dry-run JSON
        assert!(
            resp.stdout.contains("Fetch user by id"),
            "should show help text"
        );
        assert!(
            !resp.stdout.contains("dry_run"),
            "should NOT show dry-run JSON"
        );
    }

    #[tokio::test]
    async fn test_custom_dry_run_handler() {
        let tool = ScriptedTool::builder("dr_test")
            .tool_with_dry_run(
                ToolDef::new("check", "Validate input").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": { "id": {"type": "integer"} }
                })),
                |args: &ToolArgs| {
                    let id = args.param_i64("id").ok_or("missing --id")?;
                    Ok(format!("executed {id}\n"))
                },
                |args: &ToolArgs| {
                    let id = args.param_i64("id").ok_or("missing --id")?;
                    Ok(format!("custom-dry-run id={id}\n"))
                },
            )
            .build();

        let resp = tool
            .execute(ToolRequest {
                commands: "check --dry-run --id 7".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "custom-dry-run id=7");
    }

    #[tokio::test]
    async fn test_help_flag_returns_help() {
        let tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "get_user --help".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(
            resp.stdout.contains("get_user"),
            "help should include tool name"
        );
        assert!(
            resp.stdout.contains("Fetch user by id"),
            "help should include description"
        );
        assert!(
            resp.stdout.contains("--id"),
            "help should include parameter flags"
        );
    }
}
