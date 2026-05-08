// ToolDefExtension centralizes ToolDef-backed builtins so plain Bash and
// ScriptedTool share the same command, help, discover, dry-run, and trace behavior.

use super::{
    CallbackKind, RegisteredTool, ScriptedCommandInvocation, ScriptedCommandKind, ToolArgs,
    ToolDef, ToolImpl,
};
use crate::builtins::{Builtin, Context, Extension};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::tool_def::{parse_flags, usage_from_schema};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Mutex};

pub(crate) type InvocationLog = Arc<Mutex<VecDeque<ScriptedCommandInvocation>>>;
const MAX_LOG_ENTRIES: usize = 256;
const MAX_LOG_ARG_BYTES: usize = 1024;

fn push_invocation(
    log: &InvocationLog,
    name: &str,
    kind: ScriptedCommandKind,
    args: &[String],
    exit_code: i32,
) {
    let args = truncate_args(args);
    let mut invocations = log.lock().expect("tool-def invocation log poisoned");
    if invocations.len() == MAX_LOG_ENTRIES {
        invocations.pop_front();
    }
    invocations.push_back(ScriptedCommandInvocation {
        name: name.to_string(),
        kind,
        args,
        exit_code,
    });
}

fn truncate_args(args: &[String]) -> Vec<String> {
    args.iter().map(|arg| truncate_arg(arg)).collect()
}

fn truncate_arg(arg: &str) -> String {
    if arg.len() <= MAX_LOG_ARG_BYTES {
        return arg.to_string();
    }
    // Byte-aware truncation that respects UTF-8 char boundaries.
    let cut = arg
        .char_indices()
        .map(|(i, c)| i + c.len_utf8())
        .take_while(|&end| end <= MAX_LOG_ARG_BYTES)
        .last()
        .unwrap_or(0);
    arg[..cut].to_string()
}

/// Builder for [`ToolDefExtension`].
pub struct ToolDefExtensionBuilder {
    tools: Vec<RegisteredTool>,
    sanitize_errors: bool,
}

impl Default for ToolDefExtensionBuilder {
    fn default() -> Self {
        Self {
            tools: Vec::new(),
            sanitize_errors: true,
        }
    }
}

impl ToolDefExtensionBuilder {
    /// Register a [`ToolImpl`] (definition + exec functions).
    pub fn tool(mut self, tool: ToolImpl) -> Self {
        self.tools.push(RegisteredTool::from_tool_impl(tool));
        self
    }

    /// Register a tool with its definition and synchronous exec function.
    pub fn tool_fn(
        mut self,
        def: ToolDef,
        exec: impl Fn(&ToolArgs) -> std::result::Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        self.tools.push(RegisteredTool {
            def,
            callback: CallbackKind::Sync(Arc::new(exec)),
            dry_run: None,
        });
        self
    }

    /// Register a sync tool plus a custom `--dry-run` handler.
    pub fn tool_with_dry_run(
        mut self,
        def: ToolDef,
        exec: impl Fn(&ToolArgs) -> std::result::Result<String, String> + Send + Sync + 'static,
        dry_run: impl Fn(&ToolArgs) -> std::result::Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        self.tools.push(RegisteredTool {
            def,
            callback: CallbackKind::Sync(Arc::new(exec)),
            dry_run: Some(CallbackKind::Sync(Arc::new(dry_run))),
        });
        self
    }

    /// Register a tool with its definition and async exec function.
    pub fn async_tool_fn<F, Fut>(mut self, def: ToolDef, exec: F) -> Self
    where
        F: Fn(ToolArgs) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<String, String>> + Send + 'static,
    {
        self.tools.push(RegisteredTool {
            def,
            callback: CallbackKind::Async(Arc::new(move |args| Box::pin(exec(args)))),
            dry_run: None,
        });
        self
    }

    /// Replace callback errors with generic messages before exposing them to scripts.
    pub fn sanitize_errors(mut self, sanitize: bool) -> Self {
        self.sanitize_errors = sanitize;
        self
    }

    /// Build the extension.
    ///
    /// Each call mints a fresh, isolated invocation log. Clones of the
    /// returned extension share the log with the original — keep a clone
    /// before passing the extension to a `Bash` if you intend to call
    /// [`ToolDefExtension::take_invocations`] later.
    pub fn build(&self) -> ToolDefExtension {
        ToolDefExtension {
            tools: self.tools.clone(),
            sanitize_errors: self.sanitize_errors,
            invocation_log: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

/// Bash extension that registers ToolDef-backed commands plus `help` and `discover`.
///
/// Each [`ToolDefExtensionBuilder::build`] mints a fresh invocation log, so
/// distinct builds (e.g. per tenant) never share traces. Cloning shares the
/// log with the original — that is the supported pattern for retaining a
/// `take_invocations` handle after passing the extension to a `Bash`.
#[derive(Clone)]
pub struct ToolDefExtension {
    tools: Vec<RegisteredTool>,
    sanitize_errors: bool,
    invocation_log: InvocationLog,
}

impl ToolDefExtension {
    /// Create an empty builder.
    pub fn builder() -> ToolDefExtensionBuilder {
        ToolDefExtensionBuilder::default()
    }

    pub(crate) fn from_registered_tools(tools: Vec<RegisteredTool>) -> Self {
        Self {
            tools,
            sanitize_errors: true,
            invocation_log: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub(crate) fn with_invocation_log(mut self, log: InvocationLog) -> Self {
        self.invocation_log = log;
        self
    }

    /// Control whether callback errors are sanitized.
    pub fn sanitize_errors(mut self, sanitize: bool) -> Self {
        self.sanitize_errors = sanitize;
        self
    }

    /// Return and clear accumulated command invocation trace entries.
    pub fn take_invocations(&self) -> Vec<ScriptedCommandInvocation> {
        let mut invocations = self
            .invocation_log
            .lock()
            .expect("tool-def invocation log poisoned");
        std::mem::take(&mut *invocations).into()
    }

    fn snapshots(&self) -> Vec<ToolDefSnapshot> {
        self.tools
            .iter()
            .map(|t| ToolDefSnapshot {
                name: t.def.name.clone(),
                description: t.def.description.clone(),
                input_schema: t.def.input_schema.clone(),
                tags: t.def.tags.clone(),
                category: t.def.category.clone(),
            })
            .collect()
    }
}

impl Extension for ToolDefExtension {
    fn builtins(&self) -> Vec<(String, Box<dyn Builtin>)> {
        let mut builtins: Vec<(String, Box<dyn Builtin>)> = Vec::new();
        for tool in &self.tools {
            let name = tool.def.name.clone();
            builtins.push((
                name.clone(),
                Box::new(ToolBuiltinAdapter {
                    name,
                    description: tool.def.description.clone(),
                    callback: tool.callback.clone(),
                    schema: tool.def.input_schema.clone(),
                    log: Arc::clone(&self.invocation_log),
                    sanitize_errors: self.sanitize_errors,
                    dry_run: tool.dry_run.clone(),
                }),
            ));
        }

        let snapshots = self.snapshots();
        builtins.push((
            "help".to_string(),
            Box::new(HelpBuiltin {
                tools: snapshots.clone(),
                log: Arc::clone(&self.invocation_log),
            }),
        ));
        builtins.push((
            "discover".to_string(),
            Box::new(DiscoverBuiltin {
                tools: snapshots,
                log: Arc::clone(&self.invocation_log),
            }),
        ));
        builtins
    }
}

/// Adapts a [`CallbackKind`] into a [`Builtin`] so the interpreter can execute it.
struct ToolBuiltinAdapter {
    name: String,
    description: String,
    callback: CallbackKind,
    schema: serde_json::Value,
    log: InvocationLog,
    sanitize_errors: bool,
    dry_run: Option<CallbackKind>,
}

impl ToolBuiltinAdapter {
    fn help_text(&self) -> String {
        let mut out = format!("{} - {}\n", self.name, self.description);
        if let Some(usage) = usage_from_schema(&self.schema) {
            out.push_str(&format!("Usage: {} {}\n", self.name, usage));
        }
        out
    }
}

#[async_trait]
impl Builtin for ToolBuiltinAdapter {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.iter().any(|a| a == "--help") {
            let result = ExecResult::ok(self.help_text());
            push_invocation(
                &self.log,
                &self.name,
                ScriptedCommandKind::Help,
                ctx.args,
                result.exit_code,
            );
            return Ok(result);
        }

        if ctx.args.iter().any(|a| a == "--dry-run") {
            let stripped: Vec<String> = ctx
                .args
                .iter()
                .filter(|a| a.as_str() != "--dry-run")
                .cloned()
                .collect();
            let exit_result = match parse_flags(&stripped, &self.schema) {
                Ok(params) => {
                    if let Some(ref dr) = self.dry_run {
                        let tool_args = ToolArgs {
                            params,
                            stdin: ctx.stdin.map(String::from),
                        };
                        let cb_result = match dr {
                            CallbackKind::Sync(cb) => (cb)(&tool_args),
                            CallbackKind::Async(cb) => (cb)(tool_args).await,
                        };
                        match cb_result {
                            Ok(stdout) => ExecResult::ok(stdout),
                            Err(_msg) if self.sanitize_errors => {
                                #[cfg(feature = "tracing")]
                                tracing::debug!(
                                    tool = %self.name,
                                    error = %_msg,
                                    "tool dry-run callback error (sanitized)"
                                );
                                ExecResult::err(format!("{}: callback failed\n", self.name), 1)
                            }
                            Err(msg) => ExecResult::err(msg, 1),
                        }
                    } else {
                        let obj = serde_json::json!({
                            "dry_run": true,
                            "valid": true,
                            "tool": self.name,
                            "params": params,
                        });
                        ExecResult::ok(format!(
                            "{}\n",
                            serde_json::to_string(&obj).unwrap_or_default()
                        ))
                    }
                }
                Err(err) => {
                    let obj = serde_json::json!({
                        "dry_run": true,
                        "valid": false,
                        "tool": self.name,
                        "error": err,
                    });
                    let json = serde_json::to_string(&obj).unwrap_or_default();
                    ExecResult::err(format!("{json}\n"), 1)
                }
            };
            push_invocation(
                &self.log,
                &self.name,
                ScriptedCommandKind::Tool,
                ctx.args,
                exit_result.exit_code,
            );
            return Ok(exit_result);
        }

        let exit_result = match parse_flags(ctx.args, &self.schema) {
            Ok(params) => {
                let tool_args = ToolArgs {
                    params,
                    stdin: ctx.stdin.map(String::from),
                };
                let cb_result = match &self.callback {
                    CallbackKind::Sync(cb) => (cb)(&tool_args),
                    CallbackKind::Async(cb) => (cb)(tool_args).await,
                };
                match cb_result {
                    Ok(stdout) => ExecResult::ok(stdout),
                    Err(_msg) if self.sanitize_errors => {
                        #[cfg(feature = "tracing")]
                        tracing::debug!(
                            tool = %self.name,
                            error = %_msg,
                            "tool callback error (sanitized)"
                        );
                        ExecResult::err(format!("{}: callback failed\n", self.name), 1)
                    }
                    Err(msg) => ExecResult::err(msg, 1),
                }
            }
            Err(msg) => ExecResult::err(msg, 2),
        };

        push_invocation(
            &self.log,
            &self.name,
            ScriptedCommandKind::Tool,
            ctx.args,
            exit_result.exit_code,
        );
        Ok(exit_result)
    }
}

#[derive(Clone)]
struct ToolDefSnapshot {
    name: String,
    description: String,
    input_schema: serde_json::Value,
    tags: Vec<String>,
    category: Option<String>,
}

struct HelpBuiltin {
    tools: Vec<ToolDefSnapshot>,
    log: InvocationLog,
}

#[async_trait]
impl Builtin for HelpBuiltin {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let args = ctx.args;
        let result = if args.is_empty() || (args.len() == 1 && args[0] == "--list") {
            let mut out = String::new();
            for t in &self.tools {
                out.push_str(&format!("{:<20} {}\n", t.name, t.description));
            }
            ExecResult::ok(out)
        } else {
            let tool_name = args.iter().find(|a| !a.starts_with("--"));
            let json_mode = args.iter().any(|a| a == "--json");

            let Some(tool_name) = tool_name else {
                let result =
                    ExecResult::err("usage: help [--list] [<tool>] [--json]".to_string(), 1);
                push_invocation(
                    &self.log,
                    "help",
                    ScriptedCommandKind::Help,
                    args,
                    result.exit_code,
                );
                return Ok(result);
            };

            let Some(tool) = self.tools.iter().find(|t| t.name == *tool_name) else {
                let result = ExecResult::err(format!("help: unknown tool: {tool_name}"), 1);
                push_invocation(
                    &self.log,
                    "help",
                    ScriptedCommandKind::Help,
                    args,
                    result.exit_code,
                );
                return Ok(result);
            };

            if json_mode {
                let obj = serde_json::json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                });
                let json_str = serde_json::to_string_pretty(&obj).unwrap_or_default();
                ExecResult::ok(format!("{json_str}\n"))
            } else {
                let mut out = format!("{} - {}\n", tool.name, tool.description);
                if let Some(usage) = usage_from_schema(&tool.input_schema) {
                    out.push_str(&format!("Usage: {} {}\n", tool.name, usage));
                }
                ExecResult::ok(out)
            }
        };

        push_invocation(
            &self.log,
            "help",
            ScriptedCommandKind::Help,
            args,
            result.exit_code,
        );
        Ok(result)
    }
}

struct DiscoverBuiltin {
    tools: Vec<ToolDefSnapshot>,
    log: InvocationLog,
}

impl DiscoverBuiltin {
    fn filter_tools(&self, args: &[String]) -> Vec<&ToolDefSnapshot> {
        if let Some(pos) = args.iter().position(|a| a == "--category") {
            let cat = args.get(pos + 1).map(|s| s.as_str()).unwrap_or("");
            return self
                .tools
                .iter()
                .filter(|t| t.category.as_deref() == Some(cat))
                .collect();
        }

        if let Some(pos) = args.iter().position(|a| a == "--tag") {
            let tag = args.get(pos + 1).map(|s| s.as_str()).unwrap_or("");
            return self
                .tools
                .iter()
                .filter(|t| t.tags.iter().any(|tg| tg == tag))
                .collect();
        }

        if let Some(pos) = args.iter().position(|a| a == "--search") {
            let keyword = args
                .get(pos + 1)
                .map(|s| s.to_lowercase())
                .unwrap_or_default();
            return self
                .tools
                .iter()
                .filter(|t| {
                    t.name.to_lowercase().contains(&keyword)
                        || t.description.to_lowercase().contains(&keyword)
                })
                .collect();
        }

        self.tools.iter().collect()
    }
}

#[async_trait]
impl Builtin for DiscoverBuiltin {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let args = ctx.args;
        let result = if args.is_empty() {
            ExecResult::err(
                "usage: discover --categories | --category <name> | --tag <tag> | --search <keyword> [--json]".to_string(),
                1,
            )
        } else {
            let json_mode = args.iter().any(|a| a == "--json");

            if args.iter().any(|a| a == "--categories") {
                let mut cats: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                for t in &self.tools {
                    if let Some(ref cat) = t.category {
                        *cats.entry(cat.clone()).or_insert(0) += 1;
                    }
                }
                if json_mode {
                    let arr: Vec<serde_json::Value> = cats
                        .iter()
                        .map(|(name, count)| serde_json::json!({"category": name, "count": count}))
                        .collect();
                    let json_str =
                        serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".to_string());
                    ExecResult::ok(format!("{json_str}\n"))
                } else {
                    let mut out = String::new();
                    for (name, count) in &cats {
                        let plural = if *count == 1 { "tool" } else { "tools" };
                        out.push_str(&format!("{name} ({count} {plural})\n"));
                    }
                    ExecResult::ok(out)
                }
            } else {
                let filtered = self.filter_tools(args);
                if json_mode {
                    let arr: Vec<serde_json::Value> = filtered
                        .iter()
                        .map(|t| {
                            let mut obj = serde_json::json!({
                                "name": t.name,
                                "description": t.description,
                            });
                            if !t.tags.is_empty() {
                                obj["tags"] = serde_json::json!(t.tags);
                            }
                            if let Some(ref cat) = t.category {
                                obj["category"] = serde_json::json!(cat);
                            }
                            obj
                        })
                        .collect();
                    let json_str =
                        serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".to_string());
                    ExecResult::ok(format!("{json_str}\n"))
                } else {
                    let mut out = String::new();
                    for t in &filtered {
                        out.push_str(&format!("{:<20} {}\n", t.name, t.description));
                    }
                    ExecResult::ok(out)
                }
            }
        };

        push_invocation(
            &self.log,
            "discover",
            ScriptedCommandKind::Discover,
            args,
            result.exit_code,
        );
        Ok(result)
    }
}
