//! Tool contract and `BashTool` implementation.
//!
//! # Public Library Contract
//!
//! `bashkit` follows the Everruns toolkit-library contract:
//!
//! ```text
//! ToolBuilder (config) -> Tool (metadata) -> ToolExecution (single-use runtime)
//! ```
//!
//! [`BashToolBuilder`] configures a reusable tool definition. [`BashTool`] exposes
//! locale-aware metadata plus [`Tool::execution`] for validated, single-use runs.
//! [`ToolExecution`] returns structured [`ToolOutput`] and can optionally stream
//! [`ToolOutputChunk`] values during execution.
//!
//! # Architecture
//!
//! - [`Tool`] trait: shared metadata + execution contract
//! - [`BashToolBuilder`]: reusable builder for config, schemas, OpenAI tool JSON,
//!   and `tower::Service` integration
//! - [`BashTool`]: immutable metadata object implementing [`Tool`]
//! - [`ToolExecution`]: validated, single-use runtime for one call
//!
//! # Builder Example
//!
//! ```
//! use bashkit::{BashTool, Tool};
//!
//! let builder = BashTool::builder()
//!     .locale("en-US")
//!     .username("agent")
//!     .hostname("sandbox");
//!
//! let tool = builder.build();
//! assert_eq!(tool.name(), "bashkit");
//! assert_eq!(tool.display_name(), "Bash");
//! assert!(builder.build_tool_definition()["function"]["parameters"].is_object());
//! ```
//!
//! # Execution Example
//!
//! ```
//! use bashkit::{BashTool, Tool};
//! use futures::StreamExt;
//!
//! # tokio_test::block_on(async {
//! let tool = BashTool::default();
//! let execution = tool
//!     .execution(serde_json::json!({"commands": "printf 'a\nb\n'"}))
//!     .expect("valid args");
//! let mut stream = execution.output_stream().expect("stream available");
//!
//! let handle = tokio::spawn(async move { execution.execute().await.expect("execution succeeds") });
//! let first = stream.next().await.expect("first chunk");
//! assert_eq!(first.kind, "stdout");
//! assert!(first.data.as_str().is_some_and(|chunk| chunk.starts_with("a\n")));
//!
//! let output = handle.await.expect("join");
//! assert_eq!(output.result["stdout"], "a\nb\n");
//! # });
//! ```

use crate::builtins::Builtin;
use crate::error::Error;
use crate::{Bash, ExecResult, ExecutionLimits, OutputCallback};
use async_trait::async_trait;
use futures::Stream;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
type ToolExecutionFuture = Pin<Box<dyn Future<Output = Result<ToolOutput, ToolError>> + Send>>;
type ToolExecutionRunner = Box<
    dyn FnOnce(Option<tokio::sync::mpsc::UnboundedSender<ToolOutputChunk>>) -> ToolExecutionFuture
        + Send,
>;

/// Library version from Cargo.toml
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Standard `tower::Service` type for toolkit integrations.
pub type ToolService =
    tower::util::BoxCloneService<serde_json::Value, serde_json::Value, ToolError>;

/// Tool execution error.
///
/// The split between [`ToolError::UserFacing`] and [`ToolError::Internal`] lets
/// consumers decide what is safe to send back to the LLM. User-facing errors
/// should be short, actionable, and locale-aware. Internal errors are for logs.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// Safe to show to the LLM/user.
    #[error("{0}")]
    UserFacing(String),
    /// Internal failure for logs/diagnostics only.
    #[error("{0}")]
    Internal(String),
}

impl ToolError {
    /// Whether the message is safe to show to the LLM.
    pub fn is_user_facing(&self) -> bool {
        matches!(self, Self::UserFacing(_))
    }
}

/// Image payload returned by a tool.
///
/// `bashkit` does not currently emit images, but the contract keeps parity with
/// other toolkit crates that may return screenshots or rendered assets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolImage {
    pub base64: String,
    pub media_type: String,
}

/// Consumer-facing metadata that never goes to the LLM.
///
/// Use [`ToolOutputMetadata::extra`] for kit-specific diagnostics such as exit
/// codes, command counts, or bytes transferred.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOutputMetadata {
    #[serde(with = "duration_millis")]
    pub duration: Duration,
    pub extra: serde_json::Value,
}

/// Structured execution result.
///
/// [`ToolOutput::result`] is the JSON payload intended for the LLM tool result.
/// [`ToolOutput::metadata`] is reserved for the host application.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOutput {
    pub result: serde_json::Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ToolImage>,
    pub metadata: ToolOutputMetadata,
}

/// Incremental tool output chunk.
///
/// `kind` is consumer-routable (`stdout`, `stderr`, `progress`, ...). `data`
/// stays JSON so non-text chunks can be added later without changing the type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOutputChunk {
    pub data: serde_json::Value,
    pub kind: String,
}

/// Stream returned by [`ToolExecution::output_stream`].
///
/// This stream is informational. The final authoritative result still comes
/// from [`ToolExecution::execute`].
pub struct ToolOutputStream {
    receiver: tokio::sync::mpsc::UnboundedReceiver<ToolOutputChunk>,
}

impl Stream for ToolOutputStream {
    type Item = ToolOutputChunk;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

#[derive(Default)]
struct ToolExecutionStreamState {
    sender: Option<tokio::sync::mpsc::UnboundedSender<ToolOutputChunk>>,
    receiver: Option<tokio::sync::mpsc::UnboundedReceiver<ToolOutputChunk>>,
}

/// Stateful, single-use tool execution.
///
/// Build one with [`Tool::execution`]. Call [`ToolExecution::output_stream`]
/// before [`ToolExecution::execute`] if you need live updates.
pub struct ToolExecution {
    runner: Option<ToolExecutionRunner>,
    stream_state: Arc<Mutex<ToolExecutionStreamState>>,
}

impl ToolExecution {
    pub(crate) fn new<F, Fut>(runner: F) -> Self
    where
        F: FnOnce(Option<tokio::sync::mpsc::UnboundedSender<ToolOutputChunk>>) -> Fut
            + Send
            + 'static,
        Fut: Future<Output = Result<ToolOutput, ToolError>> + Send + 'static,
    {
        Self {
            runner: Some(Box::new(move |sender| Box::pin(runner(sender)))),
            stream_state: Arc::new(Mutex::new(ToolExecutionStreamState::default())),
        }
    }

    /// Stream incremental output. Must be called before [`Self::execute`].
    pub fn output_stream(&self) -> Option<ToolOutputStream> {
        let mut state = match self.stream_state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if state.receiver.is_none() {
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            state.sender = Some(sender);
            state.receiver = Some(receiver);
        }
        state
            .receiver
            .take()
            .map(|receiver| ToolOutputStream { receiver })
    }

    /// Run the execution to completion.
    pub async fn execute(mut self) -> Result<ToolOutput, ToolError> {
        let sender = match self.stream_state.lock() {
            Ok(state) => state.sender.clone(),
            Err(poisoned) => poisoned.into_inner().sender.clone(),
        };
        let Some(runner) = self.runner.take() else {
            return Err(ToolError::Internal(
                "tool execution may only be run once".to_string(),
            ));
        };
        runner(sender).await
    }
}

mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(value.as_millis() as u64)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

/// List of built-in commands (organized by category)
const BUILTINS: &str = "\
echo printf cat read \
grep sed awk jq head tail sort uniq cut tr wc nl paste column comm diff strings tac rev \
cd pwd ls find mkdir mktemp rm rmdir cp mv touch chmod chown ln \
file stat less tar gzip gunzip du df \
test [ true false exit return break continue \
export set unset local shift source eval declare typeset readonly shopt getopts \
sleep date seq expr yes wait timeout xargs tee watch \
basename dirname realpath \
pushd popd dirs \
whoami hostname uname id env printenv history \
curl wget \
od xxd hexdump base64 \
kill";

/// Request to execute bash commands
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolRequest {
    /// Bash commands to execute (like `bash -c "commands"`)
    pub commands: String,
    /// Optional per-call timeout in milliseconds.
    /// When set, execution is aborted after this duration and a response
    /// with `exit_code = 124` is returned (matching the bash `timeout` convention).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl ToolRequest {
    /// Create a request with just commands (no timeout).
    pub fn new(commands: impl Into<String>) -> Self {
        Self {
            commands: commands.into(),
            timeout_ms: None,
        }
    }
}

/// Response from executing a bash script
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolResponse {
    /// Standard output from the script
    pub stdout: String,
    /// Standard error from the script
    pub stderr: String,
    /// Exit code (0 = success)
    pub exit_code: i32,
    /// Error message if execution failed before running
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl From<ExecResult> for ToolResponse {
    fn from(result: ExecResult) -> Self {
        Self {
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code,
            error: None,
        }
    }
}

/// Status update during tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStatus {
    /// Current phase (e.g., "validate", "parse", "execute", "output", "complete")
    pub phase: String,
    /// Optional message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Estimated completion percentage (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent_complete: Option<f32>,
    /// Estimated time remaining in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_ms: Option<u64>,
    /// Incremental stdout/stderr chunk (only present when `phase == "output"`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Which stream the output belongs to: `"stdout"` or `"stderr"`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
}

impl ToolStatus {
    /// Create a new status with phase
    pub fn new(phase: impl Into<String>) -> Self {
        Self {
            phase: phase.into(),
            message: None,
            percent_complete: None,
            eta_ms: None,
            output: None,
            stream: None,
        }
    }

    /// Create an output status carrying a stdout chunk.
    pub fn stdout(chunk: impl Into<String>) -> Self {
        Self {
            phase: "output".to_string(),
            message: None,
            percent_complete: None,
            eta_ms: None,
            output: Some(chunk.into()),
            stream: Some("stdout".to_string()),
        }
    }

    /// Create an output status carrying a stderr chunk.
    pub fn stderr(chunk: impl Into<String>) -> Self {
        Self {
            phase: "output".to_string(),
            message: None,
            percent_complete: None,
            eta_ms: None,
            output: Some(chunk.into()),
            stream: Some("stderr".to_string()),
        }
    }

    /// Set message
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Set completion percentage
    pub fn with_percent(mut self, percent: f32) -> Self {
        self.percent_complete = Some(percent);
        self
    }

    /// Set ETA
    pub fn with_eta(mut self, eta_ms: u64) -> Self {
        self.eta_ms = Some(eta_ms);
        self
    }
}

// ============================================================================
// Tool Trait - Public Library Contract
// ============================================================================

/// Tool contract for LLM integration.
///
/// # Public Contract
///
/// This trait is a **public library contract**. Breaking changes require a major version bump.
/// See `specs/009-tool-contract.md` for the full specification.
///
/// All tools must implement this trait to be usable by LLMs and agents.
/// The trait provides introspection (schemas, docs) and execution methods.
///
/// # Implementors
///
/// - [`BashTool`]: Virtual bash interpreter
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool identifier (e.g., "bashkit", "calculator")
    fn name(&self) -> &str;

    /// Human-readable display name for UI.
    fn display_name(&self) -> &str;

    /// One-line description for tool listings
    fn short_description(&self) -> &str;

    /// Token-efficient description for LLMs.
    fn description(&self) -> &str;

    /// Full documentation for LLMs (markdown, with examples)
    fn help(&self) -> String;

    /// Condensed description for system prompts (token-efficient)
    fn system_prompt(&self) -> String;

    /// Locale used for user-facing text.
    fn locale(&self) -> &str;

    /// JSON Schema for input validation
    fn input_schema(&self) -> serde_json::Value;

    /// JSON Schema for output structure
    fn output_schema(&self) -> serde_json::Value;

    /// Library/tool version
    fn version(&self) -> &str;

    /// Create a single-use execution.
    fn execution(&self, args: serde_json::Value) -> Result<ToolExecution, ToolError>;

    /// Execute the tool
    async fn execute(&self, req: ToolRequest) -> ToolResponse;

    /// Execute with status callbacks for progress tracking
    async fn execute_with_status(
        &self,
        req: ToolRequest,
        status_callback: Box<dyn FnMut(ToolStatus) + Send>,
    ) -> ToolResponse;
}

// ============================================================================
// BashTool - Implementation
// ============================================================================

/// Builder for configuring BashTool
#[derive(Default)]
pub struct BashToolBuilder {
    /// Locale for user-facing text.
    locale: String,
    /// Custom username for virtual identity
    username: Option<String>,
    /// Custom hostname for virtual identity
    hostname: Option<String>,
    /// Execution limits
    limits: Option<ExecutionLimits>,
    /// Environment variables to set
    env_vars: Vec<(String, String)>,
    /// Custom builtins (name, implementation). Arc enables reuse across create_bash calls.
    builtins: Vec<(String, Arc<dyn Builtin>)>,
}

impl BashToolBuilder {
    /// Create a new tool builder with defaults
    pub fn new() -> Self {
        Self {
            locale: "en-US".to_string(),
            ..Self::default()
        }
    }

    /// Set locale for descriptions, prompts, help, and user-facing errors.
    pub fn locale(mut self, locale: &str) -> Self {
        self.locale = locale.to_string();
        self
    }

    /// Set custom username for virtual identity
    pub fn username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    /// Set custom hostname for virtual identity
    pub fn hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = Some(hostname.into());
        self
    }

    /// Set execution limits
    pub fn limits(mut self, limits: ExecutionLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Add an environment variable
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.push((key.into(), value.into()));
        self
    }

    /// Register a custom builtin command
    ///
    /// Custom builtins extend the shell with domain-specific commands.
    /// They will be documented in the tool's `help()` output.
    /// If the builtin implements [`Builtin::llm_hint`], its hint will be
    /// included in `help()` and `system_prompt()`.
    pub fn builtin(mut self, name: impl Into<String>, builtin: Box<dyn Builtin>) -> Self {
        self.builtins.push((name.into(), Arc::from(builtin)));
        self
    }

    /// Enable embedded Python (`python`/`python3` builtins) via Monty interpreter
    /// with default resource limits.
    ///
    /// Requires the `python` feature flag. Python `pathlib.Path` operations are
    /// bridged to the virtual filesystem. Limitations (no `open()`, no HTTP) are
    /// automatically documented in `help()` and `system_prompt()`.
    #[cfg(feature = "python")]
    pub fn python(self) -> Self {
        self.python_with_limits(crate::builtins::PythonLimits::default())
    }

    /// Enable embedded Python with custom resource limits.
    #[cfg(feature = "python")]
    pub fn python_with_limits(self, limits: crate::builtins::PythonLimits) -> Self {
        use crate::builtins::Python;
        self.builtin("python", Box::new(Python::with_limits(limits.clone())))
            .builtin("python3", Box::new(Python::with_limits(limits)))
    }

    /// Build the BashTool
    pub fn build(&self) -> BashTool {
        let builtin_names: Vec<String> = self.builtins.iter().map(|(n, _)| n.clone()).collect();

        // Collect LLM hints from builtins, deduplicated
        let mut builtin_hints: Vec<String> = self
            .builtins
            .iter()
            .filter_map(|(_, b)| b.llm_hint().map(String::from))
            .collect();
        builtin_hints.sort();
        builtin_hints.dedup();

        let locale = self.locale.clone();
        let display_name = localized(locale.as_str(), "Bash", "Баш");

        BashTool {
            locale,
            display_name: display_name.to_string(),
            short_desc: localized(
                self.locale.as_str(),
                "Run bash commands in an isolated virtual filesystem",
                "Виконує bash-команди в ізольованій віртуальній файловій системі",
            )
            .to_string(),
            description: build_bash_description(self.locale.as_str(), &builtin_names),
            username: self.username.clone(),
            hostname: self.hostname.clone(),
            limits: self.limits.clone(),
            env_vars: self.env_vars.clone(),
            builtins: self.builtins.clone(),
            builtin_names,
            builtin_hints,
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

    /// Build the input schema without constructing a full tool.
    pub fn build_input_schema(&self) -> serde_json::Value {
        let schema = schema_for!(ToolRequest);
        serde_json::to_value(schema).unwrap_or_default()
    }

    /// Build the output schema for `ToolOutput::result`.
    pub fn build_output_schema(&self) -> serde_json::Value {
        let schema = schema_for!(ToolResponse);
        serde_json::to_value(schema).unwrap_or_default()
    }
}

/// Virtual bash interpreter implementing the Tool trait
#[derive(Clone)]
pub struct BashTool {
    locale: String,
    display_name: String,
    short_desc: String,
    description: String,
    username: Option<String>,
    hostname: Option<String>,
    limits: Option<ExecutionLimits>,
    env_vars: Vec<(String, String)>,
    builtins: Vec<(String, Arc<dyn Builtin>)>,
    /// Names of custom builtins (for documentation)
    builtin_names: Vec<String>,
    /// LLM hints from registered builtins
    builtin_hints: Vec<String>,
}

impl BashTool {
    /// Create a new tool builder
    pub fn builder() -> BashToolBuilder {
        BashToolBuilder::new()
    }

    /// Create a Bash instance with configured settings
    fn create_bash(&self) -> Bash {
        let mut builder = Bash::builder();

        if let Some(ref username) = self.username {
            builder = builder.username(username);
        }
        if let Some(ref hostname) = self.hostname {
            builder = builder.hostname(hostname);
        }
        if let Some(ref limits) = self.limits {
            builder = builder.limits(limits.clone());
        }
        for (key, value) in &self.env_vars {
            builder = builder.env(key, value);
        }
        // Clone Arc builtins so they survive across multiple create_bash calls
        for (name, builtin) in &self.builtins {
            builder = builder.builtin(name.clone(), Box::new(Arc::clone(builtin)));
        }

        builder.build()
    }

    /// Build dynamic help with configuration
    fn build_help(&self) -> String {
        build_bash_help(self)
    }

    /// Single-line warning listing language interpreters not registered as builtins.
    /// Returns `None` when all tracked languages are available.
    fn language_warning(&self) -> Option<String> {
        let mut missing = Vec::new();

        let has_perl = self.builtin_names.iter().any(|n| n == "perl");
        if !has_perl {
            missing.push("perl");
        }

        let has_python = self
            .builtin_names
            .iter()
            .any(|n| n == "python" || n == "python3");
        if !has_python {
            missing.push("python/python3");
        }

        if missing.is_empty() {
            None
        } else {
            Some(format!("{} not available.", missing.join(", ")))
        }
    }

    /// Build dynamic system prompt
    fn build_system_prompt(&self) -> String {
        build_bash_system_prompt(self)
    }

    async fn run_request_with_stream(
        &self,
        req: ToolRequest,
        stream_sender: Option<tokio::sync::mpsc::UnboundedSender<ToolOutputChunk>>,
    ) -> ToolResponse {
        if req.commands.is_empty() {
            return ToolResponse {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                error: None,
            };
        }

        let tool = self.clone();
        let mut bash = tool.create_bash();

        let fut = async {
            let result = if let Some(sender) = stream_sender {
                let output_cb: OutputCallback = Box::new(move |stdout_chunk, stderr_chunk| {
                    if !stdout_chunk.is_empty() {
                        let _ = sender.send(ToolOutputChunk {
                            data: serde_json::json!(stdout_chunk),
                            kind: "stdout".to_string(),
                        });
                    }
                    if !stderr_chunk.is_empty() {
                        let _ = sender.send(ToolOutputChunk {
                            data: serde_json::json!(stderr_chunk),
                            kind: "stderr".to_string(),
                        });
                    }
                });
                bash.exec_streaming(&req.commands, output_cb).await
            } else {
                bash.exec(&req.commands).await
            };

            match result {
                Ok(result) => result.into(),
                Err(err) => ToolResponse {
                    stdout: String::new(),
                    stderr: err.to_string(),
                    exit_code: 1,
                    error: Some(error_kind(&err)),
                },
            }
        };

        if let Some(ms) = req.timeout_ms {
            let duration = Duration::from_millis(ms);
            match tokio::time::timeout(duration, fut).await {
                Ok(response) => response,
                Err(_) => timeout_response(duration),
            }
        } else {
            fut.await
        }
    }
}

impl Default for BashTool {
    fn default() -> Self {
        BashToolBuilder::new().build()
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bashkit"
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn short_description(&self) -> &str {
        &self.short_desc
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn help(&self) -> String {
        self.build_help()
    }

    fn system_prompt(&self) -> String {
        self.build_system_prompt()
    }

    fn locale(&self) -> &str {
        &self.locale
    }

    fn input_schema(&self) -> serde_json::Value {
        BashToolBuilder {
            locale: self.locale.clone(),
            username: self.username.clone(),
            hostname: self.hostname.clone(),
            limits: self.limits.clone(),
            env_vars: self.env_vars.clone(),
            builtins: self.builtins.clone(),
        }
        .build_input_schema()
    }

    fn output_schema(&self) -> serde_json::Value {
        BashToolBuilder {
            locale: self.locale.clone(),
            username: self.username.clone(),
            hostname: self.hostname.clone(),
            limits: self.limits.clone(),
            env_vars: self.env_vars.clone(),
            builtins: self.builtins.clone(),
        }
        .build_output_schema()
    }

    fn version(&self) -> &str {
        VERSION
    }

    fn execution(&self, args: serde_json::Value) -> Result<ToolExecution, ToolError> {
        let req = tool_request_from_value(self.locale(), args)?;
        let tool = self.clone();

        Ok(ToolExecution::new(move |stream_sender| async move {
            let start = std::time::Instant::now();
            let response = tool.run_request_with_stream(req, stream_sender).await;
            tool_output_from_response(response, start.elapsed())
        }))
    }

    async fn execute(&self, req: ToolRequest) -> ToolResponse {
        if req.commands.is_empty() {
            return ToolResponse {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                error: None,
            };
        }

        let mut bash = self.create_bash();

        let fut = async {
            match bash.exec(&req.commands).await {
                Ok(result) => result.into(),
                Err(e) => ToolResponse {
                    stdout: String::new(),
                    stderr: e.to_string(),
                    exit_code: 1,
                    error: Some(error_kind(&e)),
                },
            }
        };

        if let Some(ms) = req.timeout_ms {
            let dur = Duration::from_millis(ms);
            match tokio::time::timeout(dur, fut).await {
                Ok(resp) => resp,
                Err(_elapsed) => timeout_response(dur),
            }
        } else {
            fut.await
        }
    }

    async fn execute_with_status(
        &self,
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

        // Wire streaming: forward output chunks as ToolStatus events
        let status_cb = Arc::new(Mutex::new(status_callback));
        let status_cb_output = status_cb.clone();
        let output_cb: OutputCallback = Box::new(move |stdout_chunk, stderr_chunk| {
            if let Ok(mut cb) = status_cb_output.lock() {
                if !stdout_chunk.is_empty() {
                    cb(ToolStatus::stdout(stdout_chunk));
                }
                if !stderr_chunk.is_empty() {
                    cb(ToolStatus::stderr(stderr_chunk));
                }
            }
        });

        let timeout_ms = req.timeout_ms;

        let fut = async {
            let response = match bash.exec_streaming(&req.commands, output_cb).await {
                Ok(result) => result.into(),
                Err(e) => ToolResponse {
                    stdout: String::new(),
                    stderr: e.to_string(),
                    exit_code: 1,
                    error: Some(error_kind(&e)),
                },
            };

            if let Ok(mut cb) = status_cb.lock() {
                cb(ToolStatus::new("complete").with_percent(100.0));
            }

            response
        };

        if let Some(ms) = timeout_ms {
            let dur = Duration::from_millis(ms);
            match tokio::time::timeout(dur, fut).await {
                Ok(resp) => resp,
                Err(_elapsed) => timeout_response(dur),
            }
        } else {
            fut.await
        }
    }
}

/// Extract error kind from Error for categorization
fn error_kind(e: &Error) -> String {
    match e {
        Error::Parse(_) | Error::ParseAt { .. } => "parse_error".to_string(),
        Error::Execution(_) => "execution_error".to_string(),
        Error::Io(_) => "io_error".to_string(),
        Error::CommandNotFound(_) => "command_not_found".to_string(),
        Error::ResourceLimit(_) => "resource_limit".to_string(),
        Error::Network(_) => "network_error".to_string(),
        Error::Regex(_) => "regex_error".to_string(),
        Error::Internal(_) => "internal_error".to_string(),
        Error::Cancelled => "cancelled".to_string(),
    }
}

/// Build a ToolResponse for a timed-out execution (exit code 124, like bash `timeout`).
fn timeout_response(dur: Duration) -> ToolResponse {
    ToolResponse {
        stdout: String::new(),
        stderr: format!(
            "bashkit: execution timed out after {:.1}s\n",
            dur.as_secs_f64()
        ),
        exit_code: 124,
        error: Some("timeout".to_string()),
    }
}

pub(crate) fn localized<'a>(locale: &str, en: &'a str, uk: &'a str) -> &'a str {
    if locale.starts_with("uk") { uk } else { en }
}

fn build_bash_description(locale: &str, builtin_names: &[String]) -> String {
    let mut desc = localized(
        locale,
        "Run bash commands in an isolated virtual filesystem",
        "Виконує bash-команди в ізольованій віртуальній файловій системі",
    )
    .to_string();
    if !builtin_names.is_empty() {
        desc.push_str(". ");
        desc.push_str(localized(
            locale,
            "Custom commands",
            "Користувацькі команди",
        ));
        desc.push_str(": ");
        desc.push_str(&builtin_names.join(", "));
    }
    desc
}

fn build_bash_system_prompt(tool: &BashTool) -> String {
    let mut parts = vec![format!(
        "{}: {}.",
        tool.name(),
        localized(
            tool.locale(),
            "run bash commands in an isolated virtual filesystem",
            "виконує bash-команди в ізольованій віртуальній файловій системі",
        )
    )];

    parts.push(
        localized(
            tool.locale(),
            "Returns JSON with stdout, stderr, exit_code.",
            "Повертає JSON з stdout, stderr, exit_code.",
        )
        .to_string(),
    );

    if let Some(username) = &tool.username {
        parts.push(format!(
            "{} /home/{username}.",
            localized(tool.locale(), "Home", "Домівка")
        ));
    }

    if !tool.builtin_hints.is_empty() {
        parts.extend(tool.builtin_hints.iter().cloned());
    }

    if let Some(warning) = tool.language_warning() {
        parts.push(warning);
    }

    parts.join(" ")
}

fn build_bash_help(tool: &BashTool) -> String {
    let mut doc = String::new();
    doc.push_str(&format!("# {}\n\n", tool.display_name()));
    doc.push_str(tool.description());
    doc.push_str(".\n\n");
    doc.push_str(&format!(
        "**Version:** {}\n**Name:** `{}`\n**Locale:** `{}`\n\n",
        tool.version(),
        tool.name(),
        tool.locale()
    ));

    doc.push_str("## Parameters\n\n");
    doc.push_str("| Name | Type | Required | Default | Description |\n");
    doc.push_str("|------|------|----------|---------|-------------|\n");
    doc.push_str("| `commands` | string | yes | — | Bash commands to execute |\n");
    doc.push_str("| `timeout_ms` | integer | no | — | Per-call timeout in milliseconds |\n\n");

    doc.push_str("## Result\n\n");
    doc.push_str("| Field | Type | Description |\n");
    doc.push_str("|------|------|-------------|\n");
    doc.push_str("| `stdout` | string | Standard output |\n");
    doc.push_str("| `stderr` | string | Standard error |\n");
    doc.push_str("| `exit_code` | integer | Shell exit code |\n");
    doc.push_str("| `error` | string | Error category when execution fails |\n\n");

    doc.push_str("## Examples\n\n");
    doc.push_str("```json\n");
    doc.push_str("{\"commands\":\"echo hello\"}\n");
    doc.push_str("```\n\n");

    doc.push_str("```json\n");
    doc.push_str(
        "{\"commands\":\"echo data > /tmp/f.txt && cat /tmp/f.txt\",\"timeout_ms\":5000}\n",
    );
    doc.push_str("```\n\n");

    doc.push_str("## Behavior\n\n");
    doc.push_str("- Filesystem is virtual and isolated per execution.\n");
    doc.push_str("- Standard bash syntax is supported, including pipes, redirects, loops, functions, and arrays.\n");
    doc.push_str("- Builtins available by default: `");
    doc.push_str(BUILTINS);
    doc.push_str("`\n");
    if !tool.builtin_names.is_empty() {
        doc.push_str("- Custom commands: `");
        doc.push_str(&tool.builtin_names.join("`, `"));
        doc.push_str("`\n");
    }
    if let Some(username) = &tool.username {
        doc.push_str(&format!("- User: `{username}`\n"));
    }
    if let Some(hostname) = &tool.hostname {
        doc.push_str(&format!("- Host: `{hostname}`\n"));
    }
    if let Some(limits) = &tool.limits {
        doc.push_str(&format!(
            "- Limits: {} commands, {} loop iterations, {} function depth\n",
            limits.max_commands, limits.max_loop_iterations, limits.max_function_depth
        ));
    }
    if !tool.env_vars.is_empty() {
        let env_keys: Vec<&str> = tool.env_vars.iter().map(|(key, _)| key.as_str()).collect();
        doc.push_str("- Environment variables: `");
        doc.push_str(&env_keys.join("`, `"));
        doc.push_str("`\n");
    }
    if !tool.builtin_hints.is_empty() {
        doc.push_str("\n## Notes\n\n");
        for hint in &tool.builtin_hints {
            doc.push_str("- ");
            doc.push_str(hint);
            doc.push('\n');
        }
    }
    if let Some(warning) = tool.language_warning() {
        doc.push_str("\n## Warnings\n\n");
        doc.push_str("- ");
        doc.push_str(&warning);
        doc.push('\n');
    }

    doc
}

pub(crate) fn tool_request_from_value(
    locale: &str,
    args: serde_json::Value,
) -> Result<ToolRequest, ToolError> {
    let Some(obj) = args.as_object() else {
        return Err(ToolError::UserFacing(
            localized(
                locale,
                "tool arguments must be a JSON object",
                "аргументи інструмента мають бути JSON-об'єктом",
            )
            .to_string(),
        ));
    };

    let Some(commands) = obj.get("commands").and_then(|value| value.as_str()) else {
        return Err(ToolError::UserFacing(
            localized(
                locale,
                "`commands` is required",
                "поле `commands` є обов'язковим",
            )
            .to_string(),
        ));
    };

    let timeout_ms = match obj.get("timeout_ms") {
        Some(value) => Some(value.as_u64().ok_or_else(|| {
            ToolError::UserFacing(
                localized(
                    locale,
                    "`timeout_ms` must be an integer",
                    "поле `timeout_ms` має бути цілим числом",
                )
                .to_string(),
            )
        })?),
        None => None,
    };

    Ok(ToolRequest {
        commands: commands.to_string(),
        timeout_ms,
    })
}

pub(crate) fn tool_output_from_response(
    response: ToolResponse,
    duration: Duration,
) -> Result<ToolOutput, ToolError> {
    let exit_code = response.exit_code;
    let result = serde_json::to_value(response)
        .map_err(|err| ToolError::Internal(format!("failed to serialize tool response: {err}")))?;
    Ok(ToolOutput {
        result,
        images: Vec::new(),
        metadata: ToolOutputMetadata {
            duration,
            extra: serde_json::json!({ "exit_code": exit_code }),
        },
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_bash_tool_builder() {
        let tool = BashTool::builder()
            .username("testuser")
            .hostname("testhost")
            .env("FOO", "bar")
            .limits(ExecutionLimits::new().max_commands(100))
            .build();

        assert_eq!(tool.username, Some("testuser".to_string()));
        assert_eq!(tool.hostname, Some("testhost".to_string()));
        assert_eq!(tool.env_vars, vec![("FOO".to_string(), "bar".to_string())]);
    }

    #[test]
    fn test_tool_trait_methods() {
        let tool = BashTool::default();

        // Test trait methods
        assert_eq!(tool.name(), "bashkit");
        assert_eq!(tool.display_name(), "Bash");
        assert_eq!(
            tool.short_description(),
            "Run bash commands in an isolated virtual filesystem"
        );
        assert_eq!(
            tool.description(),
            "Run bash commands in an isolated virtual filesystem"
        );
        assert_eq!(tool.locale(), "en-US");
        assert!(tool.help().contains("# Bash"));
        assert!(tool.help().contains("## Parameters"));
        assert!(tool.system_prompt().starts_with("bashkit:"));
        assert_eq!(tool.version(), VERSION);
    }

    #[test]
    fn test_tool_description_with_config() {
        let tool = BashTool::builder()
            .username("agent")
            .hostname("sandbox")
            .env("API_KEY", "secret")
            .limits(ExecutionLimits::new().max_commands(50))
            .build();

        // helptext should include configuration in markdown
        let helptext = tool.help();
        assert!(helptext.contains("User: `agent`"));
        assert!(helptext.contains("Host: `sandbox`"));
        assert!(helptext.contains("50 commands"));
        assert!(helptext.contains("API_KEY"));

        // system_prompt should include home
        let sysprompt = tool.system_prompt();
        assert!(sysprompt.starts_with("bashkit:"));
        assert!(sysprompt.contains("Home /home/agent."));
    }

    #[test]
    fn test_tool_schemas() {
        let tool = BashTool::default();
        let input_schema = tool.input_schema();
        let output_schema = tool.output_schema();

        // Input schema should have commands property
        assert!(input_schema["properties"]["commands"].is_object());

        // Output schema should have stdout, stderr, exit_code
        assert!(output_schema["properties"]["stdout"].is_object());
        assert!(output_schema["properties"]["stderr"].is_object());
        assert!(output_schema["properties"]["exit_code"].is_object());
    }

    #[test]
    fn test_builder_contract_helpers() {
        let builder = BashTool::builder().username("agent");
        let definition = builder.build_tool_definition();
        let input_schema = builder.build_input_schema();
        let output_schema = builder.build_output_schema();

        assert_eq!(definition["type"], "function");
        assert_eq!(definition["function"]["name"], "bashkit");
        assert_eq!(definition["function"]["parameters"], input_schema);
        assert!(output_schema["properties"]["stdout"].is_object());
    }

    #[tokio::test]
    async fn test_builder_service_executes() {
        use tower::ServiceExt;

        let service = BashTool::builder().build_service();
        let result = service
            .oneshot(serde_json::json!({"commands": "echo hello"}))
            .await
            .unwrap_or_else(|err| panic!("service should execute: {err}"));

        assert_eq!(result["stdout"], "hello\n");
        assert_eq!(result["exit_code"], 0);
    }

    #[test]
    fn test_execution_rejects_invalid_args() {
        let tool = BashTool::default();
        let err = tool
            .execution(serde_json::json!({"timeout_ms": 10}))
            .err()
            .unwrap_or_else(|| panic!("execution should reject missing commands"));
        assert_eq!(
            err,
            ToolError::UserFacing("`commands` is required".to_string())
        );
    }

    #[tokio::test]
    async fn test_execution_returns_tool_output() {
        let tool = BashTool::default();
        let execution = tool
            .execution(serde_json::json!({"commands": "echo hello"}))
            .unwrap_or_else(|err| panic!("execution should be created: {err}"));
        let output = execution
            .execute()
            .await
            .unwrap_or_else(|err| panic!("execution should succeed: {err}"));

        assert_eq!(output.result["stdout"], "hello\n");
        assert_eq!(output.metadata.extra["exit_code"], 0);
        assert!(output.metadata.duration >= Duration::from_millis(0));
    }

    #[tokio::test]
    async fn test_execution_stream_emits_output_chunks() {
        use futures::StreamExt;

        let tool = BashTool::default();
        let execution = tool
            .execution(serde_json::json!({"commands": "for i in 1 2; do echo $i; done"}))
            .unwrap_or_else(|err| panic!("execution should be created: {err}"));
        let mut stream = execution
            .output_stream()
            .unwrap_or_else(|| panic!("stream should be available"));

        let handle = tokio::spawn(async move {
            execution
                .execute()
                .await
                .unwrap_or_else(|err| panic!("execution should succeed: {err}"))
        });

        let mut chunks = Vec::new();
        while let Some(chunk) = stream.next().await {
            chunks.push(chunk);
        }

        let output = handle
            .await
            .unwrap_or_else(|err| panic!("join should succeed: {err}"));

        assert_eq!(output.result["stdout"], "1\n2\n");
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].kind, "stdout");
        assert_eq!(chunks[0].data, serde_json::json!("1\n"));
    }

    #[test]
    fn test_locale_localizes_user_facing_text() {
        let tool = BashTool::builder().locale("uk-UA").build();
        assert_eq!(tool.display_name(), "Баш");
        assert_eq!(
            tool.description(),
            "Виконує bash-команди в ізольованій віртуальній файловій системі"
        );
        assert!(tool.system_prompt().starts_with("bashkit:"));
    }

    #[test]
    fn test_tool_status() {
        let status = ToolStatus::new("execute")
            .with_message("Running commands")
            .with_percent(50.0)
            .with_eta(5000);

        assert_eq!(status.phase, "execute");
        assert_eq!(status.message, Some("Running commands".to_string()));
        assert_eq!(status.percent_complete, Some(50.0));
        assert_eq!(status.eta_ms, Some(5000));
    }

    #[tokio::test]
    async fn test_tool_execute_empty() {
        let tool = BashTool::default();
        let req = ToolRequest {
            commands: String::new(),
            timeout_ms: None,
        };
        let resp = tool.execute(req).await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_execute_echo() {
        let tool = BashTool::default();
        let req = ToolRequest {
            commands: "echo hello".to_string(),
            timeout_ms: None,
        };
        let resp = tool.execute(req).await;
        assert_eq!(resp.stdout, "hello\n");
        assert_eq!(resp.exit_code, 0);
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_builtin_hints_in_help_and_system_prompt() {
        use crate::builtins::Builtin;
        use crate::error::Result;
        use crate::interpreter::ExecResult;

        struct HintedBuiltin;

        #[async_trait]
        impl Builtin for HintedBuiltin {
            async fn execute(&self, _ctx: crate::builtins::Context<'_>) -> Result<ExecResult> {
                Ok(ExecResult::ok(String::new()))
            }
            fn llm_hint(&self) -> Option<&'static str> {
                Some("mycommand: Processes CSV. Max 10MB. No streaming.")
            }
        }

        let tool = BashTool::builder()
            .builtin("mycommand", Box::new(HintedBuiltin))
            .build();

        // Hint should appear in help
        let helptext = tool.help();
        assert!(
            helptext.contains("## Notes"),
            "help should have Notes section"
        );
        assert!(
            helptext.contains("mycommand: Processes CSV"),
            "help should contain the hint"
        );

        // Hint should appear in system_prompt
        let sysprompt = tool.system_prompt();
        assert!(
            sysprompt.contains("mycommand: Processes CSV"),
            "system_prompt should contain the hint"
        );
    }

    #[test]
    fn test_no_hints_without_hinted_builtins() {
        let tool = BashTool::default();

        let helptext = tool.help();
        assert!(
            !helptext.contains("## Notes"),
            "help should not have Notes without hinted builtins"
        );

        let sysprompt = tool.system_prompt();
        assert!(
            !sysprompt.contains("Processes CSV"),
            "system_prompt should not have hints without hinted builtins"
        );
    }

    #[test]
    fn test_language_warning_default() {
        let tool = BashTool::default();

        let sysprompt = tool.system_prompt();
        assert!(
            sysprompt.contains("perl, python/python3 not available."),
            "system_prompt should have single combined warning"
        );

        let helptext = tool.help();
        assert!(
            helptext.contains("## Warnings"),
            "help should have Warnings section"
        );
        assert!(
            helptext.contains("perl, python/python3 not available."),
            "help should have single combined warning"
        );
    }

    #[test]
    fn test_language_warning_suppressed_by_custom_builtins() {
        use crate::builtins::Builtin;
        use crate::error::Result;
        use crate::interpreter::ExecResult;

        struct NoopBuiltin;

        #[async_trait]
        impl Builtin for NoopBuiltin {
            async fn execute(&self, _ctx: crate::builtins::Context<'_>) -> Result<ExecResult> {
                Ok(ExecResult::ok(String::new()))
            }
        }

        let tool = BashTool::builder()
            .builtin("python", Box::new(NoopBuiltin))
            .builtin("perl", Box::new(NoopBuiltin))
            .build();

        let sysprompt = tool.system_prompt();
        assert!(
            !sysprompt.contains("not available"),
            "no warning when all languages registered"
        );

        let helptext = tool.help();
        assert!(
            !helptext.contains("## Warnings"),
            "no Warnings section when all languages registered"
        );
    }

    #[test]
    fn test_language_warning_partial() {
        use crate::builtins::Builtin;
        use crate::error::Result;
        use crate::interpreter::ExecResult;

        struct NoopBuiltin;

        #[async_trait]
        impl Builtin for NoopBuiltin {
            async fn execute(&self, _ctx: crate::builtins::Context<'_>) -> Result<ExecResult> {
                Ok(ExecResult::ok(String::new()))
            }
        }

        // python3 registered -> only perl warned
        let tool = BashTool::builder()
            .builtin("python3", Box::new(NoopBuiltin))
            .build();

        let sysprompt = tool.system_prompt();
        assert!(
            sysprompt.contains("perl not available."),
            "should warn about perl only"
        );
        assert!(
            !sysprompt.contains("python/python3"),
            "python warning suppressed when python3 registered"
        );
    }

    #[test]
    fn test_duplicate_hints_deduplicated() {
        use crate::builtins::Builtin;
        use crate::error::Result;
        use crate::interpreter::ExecResult;

        struct SameHint;

        #[async_trait]
        impl Builtin for SameHint {
            async fn execute(&self, _ctx: crate::builtins::Context<'_>) -> Result<ExecResult> {
                Ok(ExecResult::ok(String::new()))
            }
            fn llm_hint(&self) -> Option<&'static str> {
                Some("same hint")
            }
        }

        let tool = BashTool::builder()
            .builtin("cmd1", Box::new(SameHint))
            .builtin("cmd2", Box::new(SameHint))
            .build();

        let helptext = tool.help();
        // Should appear exactly once
        assert_eq!(
            helptext.matches("same hint").count(),
            1,
            "Duplicate hints should be deduplicated"
        );
    }

    #[cfg(feature = "python")]
    #[test]
    fn test_python_hint_via_builder() {
        let tool = BashTool::builder().python().build();

        let helptext = tool.help();
        assert!(helptext.contains("python"), "help should mention python");
        assert!(
            helptext.contains("no open()"),
            "help should document open() limitation"
        );
        assert!(
            helptext.contains("No HTTP"),
            "help should document HTTP limitation"
        );

        let sysprompt = tool.system_prompt();
        assert!(
            sysprompt.contains("python"),
            "system_prompt should mention python"
        );

        // Python warning should be suppressed when python is enabled via Monty
        assert!(
            !sysprompt.contains("python/python3 not available"),
            "python warning should not appear when Monty python enabled"
        );
    }

    #[tokio::test]
    async fn test_tool_execute_with_status() {
        use std::sync::{Arc, Mutex};

        let tool = BashTool::default();
        let req = ToolRequest {
            commands: "echo test".to_string(),
            timeout_ms: None,
        };

        let phases = Arc::new(Mutex::new(Vec::new()));
        let phases_clone = phases.clone();

        let resp = tool
            .execute_with_status(
                req,
                Box::new(move |status| {
                    phases_clone
                        .lock()
                        .expect("lock poisoned")
                        .push(status.phase.clone());
                }),
            )
            .await;

        assert_eq!(resp.stdout, "test\n");
        let phases = phases.lock().expect("lock poisoned");
        assert!(phases.contains(&"validate".to_string()));
        assert!(phases.contains(&"complete".to_string()));
    }

    #[tokio::test]
    async fn test_execute_with_status_streams_output() {
        let tool = BashTool::default();
        let req = ToolRequest {
            commands: "for i in a b c; do echo $i; done".to_string(),
            timeout_ms: None,
        };

        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();

        let resp = tool
            .execute_with_status(
                req,
                Box::new(move |status| {
                    events_clone.lock().expect("lock poisoned").push(status);
                }),
            )
            .await;

        assert_eq!(resp.stdout, "a\nb\nc\n");
        assert_eq!(resp.exit_code, 0);

        let events = events.lock().expect("lock poisoned");
        // Should have output events for each iteration
        let output_events: Vec<_> = events.iter().filter(|s| s.phase == "output").collect();
        assert_eq!(
            output_events.len(),
            3,
            "expected 3 output events, got {output_events:?}"
        );
        assert_eq!(output_events[0].output.as_deref(), Some("a\n"));
        assert_eq!(output_events[0].stream.as_deref(), Some("stdout"));
        assert_eq!(output_events[1].output.as_deref(), Some("b\n"));
        assert_eq!(output_events[2].output.as_deref(), Some("c\n"));
    }

    #[tokio::test]
    async fn test_execute_with_status_streams_list_commands() {
        let tool = BashTool::default();
        let req = ToolRequest {
            commands: "echo start; echo end".to_string(),
            timeout_ms: None,
        };

        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();

        let resp = tool
            .execute_with_status(
                req,
                Box::new(move |status| {
                    events_clone.lock().expect("lock poisoned").push(status);
                }),
            )
            .await;

        assert_eq!(resp.stdout, "start\nend\n");

        let events = events.lock().expect("lock poisoned");
        let output_events: Vec<_> = events.iter().filter(|s| s.phase == "output").collect();
        assert_eq!(
            output_events.len(),
            2,
            "expected 2 output events, got {output_events:?}"
        );
        assert_eq!(output_events[0].output.as_deref(), Some("start\n"));
        assert_eq!(output_events[1].output.as_deref(), Some("end\n"));
    }

    #[tokio::test]
    async fn test_execute_with_status_no_duplicate_output() {
        let tool = BashTool::default();
        // mix of list + loop: should get 5 distinct events, no duplicates
        let req = ToolRequest {
            commands: "echo start; for i in 1 2 3; do echo $i; done; echo end".to_string(),
            timeout_ms: None,
        };

        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();

        let resp = tool
            .execute_with_status(
                req,
                Box::new(move |status| {
                    events_clone.lock().expect("lock poisoned").push(status);
                }),
            )
            .await;

        assert_eq!(resp.stdout, "start\n1\n2\n3\nend\n");

        let events = events.lock().expect("lock poisoned");
        let output_events: Vec<_> = events
            .iter()
            .filter(|s| s.phase == "output")
            .map(|s| s.output.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(
            output_events,
            vec!["start\n", "1\n", "2\n", "3\n", "end\n"],
            "should have exactly 5 distinct output events"
        );
    }

    #[test]
    fn test_tool_status_stdout_constructor() {
        let status = ToolStatus::stdout("hello\n");
        assert_eq!(status.phase, "output");
        assert_eq!(status.output.as_deref(), Some("hello\n"));
        assert_eq!(status.stream.as_deref(), Some("stdout"));
        assert!(status.message.is_none());
    }

    #[test]
    fn test_tool_status_stderr_constructor() {
        let status = ToolStatus::stderr("error\n");
        assert_eq!(status.phase, "output");
        assert_eq!(status.output.as_deref(), Some("error\n"));
        assert_eq!(status.stream.as_deref(), Some("stderr"));
    }

    #[tokio::test]
    async fn test_tool_execute_timeout() {
        let tool = BashTool::default();
        let req = ToolRequest {
            commands: "sleep 10".to_string(),
            timeout_ms: Some(100),
        };
        let resp = tool.execute(req).await;
        assert_eq!(resp.exit_code, 124);
        assert!(resp.stderr.contains("timed out"));
        assert_eq!(resp.error, Some("timeout".to_string()));
    }

    #[tokio::test]
    async fn test_tool_execute_no_timeout() {
        let tool = BashTool::default();
        let req = ToolRequest {
            commands: "echo fast".to_string(),
            timeout_ms: Some(5000),
        };
        let resp = tool.execute(req).await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout, "fast\n");
    }

    #[test]
    fn test_tool_request_new() {
        let req = ToolRequest::new("echo test");
        assert_eq!(req.commands, "echo test");
        assert_eq!(req.timeout_ms, None);
    }

    #[test]
    fn test_tool_request_deserialize_without_timeout() {
        let json = r#"{"commands":"echo hello"}"#;
        let req: ToolRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.commands, "echo hello");
        assert_eq!(req.timeout_ms, None);
    }

    #[test]
    fn test_tool_request_deserialize_with_timeout() {
        let json = r#"{"commands":"echo hello","timeout_ms":5000}"#;
        let req: ToolRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.commands, "echo hello");
        assert_eq!(req.timeout_ms, Some(5000));
    }

    // Issue #422: create_bash should not empty builtins after first call
    #[tokio::test]
    async fn test_create_bash_preserves_builtins() {
        use crate::ExecResult;
        use crate::builtins::{Builtin, Context};
        use async_trait::async_trait;

        struct TestBuiltin;

        #[async_trait]
        impl Builtin for TestBuiltin {
            async fn execute(&self, _ctx: Context<'_>) -> crate::Result<ExecResult> {
                Ok(ExecResult::ok("test_output\n"))
            }
        }

        let tool = BashToolBuilder::new()
            .builtin("testcmd", Box::new(TestBuiltin))
            .build();

        // First call
        let mut bash1 = tool.create_bash();
        let result1 = bash1.exec("testcmd").await.unwrap();
        assert!(
            result1.stdout.contains("test_output"),
            "first call should have custom builtin"
        );

        // Second call should still have the builtin
        let mut bash2 = tool.create_bash();
        let result2 = bash2.exec("testcmd").await.unwrap();
        assert!(
            result2.stdout.contains("test_output"),
            "second call should still have custom builtin"
        );
    }
}
