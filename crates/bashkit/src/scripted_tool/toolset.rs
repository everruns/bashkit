// ScriptingToolSet: higher-level wrapper around ScriptedTool that controls
// how tools are exposed based on DiscoveryMode.
//
// - Exclusive (default): returns one ScriptedTool with full schemas in prompt.
// - WithDiscovery: returns two tools — ScriptedTool (compact prompt) +
//   DiscoverTool (discover/help only).

use super::{
    CallbackKind, RegisteredTool, ScriptedExecutionTrace, ScriptedTool, ToolArgs, ToolDef, ToolImpl,
};
use crate::ExecutionLimits;
use crate::tool::{Tool, ToolError, ToolRequest, ToolResponse, ToolStatus, VERSION};
use async_trait::async_trait;
use schemars::schema_for;
use std::sync::Arc;

// ============================================================================
// DiscoveryMode
// ============================================================================

/// Controls how [`ScriptingToolSet::tools`] exposes tool information to the LLM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiscoveryMode {
    /// Returns one tool with full schemas in system_prompt.
    #[default]
    Exclusive,
    /// Returns two tools: a compact ScriptedTool + a DiscoverTool for
    /// runtime schema discovery via `discover` and `help` builtins.
    WithDiscovery,
}

// ============================================================================
// DiscoverTool — discovery-only tool for WithDiscovery mode
// ============================================================================

/// A [`Tool`] that exposes only `discover` and `help` builtins for runtime
/// schema discovery. Returned by [`ScriptingToolSet::tools`] in
/// [`DiscoveryMode::WithDiscovery`] mode alongside the main script tool.
///
/// The LLM uses this tool to explore available commands before writing
/// scripts for the main tool.
///
/// ```rust
/// use bashkit::{ScriptingToolSet, ToolArgs, ToolDef, Tool};
///
/// # tokio_test::block_on(async {
/// let toolset = ScriptingToolSet::builder("api")
///     .tool_fn(
///         ToolDef::new("greet", "Greet someone").with_category("social"),
///         |_args: &ToolArgs| Ok("hello\n".to_string()),
///     )
///     .with_discovery()
///     .build();
///
/// let tools = toolset.tools();
/// assert_eq!(tools.len(), 2);
///
/// // Second tool is the DiscoverTool
/// let discover = &tools[1];
/// assert!(discover.name().ends_with("_discover"));
/// # });
/// ```
pub struct DiscoverTool {
    name: String,
    locale: String,
    display_name: String,
    short_desc: String,
    inner: ScriptedTool,
}

const DISCOVER_ALLOWED_COMMANDS: &[&str] = &["discover", "help"];

impl DiscoverTool {
    fn has_forbidden_shell_syntax(input: &str) -> bool {
        input.contains('\n')
            || input.contains('\r')
            || input.contains(';')
            || input.contains('&')
            || input.contains('|')
            || input.contains('`')
            || input.contains("$(")
    }

    /// Reject commands that aren't `discover` or `help`.
    fn validate_commands(commands: &str) -> Result<(), String> {
        if Self::has_forbidden_shell_syntax(commands) {
            return Err("discover tool commands cannot contain shell control characters".to_string());
        }
        let first_word = commands.split_whitespace().next().unwrap_or("");
        if DISCOVER_ALLOWED_COMMANDS.contains(&first_word) {
            Ok(())
        } else {
            Err("discover tool only supports: discover, help".to_string())
        }
    }

    fn reject_response(msg: &str) -> ToolResponse {
        ToolResponse {
            stdout: String::new(),
            stderr: msg.to_string(),
            exit_code: 1,
            error: Some(msg.to_string()),
            ..Default::default()
        }
    }

    /// Translate structured MCP input into a [`ToolRequest`].
    ///
    /// Priority: `commands` (backward compat) > `all` > `query`.
    fn resolve_request(args: &serde_json::Value) -> Result<ToolRequest, String> {
        let obj = args.as_object().ok_or("arguments must be a JSON object")?;

        let timeout_ms = obj.get("timeout_ms").and_then(|v| v.as_u64());

        // Raw commands pass-through (backward compatible)
        if let Some(commands) = obj.get("commands").and_then(|v| v.as_str()) {
            return Ok(ToolRequest {
                commands: commands.to_string(),
                timeout_ms,
            });
        }

        // Structured: all
        if obj.get("all").and_then(|v| v.as_bool()).unwrap_or(false) {
            return Ok(ToolRequest {
                commands: "discover --categories".to_string(),
                timeout_ms,
            });
        }

        // Structured: query
        if let Some(query) = obj.get("query").and_then(|v| v.as_str()) {
            if Self::has_forbidden_shell_syntax(query) {
                return Err("query contains unsupported shell control characters".to_string());
            }
            return Ok(ToolRequest {
                commands: format!("discover --search {query}"),
                timeout_ms,
            });
        }

        Err("one of `commands`, `all`, or `query` is required".to_string())
    }
}

#[async_trait]
impl Tool for DiscoverTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn short_description(&self) -> &str {
        &self.short_desc
    }

    fn description(&self) -> &str {
        &self.short_desc
    }

    fn help(&self) -> String {
        format!(
            "# {}\n\nDiscover available tool commands.\n\n## Commands\n\n- `discover --categories` — list categories\n- `discover --category <name>` — list tools in category\n- `discover --tag <tag>` — filter by tag\n- `discover --search <keyword>` — search by name/description\n- `help --list` — list all tools\n- `help <tool>` — human-readable usage\n- `help <tool> --json` — machine-readable schema\n\nAll commands support `--json` for structured output.\n",
            self.display_name
        )
    }

    fn system_prompt(&self) -> String {
        format!(
            "{}: discover available tool commands. Use `discover --categories`, `discover --search <keyword>`, `help <tool>`, `help <tool> --json`.",
            self.name
        )
    }

    fn locale(&self) -> &str {
        &self.locale
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query to find tools (matched against names, descriptions, and categories)"
                },
                "all": {
                    "type": "boolean",
                    "description": "List all available tools grouped by category. When true, query is ignored."
                },
                "commands": {
                    "type": "string",
                    "description": "Raw bash commands (backward compatible). Takes precedence over query/all."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Optional timeout in milliseconds"
                }
            }
        })
    }

    fn output_schema(&self) -> serde_json::Value {
        let schema = schema_for!(ToolResponse);
        serde_json::to_value(schema).unwrap_or_default()
    }

    fn version(&self) -> &str {
        VERSION
    }

    fn execution(
        &self,
        args: serde_json::Value,
    ) -> Result<crate::tool::ToolExecution, crate::tool::ToolError> {
        let req = Self::resolve_request(&args).map_err(ToolError::UserFacing)?;
        if let Err(msg) = Self::validate_commands(&req.commands) {
            return Err(ToolError::UserFacing(msg));
        }
        self.inner
            .execution(serde_json::to_value(req).unwrap_or_default())
    }

    async fn execute(&self, req: ToolRequest) -> ToolResponse {
        if let Err(msg) = Self::validate_commands(&req.commands) {
            return Self::reject_response(&msg);
        }
        self.inner.execute(req).await
    }

    async fn execute_with_status(
        &self,
        req: ToolRequest,
        status_callback: Box<dyn FnMut(ToolStatus) + Send>,
    ) -> ToolResponse {
        if let Err(msg) = Self::validate_commands(&req.commands) {
            return Self::reject_response(&msg);
        }
        self.inner.execute_with_status(req, status_callback).await
    }
}

// ============================================================================
// ScriptingToolSetBuilder
// ============================================================================

/// Builder for [`ScriptingToolSet`].
///
/// ```rust
/// use bashkit::{ScriptingToolSet, ToolArgs, ToolDef};
///
/// let toolset = ScriptingToolSet::builder("api")
///     .short_description("Example API")
///     .tool_fn(
///         ToolDef::new("ping", "Ping a host"),
///         |_args: &ToolArgs| Ok("pong\n".to_string()),
///     )
///     .build();
/// ```
pub struct ScriptingToolSetBuilder {
    name: String,
    locale: String,
    short_desc: Option<String>,
    tools: Vec<RegisteredTool>,
    limits: Option<ExecutionLimits>,
    env_vars: Vec<(String, String)>,
    mode: DiscoveryMode,
}

impl ScriptingToolSetBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            locale: "en-US".to_string(),
            short_desc: None,
            tools: Vec::new(),
            limits: None,
            env_vars: Vec::new(),
            mode: DiscoveryMode::Exclusive,
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
    pub fn tool(mut self, tool: ToolImpl) -> Self {
        self.tools.push(RegisteredTool::from_tool_impl(tool));
        self
    }

    /// Register a tool with its definition and synchronous exec function.
    ///
    /// Convenience shorthand — constructs a [`ToolImpl`] internally.
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

    /// Register a tool with its definition and **async** exec function.
    ///
    /// Convenience shorthand — constructs a [`ToolImpl`] internally.
    pub fn async_tool_fn<F, Fut>(mut self, def: ToolDef, exec: F) -> Self
    where
        F: Fn(ToolArgs) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        let cb: super::AsyncToolExec = Arc::new(move |args| Box::pin(exec(args)));
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

    /// Switch to discovery mode: returns two tools from [`ScriptingToolSet::tools`].
    pub fn with_discovery(mut self) -> Self {
        self.mode = DiscoveryMode::WithDiscovery;
        self
    }

    /// Build the [`ScriptingToolSet`].
    pub fn build(&self) -> ScriptingToolSet {
        let short_desc = self
            .short_desc
            .clone()
            .unwrap_or_else(|| format!("ScriptingToolSet: {}", self.name));

        // Inner ScriptedTool uses compact_prompt in discovery mode
        let compact = self.mode == DiscoveryMode::WithDiscovery;

        let mut builder = ScriptedTool::builder(&self.name).locale(&self.locale);
        builder = builder.short_description(&short_desc);
        builder = builder.compact_prompt(compact);

        if let Some(limits) = &self.limits {
            builder = builder.limits(limits.clone());
        }
        for (key, value) in &self.env_vars {
            builder = builder.env(key, value);
        }

        // Move tools into inner ScriptedTool
        // We need to reconstruct because ScriptedToolBuilder expects closures
        for reg in &self.tools {
            match &reg.callback {
                CallbackKind::Sync(cb) => {
                    let cb = Arc::clone(cb);
                    builder = builder.tool_fn(reg.def.clone(), move |args: &ToolArgs| (cb)(args));
                }
                CallbackKind::Async(cb) => {
                    let cb = Arc::clone(cb);
                    builder = builder.async_tool_fn(reg.def.clone(), move |args: ToolArgs| {
                        let cb = Arc::clone(&cb);
                        async move { (cb)(args).await }
                    });
                }
            }
        }

        ScriptingToolSet {
            name: self.name.clone(),
            locale: self.locale.clone(),
            inner: builder.build(),
            mode: self.mode,
        }
    }
}

// ============================================================================
// ScriptingToolSet
// ============================================================================

/// Higher-level wrapper around [`ScriptedTool`] with mode-controlled tool exposure.
///
/// Use [`ScriptingToolSet::tools`] to get the tools to register with your LLM:
///
/// - **Exclusive** (default): Returns one tool — a [`ScriptedTool`] with full
///   schemas in its system prompt.
///
/// - **WithDiscovery**: Returns two tools — a [`ScriptedTool`] with compact
///   prompt (no schemas) + a [`DiscoverTool`] for runtime schema discovery
///   via `discover` and `help` builtins.
///
/// ```rust
/// use bashkit::{ScriptingToolSet, ToolArgs, ToolDef, Tool, ToolRequest};
///
/// # tokio_test::block_on(async {
/// // Exclusive mode (default): one tool with full schemas
/// let toolset = ScriptingToolSet::builder("api")
///     .tool_fn(
///         ToolDef::new("greet", "Greet someone")
///             .with_schema(serde_json::json!({
///                 "type": "object",
///                 "properties": { "name": {"type": "string"} }
///             })),
///         |args: &ToolArgs| Ok(format!("hello {}\n", args.param_str("name").unwrap_or("world"))),
///     )
///     .build();
///
/// let tools = toolset.tools();
/// assert_eq!(tools.len(), 1);
/// assert!(tools[0].system_prompt().contains("--name <string>"));
///
/// let resp = tools[0].execute(ToolRequest {
///     commands: "greet --name Alice".into(),
///     timeout_ms: None,
/// }).await;
/// assert_eq!(resp.stdout.trim(), "hello Alice");
/// # });
/// ```
///
/// ```rust
/// use bashkit::{ScriptingToolSet, ToolArgs, ToolDef, Tool};
///
/// // Discovery mode: two tools
/// let toolset = ScriptingToolSet::builder("api")
///     .tool_fn(
///         ToolDef::new("greet", "Greet someone"),
///         |_args: &ToolArgs| Ok("hello\n".to_string()),
///     )
///     .with_discovery()
///     .build();
///
/// let tools = toolset.tools();
/// assert_eq!(tools.len(), 2);
/// assert_eq!(tools[0].name(), "api");
/// assert_eq!(tools[1].name(), "api_discover");
/// ```
pub struct ScriptingToolSet {
    name: String,
    locale: String,
    inner: ScriptedTool,
    mode: DiscoveryMode,
}

impl ScriptingToolSet {
    /// Create a builder with the given tool name.
    pub fn builder(name: impl Into<String>) -> ScriptingToolSetBuilder {
        ScriptingToolSetBuilder::new(name)
    }

    /// Current discovery mode.
    pub fn discovery_mode(&self) -> DiscoveryMode {
        self.mode
    }

    /// Return and clear the trace from the most recent execute call.
    pub fn take_last_execution_trace(&self) -> Option<ScriptedExecutionTrace> {
        self.inner.take_last_execution_trace()
    }

    /// Return the tools to register with the LLM.
    ///
    /// - **Exclusive**: `[ScriptedTool]` — one tool with full schemas in prompt.
    /// - **WithDiscovery**: `[ScriptedTool, DiscoverTool]` — script tool with
    ///   compact prompt + discover tool for runtime schema exploration.
    pub fn tools(&self) -> Vec<Box<dyn Tool>> {
        match self.mode {
            DiscoveryMode::Exclusive => {
                vec![Box::new(self.inner.clone())]
            }
            DiscoveryMode::WithDiscovery => {
                let discover = DiscoverTool {
                    name: format!("{}_discover", self.name),
                    locale: self.locale.clone(),
                    display_name: format!("{} Discover", self.name),
                    short_desc: format!("Discover available {} commands", self.name),
                    inner: self.inner.clone(),
                };
                vec![Box::new(self.inner.clone()), Box::new(discover)]
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolRequest;

    fn make_tools() -> ScriptingToolSetBuilder {
        ScriptingToolSet::builder("test_api")
            .short_description("Test API")
            .tool_fn(
                ToolDef::new("get_user", "Fetch user by ID")
                    .with_schema(serde_json::json!({
                        "type": "object",
                        "properties": {
                            "id": {"type": "integer"}
                        },
                        "required": ["id"]
                    }))
                    .with_category("users"),
                |args: &ToolArgs| {
                    let id = args.param_i64("id").ok_or("missing --id")?;
                    Ok(format!("{{\"id\":{id},\"name\":\"Alice\"}}\n"))
                },
            )
            .tool_fn(
                ToolDef::new("list_orders", "List orders for a user")
                    .with_schema(serde_json::json!({
                        "type": "object",
                        "properties": {
                            "user_id": {"type": "integer"}
                        }
                    }))
                    .with_category("orders"),
                |args: &ToolArgs| {
                    let uid = args.param_i64("user_id").ok_or("missing --user_id")?;
                    Ok(format!("[{{\"order_id\":1,\"user_id\":{uid}}}]\n"))
                },
            )
    }

    // -- Mode defaults --

    #[test]
    fn test_builder_defaults_to_exclusive() {
        let toolset = make_tools().build();
        assert_eq!(toolset.discovery_mode(), DiscoveryMode::Exclusive);
    }

    #[test]
    fn test_with_discovery_switches_mode() {
        let toolset = make_tools().with_discovery().build();
        assert_eq!(toolset.discovery_mode(), DiscoveryMode::WithDiscovery);
    }

    // -- Exclusive mode: tools() returns 1 --

    #[test]
    fn test_exclusive_returns_one_tool() {
        let toolset = make_tools().build();
        let tools = toolset.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "test_api");
    }

    #[test]
    fn test_exclusive_tool_has_full_schemas() {
        let toolset = make_tools().build();
        let tools = toolset.tools();
        let sp = tools[0].system_prompt();
        assert!(sp.contains("get_user [--id <integer>]"), "prompt: {sp}");
        assert!(
            sp.contains("list_orders [--user_id <integer>]"),
            "prompt: {sp}"
        );
    }

    #[test]
    fn test_exclusive_tool_no_discover_instructions() {
        let toolset = make_tools().build();
        let tools = toolset.tools();
        let sp = tools[0].system_prompt();
        assert!(!sp.contains("discover --categories"), "prompt: {sp}");
    }

    // -- Discovery mode: tools() returns 2 --

    #[test]
    fn test_discovery_returns_two_tools() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name(), "test_api");
        assert_eq!(tools[1].name(), "test_api_discover");
    }

    #[test]
    fn test_discovery_script_tool_compact_prompt() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let sp = tools[0].system_prompt();
        // Compact: no schema flags
        assert!(!sp.contains("--id <integer>"), "prompt: {sp}");
        assert!(!sp.contains("--user_id <integer>"), "prompt: {sp}");
    }

    #[test]
    fn test_discovery_discover_tool_prompt() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let sp = tools[1].system_prompt();
        assert!(sp.contains("discover"), "prompt: {sp}");
        assert!(sp.contains("help"), "prompt: {sp}");
    }

    // -- Name / description --

    #[test]
    fn test_name_and_short_description() {
        let toolset = make_tools().build();
        let tools = toolset.tools();
        assert_eq!(tools[0].name(), "test_api");
        assert_eq!(tools[0].short_description(), "Test API");
    }

    #[test]
    fn test_default_short_description() {
        let toolset = ScriptingToolSet::builder("mytools")
            .tool_fn(ToolDef::new("noop", "No-op"), |_: &ToolArgs| {
                Ok("ok\n".into())
            })
            .build();
        let tools = toolset.tools();
        assert_eq!(tools[0].short_description(), "ScriptingToolSet: mytools");
    }

    // -- Execution via tools() --

    #[tokio::test]
    async fn test_execute_via_exclusive_tool() {
        let toolset = make_tools().build();
        let tools = toolset.tools();
        let resp = tools[0]
            .execute(ToolRequest {
                commands: "get_user --id 42 | jq -r '.name'".into(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "Alice");
    }

    #[tokio::test]
    async fn test_execute_via_discovery_script_tool() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let resp = tools[0]
            .execute(ToolRequest {
                commands: "get_user --id 1".into(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("Alice"));
    }

    #[tokio::test]
    async fn test_execute_via_discover_tool() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let resp = tools[1]
            .execute(ToolRequest {
                commands: "discover --categories".into(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("users"));
        assert!(resp.stdout.contains("orders"));
    }

    #[tokio::test]
    async fn test_discover_tool_help_builtin() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let resp = tools[1]
            .execute(ToolRequest {
                commands: "help get_user".into(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("get_user"));
        assert!(resp.stdout.contains("--id"));
    }

    #[tokio::test]
    async fn test_execute_with_status_via_tool() {
        use std::sync::{Arc, Mutex};

        let toolset = make_tools().build();
        let tools = toolset.tools();
        let phases = Arc::new(Mutex::new(Vec::new()));
        let phases_clone = phases.clone();

        let resp = tools[0]
            .execute_with_status(
                ToolRequest {
                    commands: "get_user --id 1".into(),
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
        assert!(phases.contains(&"complete".to_string()));
    }

    // -- help/discover builtins work in both modes --

    #[tokio::test]
    async fn test_help_builtin_works_in_exclusive() {
        let toolset = make_tools().build();
        let tools = toolset.tools();
        let resp = tools[0]
            .execute(ToolRequest {
                commands: "help get_user".into(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("get_user"));
        assert!(resp.stdout.contains("--id"));
    }

    #[tokio::test]
    async fn test_discover_builtin_works_in_exclusive() {
        let toolset = make_tools().build();
        let tools = toolset.tools();
        let resp = tools[0]
            .execute(ToolRequest {
                commands: "discover --categories".into(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("users"));
        assert!(resp.stdout.contains("orders"));
    }

    // -- env vars --

    #[tokio::test]
    async fn test_env_vars_passed_through() {
        let toolset = ScriptingToolSet::builder("env_test")
            .env("MY_VAR", "hello")
            .tool_fn(ToolDef::new("noop", "No-op"), |_: &ToolArgs| {
                Ok("ok\n".into())
            })
            .build();

        let tools = toolset.tools();
        let resp = tools[0]
            .execute(ToolRequest {
                commands: "echo $MY_VAR".into(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "hello");
    }

    // -- version / schemas --

    #[test]
    fn test_version() {
        let toolset = make_tools().build();
        let tools = toolset.tools();
        assert_eq!(tools[0].version(), VERSION);
    }

    #[test]
    fn test_schemas() {
        let toolset = make_tools().build();
        let tools = toolset.tools();
        let input = tools[0].input_schema();
        assert!(input["properties"]["commands"].is_object());
        let output = tools[0].output_schema();
        assert!(output["properties"]["stdout"].is_object());
    }

    #[test]
    fn test_discover_tool_schemas() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let input = tools[1].input_schema();
        assert!(input["properties"]["commands"].is_object());
    }

    // -- DiscoverTool command restriction --

    #[tokio::test]
    async fn test_discover_tool_allows_discover() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let resp = tools[1]
            .execute(ToolRequest {
                commands: "discover --categories".into(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("users"));
    }

    #[tokio::test]
    async fn test_discover_tool_allows_help() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let resp = tools[1]
            .execute(ToolRequest {
                commands: "help get_user".into(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("get_user"));
    }

    #[tokio::test]
    async fn test_discover_tool_rejects_other_commands() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let resp = tools[1]
            .execute(ToolRequest {
                commands: "get_user --id 42".into(),
                timeout_ms: None,
            })
            .await;
        assert_ne!(resp.exit_code, 0);
        assert!(
            resp.error
                .as_deref()
                .unwrap_or("")
                .contains("discover tool only supports"),
            "error: {:?}",
            resp.error
        );
    }

    #[tokio::test]
    async fn test_discover_tool_rejects_arbitrary_bash() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let resp = tools[1]
            .execute(ToolRequest {
                commands: "echo pwned".into(),
                timeout_ms: None,
            })
            .await;
        assert_ne!(resp.exit_code, 0);
        assert!(
            resp.error
                .as_deref()
                .unwrap_or("")
                .contains("discover tool only supports")
        );
    }

    #[test]
    fn test_discover_tool_execution_rejects_other_commands() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let args = serde_json::json!({ "commands": "get_user --id 42" });
        let result = tools[1].execution(args);
        match result {
            Err(e) => assert!(
                e.to_string().contains("discover tool only supports"),
                "unexpected error: {e}"
            ),
            Ok(_) => panic!("expected error for disallowed command"),
        }
    }

    // -- ToolImpl registration --

    #[tokio::test]
    async fn test_tool_impl_registration() {
        let get_user = ToolImpl::new(
            ToolDef::new("get_user", "Fetch user by ID")
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": { "id": {"type": "integer"} },
                    "required": ["id"]
                }))
                .with_category("users"),
        )
        .with_exec_sync(|args| {
            let id = args.param_i64("id").ok_or("missing --id")?;
            Ok(format!("{{\"id\":{id},\"name\":\"Alice\"}}\n"))
        });

        let list_orders = ToolImpl::new(
            ToolDef::new("list_orders", "List orders")
                .with_schema(serde_json::json!({
                    "type": "object",
                    "properties": { "user_id": {"type": "integer"} }
                }))
                .with_category("orders"),
        )
        .with_exec_sync(|args| {
            let uid = args.param_i64("user_id").ok_or("missing --user_id")?;
            Ok(format!("[{{\"order_id\":1,\"user_id\":{uid}}}]\n"))
        });

        // Exclusive mode
        let toolset = ScriptingToolSet::builder("api")
            .short_description("Test API")
            .tool(get_user.clone())
            .tool(list_orders.clone())
            .build();

        let tools = toolset.tools();
        assert_eq!(tools.len(), 1);
        assert!(tools[0].system_prompt().contains("get_user"));
        assert!(tools[0].system_prompt().contains("list_orders"));

        let resp = tools[0]
            .execute(ToolRequest {
                commands: "get_user --id 1 | jq -r '.name'".into(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.stdout.trim(), "Alice");

        // Discovery mode
        let toolset = ScriptingToolSet::builder("api")
            .tool(get_user)
            .tool(list_orders)
            .with_discovery()
            .build();

        let tools = toolset.tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name(), "api");
        assert_eq!(tools[1].name(), "api_discover");
    }

    // -- Structured discover input (MCP) --

    #[tokio::test]
    async fn test_discover_structured_query() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let args = serde_json::json!({ "query": "user" });
        let exec = tools[1].execution(args).expect("valid structured args");
        let output = exec.execute().await.expect("execution succeeds");
        let stdout = output.result["stdout"].as_str().unwrap_or("");
        assert!(stdout.contains("get_user"), "stdout: {stdout}");
    }

    #[tokio::test]
    async fn test_discover_structured_all() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let args = serde_json::json!({ "all": true });
        let exec = tools[1].execution(args).expect("valid structured args");
        let output = exec.execute().await.expect("execution succeeds");
        let stdout = output.result["stdout"].as_str().unwrap_or("");
        assert!(stdout.contains("users"), "stdout: {stdout}");
        assert!(stdout.contains("orders"), "stdout: {stdout}");
    }

    #[tokio::test]
    async fn test_discover_structured_query_rejects_injection() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let args = serde_json::json!({ "query": "user; help get_user --json" });
        match tools[1].execution(args) {
            Err(e) => assert!(
                e.to_string().contains("query contains unsupported shell control characters"),
                "unexpected error: {e}"
            ),
            Ok(_) => panic!("expected injection-like query to be rejected"),
        }
    }

    #[tokio::test]
    async fn test_discover_backward_compat() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let args = serde_json::json!({ "commands": "discover --search user" });
        let exec = tools[1].execution(args).expect("valid commands args");
        let output = exec.execute().await.expect("execution succeeds");
        let stdout = output.result["stdout"].as_str().unwrap_or("");
        assert!(stdout.contains("get_user"), "stdout: {stdout}");
    }

    #[tokio::test]
    async fn test_discover_backward_compat_rejects_shell_separators() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let args = serde_json::json!({ "commands": "discover --search user; help get_user" });
        match tools[1].execution(args) {
            Err(e) => assert!(
                e.to_string()
                    .contains("discover tool commands cannot contain shell control characters"),
                "unexpected error: {e}"
            ),
            Ok(_) => panic!("expected injection-like commands to be rejected"),
        }
    }

    #[test]
    fn test_discover_input_schema_is_structured() {
        let toolset = make_tools().with_discovery().build();
        let tools = toolset.tools();
        let schema = tools[1].input_schema();
        let props = &schema["properties"];
        assert!(props["query"].is_object(), "missing query property");
        assert!(props["all"].is_object(), "missing all property");
        assert!(props["commands"].is_object(), "missing commands property");
        assert!(
            props["timeout_ms"].is_object(),
            "missing timeout_ms property"
        );
    }
}
