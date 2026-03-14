// ScriptingToolSet: higher-level wrapper around ScriptedTool that controls
// system_prompt generation based on DiscoveryMode.
//
// - Exclusive (default): full schemas in prompt, LLM knows everything upfront.
// - WithDiscovery: semantic descriptions only, LLM uses discover/help builtins.

use super::{RegisteredTool, ScriptedTool, ToolArgs, ToolDef};
use crate::ExecutionLimits;
use crate::tool::{Tool, ToolRequest, ToolResponse, ToolStatus, VERSION};
use async_trait::async_trait;
use schemars::schema_for;
use std::sync::Arc;

// ============================================================================
// DiscoveryMode
// ============================================================================

/// Controls how system_prompt() exposes tool information to the LLM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiscoveryMode {
    /// Full schemas in system_prompt. LLM sees all tool names, params, types.
    #[default]
    Exclusive,
    /// Semantic descriptions only. LLM uses `discover` and `help` builtins.
    WithDiscovery,
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
///     .tool(
///         ToolDef::new("ping", "Ping a host"),
///         |_args: &ToolArgs| Ok("pong\n".to_string()),
///     )
///     .build();
/// ```
pub struct ScriptingToolSetBuilder {
    name: String,
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
            short_desc: None,
            tools: Vec::new(),
            limits: None,
            env_vars: Vec::new(),
            mode: DiscoveryMode::Exclusive,
        }
    }

    /// One-line description for tool listings.
    pub fn short_description(mut self, desc: impl Into<String>) -> Self {
        self.short_desc = Some(desc.into());
        self
    }

    /// Register a tool with its definition and execution callback.
    pub fn tool(
        mut self,
        def: ToolDef,
        callback: impl Fn(&ToolArgs) -> Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        self.tools.push(RegisteredTool {
            def,
            callback: Arc::new(callback),
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

    /// Switch to discovery mode: semantic-only prompt, LLM uses discover/help.
    pub fn with_discovery(mut self) -> Self {
        self.mode = DiscoveryMode::WithDiscovery;
        self
    }

    /// Build the [`ScriptingToolSet`].
    pub fn build(self) -> ScriptingToolSet {
        let short_desc = self
            .short_desc
            .unwrap_or_else(|| format!("ScriptingToolSet: {}", self.name));

        // Inner ScriptedTool uses compact_prompt in discovery mode
        let compact = self.mode == DiscoveryMode::WithDiscovery;

        let mut builder = ScriptedTool::builder(&self.name);
        builder = builder.short_description(&short_desc);
        builder = builder.compact_prompt(compact);

        if let Some(limits) = self.limits {
            builder = builder.limits(limits);
        }
        for (key, value) in self.env_vars {
            builder = builder.env(key, value);
        }

        // Move tools into inner ScriptedTool
        // We need to reconstruct because ScriptedToolBuilder expects closures
        for reg in self.tools {
            let cb = reg.callback;
            builder = builder.tool(reg.def, move |args: &ToolArgs| (cb)(args));
        }

        ScriptingToolSet {
            name: self.name,
            short_desc,
            inner: builder.build(),
            mode: self.mode,
        }
    }
}

// ============================================================================
// ScriptingToolSet
// ============================================================================

/// Higher-level wrapper around [`ScriptedTool`] with mode-controlled prompts.
///
/// Two modes control how `system_prompt()` exposes tools:
///
/// - **Exclusive** (default): Full tool schemas in the prompt. Best when this is
///   the only tool the LLM has — it knows everything upfront.
///
/// - **WithDiscovery**: Semantic descriptions only. The prompt tells the LLM to
///   use `discover` and `help` builtins to find tools and their schemas. Best when
///   this tool is alongside other discovery tools, or when the tool set is large.
///
/// ```rust
/// use bashkit::{ScriptingToolSet, ToolArgs, ToolDef, Tool, ToolRequest};
///
/// # tokio_test::block_on(async {
/// // Exclusive mode (default): full schemas in prompt
/// let mut toolset = ScriptingToolSet::builder("api")
///     .tool(
///         ToolDef::new("greet", "Greet someone")
///             .with_schema(serde_json::json!({
///                 "type": "object",
///                 "properties": { "name": {"type": "string"} }
///             })),
///         |args: &ToolArgs| Ok(format!("hello {}\n", args.param_str("name").unwrap_or("world"))),
///     )
///     .build();
///
/// let prompt = toolset.system_prompt();
/// assert!(prompt.contains("Usage:"));
///
/// let resp = toolset.execute(ToolRequest {
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
/// // Discovery mode: semantic-only prompt
/// let toolset = ScriptingToolSet::builder("api")
///     .tool(
///         ToolDef::new("greet", "Greet someone"),
///         |_args: &ToolArgs| Ok("hello\n".to_string()),
///     )
///     .with_discovery()
///     .build();
///
/// let prompt = toolset.system_prompt();
/// assert!(prompt.contains("discover"));
/// assert!(!prompt.contains("Usage:"));
/// ```
pub struct ScriptingToolSet {
    name: String,
    short_desc: String,
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

    /// Build discovery-mode system prompt: semantic descriptions + discover/help instructions.
    fn build_discovery_prompt(&self) -> String {
        let mut prompt = format!("# {}\n\n", self.name);
        prompt.push_str(&format!("{}\n\n", self.short_desc));

        prompt.push_str("Input: {\"commands\": \"<bash script>\"}\n");
        prompt.push_str("Output: {stdout, stderr, exit_code}\n\n");

        prompt.push_str("## Tools\n\n");
        prompt.push_str(
            "This tool provides commands you can compose via bash scripts. \
             Use discovery to find available commands:\n\n",
        );
        prompt.push_str("- `discover --categories` — list tool categories\n");
        prompt.push_str("- `discover --category <name>` — list tools in a category\n");
        prompt.push_str("- `discover --search <keyword>` — search tools by name or description\n");
        prompt.push_str("- `help <tool>` — show usage and parameters\n");
        prompt.push_str("- `help <tool> --json` — machine-readable schema\n");

        prompt.push_str(
            "\n## Tips\n\n\
             - Pass arguments as `--key value` or `--key=value` flags\n\
             - Pipe tool output through `jq` for JSON processing\n\
             - Use variables to pass data between tool calls\n\
             - Use `set -e` to stop on first error\n\
             - Standard builtins (echo, grep, sed, awk, etc.) are available\n",
        );

        prompt
    }
}

#[async_trait]
impl Tool for ScriptingToolSet {
    fn name(&self) -> &str {
        &self.name
    }

    fn short_description(&self) -> &str {
        &self.short_desc
    }

    fn description(&self) -> String {
        self.inner.description()
    }

    fn help(&self) -> String {
        self.inner.help()
    }

    fn system_prompt(&self) -> String {
        match self.mode {
            DiscoveryMode::Exclusive => self.inner.system_prompt(),
            DiscoveryMode::WithDiscovery => self.build_discovery_prompt(),
        }
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
        self.inner.execute(req).await
    }

    async fn execute_with_status(
        &mut self,
        req: ToolRequest,
        status_callback: Box<dyn FnMut(ToolStatus) + Send>,
    ) -> ToolResponse {
        self.inner.execute_with_status(req, status_callback).await
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
            .tool(
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
            .tool(
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

    // -- Exclusive mode prompt --

    #[test]
    fn test_exclusive_mode_has_full_schemas() {
        let toolset = make_tools().build();
        let sp = toolset.system_prompt();
        assert!(
            sp.contains("Usage: `get_user --id <integer>`"),
            "prompt: {sp}"
        );
        assert!(
            sp.contains("Usage: `list_orders --user_id <integer>`"),
            "prompt: {sp}"
        );
    }

    #[test]
    fn test_exclusive_mode_no_discover_instructions() {
        let toolset = make_tools().build();
        let sp = toolset.system_prompt();
        assert!(!sp.contains("discover --categories"), "prompt: {sp}");
        assert!(!sp.contains("discover --search"), "prompt: {sp}");
    }

    // -- Discovery mode prompt --

    #[test]
    fn test_discovery_mode_semantic_only() {
        let toolset = make_tools().with_discovery().build();
        let sp = toolset.system_prompt();
        assert!(sp.contains("discover --categories"), "prompt: {sp}");
        assert!(sp.contains("discover --search"), "prompt: {sp}");
        assert!(sp.contains("help <tool>"), "prompt: {sp}");
    }

    #[test]
    fn test_discovery_mode_no_usage_lines() {
        let toolset = make_tools().with_discovery().build();
        let sp = toolset.system_prompt();
        assert!(!sp.contains("Usage: `get_user"), "prompt: {sp}");
        assert!(!sp.contains("Usage: `list_orders"), "prompt: {sp}");
    }

    // -- Name / description --

    #[test]
    fn test_name_and_short_description() {
        let toolset = make_tools().build();
        assert_eq!(toolset.name(), "test_api");
        assert_eq!(toolset.short_description(), "Test API");
    }

    #[test]
    fn test_default_short_description() {
        let toolset = ScriptingToolSet::builder("mytools")
            .tool(ToolDef::new("noop", "No-op"), |_: &ToolArgs| {
                Ok("ok\n".into())
            })
            .build();
        assert_eq!(toolset.short_description(), "ScriptingToolSet: mytools");
    }

    // -- Execution delegates to inner --

    #[tokio::test]
    async fn test_execute_delegates_to_inner() {
        let mut toolset = make_tools().build();
        let resp = toolset
            .execute(ToolRequest {
                commands: "get_user --id 42 | jq -r '.name'".into(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "Alice");
    }

    #[tokio::test]
    async fn test_execute_discovery_mode_also_works() {
        let mut toolset = make_tools().with_discovery().build();
        let resp = toolset
            .execute(ToolRequest {
                commands: "get_user --id 1".into(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("Alice"));
    }

    #[tokio::test]
    async fn test_execute_with_status_delegates() {
        use std::sync::{Arc, Mutex};

        let mut toolset = make_tools().build();
        let phases = Arc::new(Mutex::new(Vec::new()));
        let phases_clone = phases.clone();

        let resp = toolset
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
        let mut toolset = make_tools().build();
        let resp = toolset
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
    async fn test_help_builtin_works_in_discovery() {
        let mut toolset = make_tools().with_discovery().build();
        let resp = toolset
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
        let mut toolset = make_tools().build();
        let resp = toolset
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
    async fn test_discover_builtin_works_in_discovery() {
        let mut toolset = make_tools().with_discovery().build();
        let resp = toolset
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
        let mut toolset = ScriptingToolSet::builder("env_test")
            .env("MY_VAR", "hello")
            .tool(ToolDef::new("noop", "No-op"), |_: &ToolArgs| {
                Ok("ok\n".into())
            })
            .build();

        let resp = toolset
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
        assert_eq!(toolset.version(), VERSION);
    }

    #[test]
    fn test_schemas() {
        let toolset = make_tools().build();
        let input = toolset.input_schema();
        assert!(input["properties"]["commands"].is_object());
        let output = toolset.output_schema();
        assert!(output["properties"]["stdout"].is_object());
    }
}
