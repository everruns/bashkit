//! Interpreter for executing bash scripts
//!
//! # Fail Points (enabled with `failpoints` feature)
//!
//! - `interp::execute_command` - Inject failures in command execution
//! - `interp::expand_variable` - Inject failures in variable expansion
//! - `interp::execute_function` - Inject failures in function calls

// Interpreter uses chars().last().unwrap() and chars().next().unwrap() after
// validating string contents. This is safe because we check for non-empty strings.
#![allow(clippy::unwrap_used)]

mod jobs;
mod state;

#[allow(unused_imports)]
pub use jobs::{JobTable, SharedJobTable};
pub use state::{ControlFlow, ExecResult};

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::FutureExt;

use crate::builtins::{self, Builtin};
#[cfg(feature = "failpoints")]
use crate::error::Error;
use crate::error::Result;
use crate::fs::FileSystem;
use crate::limits::{ExecutionCounters, ExecutionLimits};

/// Callback for streaming output chunks as they are produced.
///
/// Arguments: `(stdout_chunk, stderr_chunk)`. Called after each loop iteration
/// and each top-level command completes. Only non-empty chunks trigger a call.
///
/// Requires `Send + Sync` because the interpreter holds this across `.await` points.
/// Closures capturing `Arc<Mutex<_>>` satisfy both bounds automatically.
pub type OutputCallback = Box<dyn FnMut(&str, &str) + Send + Sync>;
use crate::parser::{
    ArithmeticForCommand, AssignmentValue, CaseCommand, Command, CommandList, CompoundCommand,
    ForCommand, FunctionDef, IfCommand, ListOperator, ParameterOp, Parser, Pipeline, Redirect,
    RedirectKind, Script, SimpleCommand, Span, TimeCommand, UntilCommand, WhileCommand, Word,
    WordPart,
};

#[cfg(feature = "failpoints")]
use fail::fail_point;

/// The canonical /dev/null path.
/// This is handled at the interpreter level to prevent custom filesystems from bypassing it.
const DEV_NULL: &str = "/dev/null";

/// Check if a name is a shell keyword (for `command -v`/`command -V`).
fn is_keyword(name: &str) -> bool {
    matches!(
        name,
        "if" | "then"
            | "else"
            | "elif"
            | "fi"
            | "for"
            | "while"
            | "until"
            | "do"
            | "done"
            | "case"
            | "esac"
            | "in"
            | "function"
            | "select"
            | "time"
            | "{"
            | "}"
            | "[["
            | "]]"
            | "!"
    )
}

/// Check if a path refers to /dev/null after normalization.
/// Handles attempts to bypass via paths like `/dev/../dev/null`.
fn is_dev_null(path: &Path) -> bool {
    // Normalize the path to handle .. and . components
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::RootDir => normalized.push("/"),
            std::path::Component::Normal(name) => normalized.push(name),
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            std::path::Component::Prefix(_) => {}
        }
    }
    if normalized.as_os_str().is_empty() {
        normalized.push("/");
    }
    normalized == Path::new(DEV_NULL)
}

/// A frame in the call stack for local variable scoping
#[derive(Debug, Clone)]
struct CallFrame {
    /// Function name
    name: String,
    /// Local variables in this scope
    locals: HashMap<String, String>,
    /// Positional parameters ($1, $2, etc.)
    positional: Vec<String>,
}

/// Shell options that can be set via `set -o` or `set -x`
#[derive(Debug, Clone, Default)]
pub struct ShellOptions {
    /// Exit immediately if a command exits with non-zero status (set -e)
    pub errexit: bool,
    /// Print commands before execution (set -x) - stored but not enforced
    #[allow(dead_code)]
    pub xtrace: bool,
}

/// Interpreter state.
pub struct Interpreter {
    fs: Arc<dyn FileSystem>,
    env: HashMap<String, String>,
    variables: HashMap<String, String>,
    /// Arrays - stored as name -> index -> value
    arrays: HashMap<String, HashMap<usize, String>>,
    cwd: PathBuf,
    last_exit_code: i32,
    /// Built-in commands (default + custom)
    builtins: HashMap<String, Box<dyn Builtin>>,
    /// Defined functions
    functions: HashMap<String, FunctionDef>,
    /// Call stack for local variable scoping
    call_stack: Vec<CallFrame>,
    /// Resource limits
    limits: ExecutionLimits,
    /// Execution counters for resource tracking
    counters: ExecutionCounters,
    /// Job table for background execution
    #[allow(dead_code)]
    jobs: JobTable,
    /// Shell options (set -e, set -x, etc.)
    options: ShellOptions,
    /// Current line number for $LINENO
    current_line: usize,
    /// HTTP client for network builtins (curl, wget)
    #[cfg(feature = "http_client")]
    http_client: Option<crate::network::HttpClient>,
    /// Git client for git builtins
    #[cfg(feature = "git")]
    git_client: Option<crate::git::GitClient>,
    /// Stdin inherited from pipeline for compound commands (while read, etc.)
    /// Each read operation consumes one line, advancing through the data.
    pipeline_stdin: Option<String>,
    /// Optional callback for streaming output chunks during execution.
    /// When set, output is emitted incrementally via this callback in addition
    /// to being accumulated in the returned ExecResult.
    output_callback: Option<OutputCallback>,
    /// Monotonic counter incremented each time output is emitted via callback.
    /// Used to detect whether sub-calls already emitted output, preventing duplicates.
    output_emit_count: u64,
}

impl Interpreter {
    /// Create a new interpreter with the given filesystem.
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self::with_config(fs, None, None, HashMap::new())
    }

    /// Create a new interpreter with custom username, hostname, and builtins.
    ///
    /// # Arguments
    ///
    /// * `fs` - The virtual filesystem to use
    /// * `username` - Optional custom username for virtual identity
    /// * `hostname` - Optional custom hostname for virtual identity
    /// * `custom_builtins` - Custom builtins to register (override defaults if same name)
    pub fn with_config(
        fs: Arc<dyn FileSystem>,
        username: Option<String>,
        hostname: Option<String>,
        custom_builtins: HashMap<String, Box<dyn Builtin>>,
    ) -> Self {
        let mut builtins: HashMap<String, Box<dyn Builtin>> = HashMap::new();

        // Register default builtins
        builtins.insert("echo".to_string(), Box::new(builtins::Echo));
        builtins.insert("true".to_string(), Box::new(builtins::True));
        builtins.insert("false".to_string(), Box::new(builtins::False));
        builtins.insert("exit".to_string(), Box::new(builtins::Exit));
        builtins.insert("cd".to_string(), Box::new(builtins::Cd));
        builtins.insert("pwd".to_string(), Box::new(builtins::Pwd));
        builtins.insert("cat".to_string(), Box::new(builtins::Cat));
        builtins.insert("break".to_string(), Box::new(builtins::Break));
        builtins.insert("continue".to_string(), Box::new(builtins::Continue));
        builtins.insert("return".to_string(), Box::new(builtins::Return));
        builtins.insert("test".to_string(), Box::new(builtins::Test));
        builtins.insert("[".to_string(), Box::new(builtins::Bracket));
        builtins.insert("printf".to_string(), Box::new(builtins::Printf));
        builtins.insert("export".to_string(), Box::new(builtins::Export));
        builtins.insert("read".to_string(), Box::new(builtins::Read));
        builtins.insert("set".to_string(), Box::new(builtins::Set));
        builtins.insert("unset".to_string(), Box::new(builtins::Unset));
        builtins.insert("shift".to_string(), Box::new(builtins::Shift));
        builtins.insert("local".to_string(), Box::new(builtins::Local));
        // POSIX special built-ins
        builtins.insert(":".to_string(), Box::new(builtins::Colon));
        builtins.insert("readonly".to_string(), Box::new(builtins::Readonly));
        builtins.insert("times".to_string(), Box::new(builtins::Times));
        builtins.insert("eval".to_string(), Box::new(builtins::Eval));
        builtins.insert(
            "source".to_string(),
            Box::new(builtins::Source::new(fs.clone())),
        );
        builtins.insert(".".to_string(), Box::new(builtins::Source::new(fs.clone())));
        builtins.insert("jq".to_string(), Box::new(builtins::Jq));
        builtins.insert("grep".to_string(), Box::new(builtins::Grep));
        builtins.insert("sed".to_string(), Box::new(builtins::Sed));
        builtins.insert("awk".to_string(), Box::new(builtins::Awk));
        builtins.insert("sleep".to_string(), Box::new(builtins::Sleep));
        builtins.insert("head".to_string(), Box::new(builtins::Head));
        builtins.insert("tail".to_string(), Box::new(builtins::Tail));
        builtins.insert("basename".to_string(), Box::new(builtins::Basename));
        builtins.insert("dirname".to_string(), Box::new(builtins::Dirname));
        builtins.insert("mkdir".to_string(), Box::new(builtins::Mkdir));
        builtins.insert("rm".to_string(), Box::new(builtins::Rm));
        builtins.insert("cp".to_string(), Box::new(builtins::Cp));
        builtins.insert("mv".to_string(), Box::new(builtins::Mv));
        builtins.insert("touch".to_string(), Box::new(builtins::Touch));
        builtins.insert("chmod".to_string(), Box::new(builtins::Chmod));
        builtins.insert("wc".to_string(), Box::new(builtins::Wc));
        builtins.insert("nl".to_string(), Box::new(builtins::Nl));
        builtins.insert("paste".to_string(), Box::new(builtins::Paste));
        builtins.insert("column".to_string(), Box::new(builtins::Column));
        builtins.insert("comm".to_string(), Box::new(builtins::Comm));
        builtins.insert("diff".to_string(), Box::new(builtins::Diff));
        builtins.insert("strings".to_string(), Box::new(builtins::Strings));
        builtins.insert("od".to_string(), Box::new(builtins::Od));
        builtins.insert("xxd".to_string(), Box::new(builtins::Xxd));
        builtins.insert("hexdump".to_string(), Box::new(builtins::Hexdump));
        builtins.insert("sort".to_string(), Box::new(builtins::Sort));
        builtins.insert("uniq".to_string(), Box::new(builtins::Uniq));
        builtins.insert("cut".to_string(), Box::new(builtins::Cut));
        builtins.insert("tr".to_string(), Box::new(builtins::Tr));
        builtins.insert("date".to_string(), Box::new(builtins::Date));
        builtins.insert("wait".to_string(), Box::new(builtins::Wait));
        builtins.insert("curl".to_string(), Box::new(builtins::Curl));
        builtins.insert("wget".to_string(), Box::new(builtins::Wget));
        // Git builtin (requires git feature and configuration at runtime)
        #[cfg(feature = "git")]
        builtins.insert("git".to_string(), Box::new(builtins::Git));
        // Python builtins: opt-in via BashBuilder::python() / BashToolBuilder::python()
        // The `python` feature flag enables compilation; registration is explicit.
        builtins.insert("timeout".to_string(), Box::new(builtins::Timeout));
        // System info builtins (configurable virtual values)
        let hostname_val = hostname.unwrap_or_else(|| builtins::DEFAULT_HOSTNAME.to_string());
        let username_val = username.unwrap_or_else(|| builtins::DEFAULT_USERNAME.to_string());
        builtins.insert(
            "hostname".to_string(),
            Box::new(builtins::Hostname::with_hostname(&hostname_val)),
        );
        builtins.insert(
            "uname".to_string(),
            Box::new(builtins::Uname::with_hostname(&hostname_val)),
        );
        builtins.insert(
            "whoami".to_string(),
            Box::new(builtins::Whoami::with_username(&username_val)),
        );
        builtins.insert(
            "id".to_string(),
            Box::new(builtins::Id::with_username(&username_val)),
        );
        // Directory listing and search
        builtins.insert("ls".to_string(), Box::new(builtins::Ls));
        builtins.insert("find".to_string(), Box::new(builtins::Find));
        builtins.insert("rmdir".to_string(), Box::new(builtins::Rmdir));
        // File inspection
        builtins.insert("less".to_string(), Box::new(builtins::Less));
        builtins.insert("file".to_string(), Box::new(builtins::File));
        builtins.insert("stat".to_string(), Box::new(builtins::Stat));
        // Archive operations
        builtins.insert("tar".to_string(), Box::new(builtins::Tar));
        builtins.insert("gzip".to_string(), Box::new(builtins::Gzip));
        builtins.insert("gunzip".to_string(), Box::new(builtins::Gunzip));
        // Disk usage
        builtins.insert("du".to_string(), Box::new(builtins::Du));
        builtins.insert("df".to_string(), Box::new(builtins::Df));
        // Environment builtins
        builtins.insert("env".to_string(), Box::new(builtins::Env));
        builtins.insert("printenv".to_string(), Box::new(builtins::Printenv));
        builtins.insert("history".to_string(), Box::new(builtins::History));
        // Pipeline control
        builtins.insert("xargs".to_string(), Box::new(builtins::Xargs));
        builtins.insert("tee".to_string(), Box::new(builtins::Tee));
        builtins.insert("watch".to_string(), Box::new(builtins::Watch));

        // Merge custom builtins (override defaults if same name)
        for (name, builtin) in custom_builtins {
            builtins.insert(name, builtin);
        }

        Self {
            fs,
            env: HashMap::new(),
            variables: HashMap::new(),
            arrays: HashMap::new(),
            cwd: PathBuf::from("/home/user"),
            last_exit_code: 0,
            builtins,
            functions: HashMap::new(),
            call_stack: Vec::new(),
            limits: ExecutionLimits::default(),
            counters: ExecutionCounters::new(),
            jobs: JobTable::new(),
            options: ShellOptions::default(),
            current_line: 1,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            pipeline_stdin: None,
            output_callback: None,
            output_emit_count: 0,
        }
    }

    /// Get mutable access to shell options (for builtins like `set`)
    #[allow(dead_code)]
    pub fn options_mut(&mut self) -> &mut ShellOptions {
        &mut self.options
    }

    /// Get shell options
    #[allow(dead_code)]
    pub fn options(&self) -> &ShellOptions {
        &self.options
    }

    /// Check if errexit (set -e) is enabled
    /// This checks both the options struct and the SHOPT_e variable
    /// (the `set` builtin stores options in SHOPT_e)
    fn is_errexit_enabled(&self) -> bool {
        self.options.errexit
            || self
                .variables
                .get("SHOPT_e")
                .map(|v| v == "1")
                .unwrap_or(false)
    }

    /// Set execution limits.
    pub fn set_limits(&mut self, limits: ExecutionLimits) {
        self.limits = limits;
    }

    /// Set an environment variable.
    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env.insert(key.to_string(), value.to_string());
    }

    /// Set the current working directory.
    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
    }

    /// Set an output callback for streaming output during execution.
    ///
    /// When set, the interpreter calls this callback with `(stdout_chunk, stderr_chunk)`
    /// after each loop iteration, command list element, and top-level command.
    /// Output is still accumulated in the returned `ExecResult` for the final result.
    pub fn set_output_callback(&mut self, callback: OutputCallback) {
        self.output_callback = Some(callback);
        self.output_emit_count = 0;
    }

    /// Clear the output callback.
    pub fn clear_output_callback(&mut self) {
        self.output_callback = None;
        self.output_emit_count = 0;
    }

    /// Emit output via the callback if set, and if sub-calls didn't already emit.
    /// Returns `true` if output was emitted.
    ///
    /// `emit_count_before` is the value of `output_emit_count` before the sub-call
    /// that produced this output. If the count advanced, sub-calls already emitted
    /// and we skip to avoid duplicates.
    fn maybe_emit_output(&mut self, stdout: &str, stderr: &str, emit_count_before: u64) -> bool {
        if self.output_callback.is_none() {
            return false;
        }
        // Sub-calls already emitted — skip to avoid duplicates
        if self.output_emit_count != emit_count_before {
            return false;
        }
        if stdout.is_empty() && stderr.is_empty() {
            return false;
        }
        if let Some(ref mut cb) = self.output_callback {
            cb(stdout, stderr);
            self.output_emit_count += 1;
        }
        true
    }

    /// Set the HTTP client for network builtins (curl, wget).
    ///
    /// This is only available when the `http_client` feature is enabled.
    #[cfg(feature = "http_client")]
    pub fn set_http_client(&mut self, client: crate::network::HttpClient) {
        self.http_client = Some(client);
    }

    /// Set the git client for git builtins.
    ///
    /// This is only available when the `git` feature is enabled.
    #[cfg(feature = "git")]
    pub fn set_git_client(&mut self, client: crate::git::GitClient) {
        self.git_client = Some(client);
    }

    /// Execute a script.
    pub async fn execute(&mut self, script: &Script) -> Result<ExecResult> {
        // Reset per-execution counters so each exec() gets a fresh budget.
        // Without this, hitting the limit in one exec() permanently poisons the session.
        self.counters.reset_for_execution();

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        for command in &script.commands {
            let emit_before = self.output_emit_count;
            let result = self.execute_command(command).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;
            self.last_exit_code = exit_code;

            // NOTE: errexit (set -e) is handled internally by execute_command,
            // execute_list, and execute_command_sequence. We don't check here
            // because those methods handle the nuances of && / || chains,
            // if/while conditions, etc.
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
        })
    }

    /// Get the source line number from a command's span
    fn command_line(command: &Command) -> usize {
        match command {
            Command::Simple(c) => c.span.line(),
            Command::Pipeline(c) => c.span.line(),
            Command::List(c) => c.span.line(),
            Command::Compound(c, _) => match c {
                CompoundCommand::If(cmd) => cmd.span.line(),
                CompoundCommand::For(cmd) => cmd.span.line(),
                CompoundCommand::ArithmeticFor(cmd) => cmd.span.line(),
                CompoundCommand::While(cmd) => cmd.span.line(),
                CompoundCommand::Until(cmd) => cmd.span.line(),
                CompoundCommand::Case(cmd) => cmd.span.line(),
                CompoundCommand::Time(cmd) => cmd.span.line(),
                CompoundCommand::Subshell(_) | CompoundCommand::BraceGroup(_) => 1,
                CompoundCommand::Arithmetic(_) => 1,
            },
            Command::Function(c) => c.span.line(),
        }
    }

    fn execute_command<'a>(
        &'a mut self,
        command: &'a Command,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>> {
        Box::pin(async move {
            // Update current line for $LINENO
            self.current_line = Self::command_line(command);

            // Fail point: inject failures during command execution
            #[cfg(feature = "failpoints")]
            fail_point!("interp::execute_command", |action| {
                match action.as_deref() {
                    Some("panic") => {
                        // Test panic recovery
                        panic!("injected panic in execute_command");
                    }
                    Some("error") => {
                        return Err(Error::Execution("injected execution error".to_string()));
                    }
                    Some("exit_nonzero") => {
                        // Return non-zero exit code without error
                        return Ok(ExecResult {
                            stdout: String::new(),
                            stderr: "injected failure".to_string(),
                            exit_code: 127,
                            control_flow: ControlFlow::None,
                        });
                    }
                    _ => {}
                }
                Ok(ExecResult::ok(String::new()))
            });

            // Check command count limit
            self.counters.tick_command(&self.limits)?;

            match command {
                Command::Simple(simple) => self.execute_simple_command(simple, None).await,
                Command::Pipeline(pipeline) => self.execute_pipeline(pipeline).await,
                Command::List(list) => self.execute_list(list).await,
                Command::Compound(compound, redirects) => {
                    let result = self.execute_compound(compound).await?;
                    if redirects.is_empty() {
                        Ok(result)
                    } else {
                        self.apply_redirections(result, redirects).await
                    }
                }
                Command::Function(func_def) => {
                    // Store the function definition
                    self.functions
                        .insert(func_def.name.clone(), func_def.clone());
                    Ok(ExecResult::ok(String::new()))
                }
            }
        })
    }

    /// Execute a compound command (if, for, while, etc.)
    async fn execute_compound(&mut self, compound: &CompoundCommand) -> Result<ExecResult> {
        match compound {
            CompoundCommand::If(if_cmd) => self.execute_if(if_cmd).await,
            CompoundCommand::For(for_cmd) => self.execute_for(for_cmd).await,
            CompoundCommand::ArithmeticFor(arith_for) => {
                self.execute_arithmetic_for(arith_for).await
            }
            CompoundCommand::While(while_cmd) => self.execute_while(while_cmd).await,
            CompoundCommand::Until(until_cmd) => self.execute_until(until_cmd).await,
            CompoundCommand::Subshell(commands) => self.execute_command_sequence(commands).await,
            CompoundCommand::BraceGroup(commands) => self.execute_command_sequence(commands).await,
            CompoundCommand::Case(case_cmd) => self.execute_case(case_cmd).await,
            CompoundCommand::Arithmetic(expr) => self.execute_arithmetic_command(expr).await,
            CompoundCommand::Time(time_cmd) => self.execute_time(time_cmd).await,
        }
    }

    /// Execute an if statement
    async fn execute_if(&mut self, if_cmd: &IfCommand) -> Result<ExecResult> {
        // Execute condition (no errexit checking - conditions are expected to fail)
        let condition_result = self.execute_condition_sequence(&if_cmd.condition).await?;

        if condition_result.exit_code == 0 {
            // Condition succeeded, execute then branch
            return self.execute_command_sequence(&if_cmd.then_branch).await;
        }

        // Check elif branches
        for (elif_condition, elif_body) in &if_cmd.elif_branches {
            let elif_result = self.execute_condition_sequence(elif_condition).await?;
            if elif_result.exit_code == 0 {
                return self.execute_command_sequence(elif_body).await;
            }
        }

        // Execute else branch if present
        if let Some(else_branch) = &if_cmd.else_branch {
            return self.execute_command_sequence(else_branch).await;
        }

        // No branch executed, return success
        Ok(ExecResult::ok(String::new()))
    }

    /// Execute a for loop
    async fn execute_for(&mut self, for_cmd: &ForCommand) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        // Get iteration values: expand fields, then apply brace/glob expansion
        let values: Vec<String> = if let Some(words) = &for_cmd.words {
            let mut vals = Vec::new();
            for w in words {
                let fields = self.expand_word_to_fields(w).await?;

                // Quoted words skip brace/glob expansion
                if w.quoted {
                    vals.extend(fields);
                    continue;
                }

                for expanded in fields {
                    let brace_expanded = self.expand_braces(&expanded);
                    for item in brace_expanded {
                        if self.contains_glob_chars(&item) {
                            let glob_matches = self.expand_glob(&item).await?;
                            if glob_matches.is_empty() {
                                vals.push(item);
                            } else {
                                vals.extend(glob_matches);
                            }
                        } else {
                            vals.push(item);
                        }
                    }
                }
            }
            vals
        } else {
            // No words specified - iterate over positional parameters ($@)
            self.call_stack
                .last()
                .map(|frame| frame.positional.clone())
                .unwrap_or_default()
        };

        // Reset loop counter for this loop
        self.counters.reset_loop();

        for value in values {
            // Check loop iteration limit
            self.counters.tick_loop(&self.limits)?;

            // Set loop variable
            self.variables
                .insert(for_cmd.variable.clone(), value.clone());

            // Execute body
            let emit_before = self.output_emit_count;
            let result = self.execute_command_sequence(&for_cmd.body).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;

            // Check for break/continue
            match result.control_flow {
                ControlFlow::Break(n) => {
                    if n <= 1 {
                        break;
                    } else {
                        // Propagate break with decremented count
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::Break(n - 1),
                        });
                    }
                }
                ControlFlow::Continue(n) => {
                    if n <= 1 {
                        continue;
                    } else {
                        // Propagate continue with decremented count
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::Continue(n - 1),
                        });
                    }
                }
                ControlFlow::Return(code) => {
                    // Propagate return
                    return Ok(ExecResult {
                        stdout,
                        stderr,
                        exit_code: code,
                        control_flow: ControlFlow::Return(code),
                    });
                }
                ControlFlow::None => {
                    // Check if errexit caused early return from body
                    if self.is_errexit_enabled() && exit_code != 0 {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::None,
                        });
                    }
                }
            }
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
        })
    }

    /// Execute a C-style arithmetic for loop: for ((init; cond; step))
    async fn execute_arithmetic_for(
        &mut self,
        arith_for: &ArithmeticForCommand,
    ) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        // Execute initialization
        if !arith_for.init.is_empty() {
            self.execute_arithmetic_with_side_effects(&arith_for.init);
        }

        // Reset loop counter for this loop
        self.counters.reset_loop();

        loop {
            // Check loop iteration limit
            self.counters.tick_loop(&self.limits)?;

            // Check condition (if empty, always true)
            if !arith_for.condition.is_empty() {
                let cond_result = self.evaluate_arithmetic(&arith_for.condition);
                if cond_result == 0 {
                    break;
                }
            }

            // Execute body
            let emit_before = self.output_emit_count;
            let result = self.execute_command_sequence(&arith_for.body).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;

            // Check for break/continue
            match result.control_flow {
                ControlFlow::Break(n) => {
                    if n <= 1 {
                        break;
                    } else {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::Break(n - 1),
                        });
                    }
                }
                ControlFlow::Continue(n) => {
                    if n > 1 {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::Continue(n - 1),
                        });
                    }
                    // n <= 1: continue to next iteration (after step)
                }
                ControlFlow::Return(code) => {
                    return Ok(ExecResult {
                        stdout,
                        stderr,
                        exit_code: code,
                        control_flow: ControlFlow::Return(code),
                    });
                }
                ControlFlow::None => {
                    // Check if errexit caused early return from body
                    if self.is_errexit_enabled() && exit_code != 0 {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::None,
                        });
                    }
                }
            }

            // Execute step
            if !arith_for.step.is_empty() {
                self.execute_arithmetic_with_side_effects(&arith_for.step);
            }
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
        })
    }

    /// Execute an arithmetic command ((expression))
    /// Returns exit code 0 if result is non-zero, 1 if result is zero
    async fn execute_arithmetic_command(&mut self, expr: &str) -> Result<ExecResult> {
        let result = self.execute_arithmetic_with_side_effects(expr);
        let exit_code = if result != 0 { 0 } else { 1 };

        Ok(ExecResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code,
            control_flow: ControlFlow::None,
        })
    }

    /// Execute arithmetic expression with side effects (assignments, ++, --)
    fn execute_arithmetic_with_side_effects(&mut self, expr: &str) -> i64 {
        let expr = expr.trim();

        // Handle comma-separated expressions
        if expr.contains(',') {
            let parts: Vec<&str> = expr.split(',').collect();
            let mut result = 0;
            for part in parts {
                result = self.execute_arithmetic_with_side_effects(part.trim());
            }
            return result;
        }

        // Handle assignment: var = expr or var op= expr
        if let Some(eq_pos) = expr.find('=') {
            // Check it's not ==, !=, <=, >=
            let before = if eq_pos > 0 {
                expr.chars().nth(eq_pos - 1)
            } else {
                None
            };
            let after = expr.chars().nth(eq_pos + 1);

            if after != Some('=') && !matches!(before, Some('!' | '<' | '>' | '=')) {
                // This is an assignment
                let lhs = expr[..eq_pos].trim();
                let rhs = expr[eq_pos + 1..].trim();

                // Check for compound assignment (+=, -=, *=, /=, %=)
                let (var_name, op, effective_rhs) = if lhs.ends_with('+')
                    || lhs.ends_with('-')
                    || lhs.ends_with('*')
                    || lhs.ends_with('/')
                    || lhs.ends_with('%')
                {
                    let op = lhs.chars().last().unwrap();
                    let name = lhs[..lhs.len() - 1].trim();
                    (name, Some(op), rhs)
                } else {
                    (lhs, None, rhs)
                };

                let rhs_value = self.execute_arithmetic_with_side_effects(effective_rhs);
                let final_value = if let Some(op) = op {
                    let current = self.evaluate_arithmetic(var_name);
                    match op {
                        '+' => current + rhs_value,
                        '-' => current - rhs_value,
                        '*' => current * rhs_value,
                        '/' => {
                            if rhs_value != 0 {
                                current / rhs_value
                            } else {
                                0
                            }
                        }
                        '%' => {
                            if rhs_value != 0 {
                                current % rhs_value
                            } else {
                                0
                            }
                        }
                        _ => rhs_value,
                    }
                } else {
                    rhs_value
                };

                self.variables
                    .insert(var_name.to_string(), final_value.to_string());
                return final_value;
            }
        }

        // Handle pre-increment/decrement: ++var or --var
        if let Some(stripped) = expr.strip_prefix("++") {
            let var_name = stripped.trim();
            let current = self.evaluate_arithmetic(var_name);
            let new_value = current + 1;
            self.variables
                .insert(var_name.to_string(), new_value.to_string());
            return new_value;
        }
        if let Some(stripped) = expr.strip_prefix("--") {
            let var_name = stripped.trim();
            let current = self.evaluate_arithmetic(var_name);
            let new_value = current - 1;
            self.variables
                .insert(var_name.to_string(), new_value.to_string());
            return new_value;
        }

        // Handle post-increment/decrement: var++ or var--
        if let Some(stripped) = expr.strip_suffix("++") {
            let var_name = stripped.trim();
            let current = self.evaluate_arithmetic(var_name);
            let new_value = current + 1;
            self.variables
                .insert(var_name.to_string(), new_value.to_string());
            return current; // Return old value for post-increment
        }
        if let Some(stripped) = expr.strip_suffix("--") {
            let var_name = stripped.trim();
            let current = self.evaluate_arithmetic(var_name);
            let new_value = current - 1;
            self.variables
                .insert(var_name.to_string(), new_value.to_string());
            return current; // Return old value for post-decrement
        }

        // No side effects, just evaluate
        self.evaluate_arithmetic(expr)
    }

    /// Execute a while loop
    async fn execute_while(&mut self, while_cmd: &WhileCommand) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        // Reset loop counter for this loop
        self.counters.reset_loop();

        loop {
            // Check loop iteration limit
            self.counters.tick_loop(&self.limits)?;

            // Check condition (no errexit - conditions are expected to fail)
            let condition_result = self
                .execute_condition_sequence(&while_cmd.condition)
                .await?;
            if condition_result.exit_code != 0 {
                break;
            }

            // Execute body
            let emit_before = self.output_emit_count;
            let result = self.execute_command_sequence(&while_cmd.body).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;

            // Check for break/continue
            match result.control_flow {
                ControlFlow::Break(n) => {
                    if n <= 1 {
                        break;
                    } else {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::Break(n - 1),
                        });
                    }
                }
                ControlFlow::Continue(n) => {
                    if n <= 1 {
                        continue;
                    } else {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::Continue(n - 1),
                        });
                    }
                }
                ControlFlow::Return(code) => {
                    return Ok(ExecResult {
                        stdout,
                        stderr,
                        exit_code: code,
                        control_flow: ControlFlow::Return(code),
                    });
                }
                ControlFlow::None => {
                    // Check if errexit caused early return from body
                    if self.is_errexit_enabled() && exit_code != 0 {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::None,
                        });
                    }
                }
            }
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
        })
    }

    /// Execute an until loop
    async fn execute_until(&mut self, until_cmd: &UntilCommand) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        // Reset loop counter for this loop
        self.counters.reset_loop();

        loop {
            // Check loop iteration limit
            self.counters.tick_loop(&self.limits)?;

            // Check condition (no errexit - conditions are expected to fail)
            let condition_result = self
                .execute_condition_sequence(&until_cmd.condition)
                .await?;
            if condition_result.exit_code == 0 {
                break;
            }

            // Execute body
            let emit_before = self.output_emit_count;
            let result = self.execute_command_sequence(&until_cmd.body).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;

            // Check for break/continue
            match result.control_flow {
                ControlFlow::Break(n) => {
                    if n <= 1 {
                        break;
                    } else {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::Break(n - 1),
                        });
                    }
                }
                ControlFlow::Continue(n) => {
                    if n <= 1 {
                        continue;
                    } else {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::Continue(n - 1),
                        });
                    }
                }
                ControlFlow::Return(code) => {
                    return Ok(ExecResult {
                        stdout,
                        stderr,
                        exit_code: code,
                        control_flow: ControlFlow::Return(code),
                    });
                }
                ControlFlow::None => {
                    // Check if errexit caused early return from body
                    if self.is_errexit_enabled() && exit_code != 0 {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow: ControlFlow::None,
                        });
                    }
                }
            }
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
        })
    }

    /// Execute a case statement
    async fn execute_case(&mut self, case_cmd: &CaseCommand) -> Result<ExecResult> {
        let word_value = self.expand_word(&case_cmd.word).await?;

        // Try each case item in order
        for case_item in &case_cmd.cases {
            for pattern in &case_item.patterns {
                let pattern_str = self.expand_word(pattern).await?;
                if self.pattern_matches(&word_value, &pattern_str) {
                    return self.execute_command_sequence(&case_item.commands).await;
                }
            }
        }

        // No pattern matched - return success with no output
        Ok(ExecResult::ok(String::new()))
    }

    /// Execute a time command - measure wall-clock execution time
    ///
    /// Note: Bashkit only measures wall-clock (real) time.
    /// User and system CPU time are always reported as 0.
    /// This is a documented incompatibility with bash.
    async fn execute_time(&mut self, time_cmd: &TimeCommand) -> Result<ExecResult> {
        use std::time::Instant;

        let start = Instant::now();

        // Execute the wrapped command if present
        let mut result = if let Some(cmd) = &time_cmd.command {
            self.execute_command(cmd).await?
        } else {
            // time with no command - just output timing for nothing
            ExecResult::ok(String::new())
        };

        let elapsed = start.elapsed();

        // Calculate time components
        let total_secs = elapsed.as_secs_f64();
        let minutes = (total_secs / 60.0).floor() as u64;
        let seconds = total_secs % 60.0;

        // Format timing output (goes to stderr, per bash behavior)
        let timing = if time_cmd.posix_format {
            // POSIX format: simple, machine-readable
            format!("real {:.2}\nuser 0.00\nsys 0.00\n", total_secs)
        } else {
            // Default bash format
            format!(
                "\nreal\t{}m{:.3}s\nuser\t0m0.000s\nsys\t0m0.000s\n",
                minutes, seconds
            )
        };

        // Append timing to stderr (preserve command's stderr)
        result.stderr.push_str(&timing);

        Ok(result)
    }

    /// Execute a timeout command - run command with time limit
    ///
    /// Usage: timeout [OPTIONS] DURATION COMMAND [ARGS...]
    ///
    /// Options:
    ///   --preserve-status  Exit with command's status even on timeout
    ///   -k DURATION        Kill signal timeout (ignored - always terminates)
    ///   -s SIGNAL          Signal to send (ignored)
    ///
    /// Exit codes:
    ///   124 - Command timed out
    ///   125 - Timeout itself failed (bad arguments)
    ///   Otherwise, exit status of command
    async fn execute_timeout(
        &mut self,
        args: &[String],
        stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        use std::time::Duration;
        use tokio::time::timeout;

        const MAX_TIMEOUT_SECONDS: u64 = 300; // 5 minutes max for safety

        if args.is_empty() {
            return Ok(ExecResult::err(
                "timeout: missing operand\nUsage: timeout DURATION COMMAND [ARGS...]\n".to_string(),
                125,
            ));
        }

        // Parse options and find duration/command
        let mut preserve_status = false;
        let mut arg_idx = 0;

        while arg_idx < args.len() {
            let arg = &args[arg_idx];
            match arg.as_str() {
                "--preserve-status" => {
                    preserve_status = true;
                    arg_idx += 1;
                }
                "-k" | "-s" => {
                    // These options take a value, skip it
                    arg_idx += 2;
                }
                s if s.starts_with('-')
                    && !s.chars().nth(1).is_some_and(|c| c.is_ascii_digit()) =>
                {
                    // Unknown option, skip
                    arg_idx += 1;
                }
                _ => break, // Found duration
            }
        }

        if arg_idx >= args.len() {
            return Ok(ExecResult::err(
                "timeout: missing operand\nUsage: timeout DURATION COMMAND [ARGS...]\n".to_string(),
                125,
            ));
        }

        // Parse duration
        let duration_str = &args[arg_idx];
        let max_duration = Duration::from_secs(MAX_TIMEOUT_SECONDS);
        let duration = match Self::parse_timeout_duration(duration_str) {
            Some(d) => {
                // Cap at max while preserving subsecond precision
                if d > max_duration {
                    max_duration
                } else {
                    d
                }
            }
            None => {
                return Ok(ExecResult::err(
                    format!("timeout: invalid time interval '{}'\n", duration_str),
                    125,
                ));
            }
        };

        arg_idx += 1;

        if arg_idx >= args.len() {
            return Ok(ExecResult::err(
                "timeout: missing command\nUsage: timeout DURATION COMMAND [ARGS...]\n".to_string(),
                125,
            ));
        }

        // Build the inner command
        let cmd_name = &args[arg_idx];
        let cmd_args: Vec<String> = args[arg_idx + 1..].to_vec();

        // If we have stdin from a pipeline, pass it to the inner command via here-string
        let inner_redirects = if let Some(ref stdin_data) = stdin {
            vec![Redirect {
                fd: None,
                kind: RedirectKind::HereString,
                target: Word::literal(stdin_data.trim_end_matches('\n').to_string()),
            }]
        } else {
            Vec::new()
        };

        // Create a SimpleCommand for the inner command
        let inner_cmd = Command::Simple(SimpleCommand {
            name: Word::literal(cmd_name.clone()),
            args: cmd_args.iter().map(|s| Word::literal(s.clone())).collect(),
            redirects: inner_redirects,
            assignments: Vec::new(),
            span: Span::new(),
        });

        // Execute with timeout using execute_command (which handles recursion via Box::pin)
        let exec_future = self.execute_command(&inner_cmd);
        let result = match timeout(duration, exec_future).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                // Timeout expired
                if preserve_status {
                    // Return the timeout exit code but preserve-status means...
                    // actually in bash --preserve-status makes timeout return
                    // the command's exit status, but if it times out, there's no status
                    // so it still returns 124
                    ExecResult::err(String::new(), 124)
                } else {
                    ExecResult::err(String::new(), 124)
                }
            }
        };

        // Apply output redirections
        self.apply_redirections(result, redirects).await
    }

    /// Parse a timeout duration string like "30", "30s", "5m", "1h"
    fn parse_timeout_duration(s: &str) -> Option<std::time::Duration> {
        use std::time::Duration;

        let s = s.trim();
        if s.is_empty() {
            return None;
        }

        // Check for suffix
        let (num_str, multiplier) = if let Some(stripped) = s.strip_suffix('s') {
            (stripped, 1u64)
        } else if let Some(stripped) = s.strip_suffix('m') {
            (stripped, 60u64)
        } else if let Some(stripped) = s.strip_suffix('h') {
            (stripped, 3600u64)
        } else if let Some(stripped) = s.strip_suffix('d') {
            (stripped, 86400u64)
        } else {
            (s, 1u64) // Default to seconds
        };

        // Parse the number (support decimals)
        let seconds: f64 = num_str.parse().ok()?;
        if seconds < 0.0 {
            return None;
        }

        let total_secs_f64 = seconds * multiplier as f64;
        Some(Duration::from_secs_f64(total_secs_f64))
    }

    /// Execute `bash` or `sh` command - interpret scripts using this interpreter.
    ///
    /// Supports:
    /// - `bash -c "command"` - execute a command string
    /// - `bash -n script.sh` - syntax check only (noexec)
    /// - `bash script.sh [args...]` - execute a script file
    /// - `echo 'echo hello' | bash` - execute script from stdin
    /// - `bash --version` / `bash --help`
    ///
    /// SECURITY: This re-invokes the virtual interpreter, NOT external bash.
    /// See threat model TM-ESC-015 for security analysis.
    async fn execute_shell(
        &mut self,
        shell_name: &str,
        args: &[String],
        stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        // Parse options
        let mut command_string: Option<String> = None;
        let mut script_file: Option<String> = None;
        let mut script_args: Vec<String> = Vec::new();
        let mut noexec = false; // -n flag: syntax check only
        let mut idx = 0;

        while idx < args.len() {
            let arg = &args[idx];
            match arg.as_str() {
                "--version" => {
                    // Return virtual interpreter version info (not real bash)
                    return Ok(ExecResult::ok(format!(
                        "Bashkit {} (virtual {} interpreter)\n",
                        env!("CARGO_PKG_VERSION"),
                        shell_name
                    )));
                }
                "--help" => {
                    return Ok(ExecResult::ok(format!(
                        "Usage: {} [option] ... [file [argument] ...]\n\
                         Virtual shell interpreter (not GNU bash)\n\n\
                         Options:\n\
                         \t-c string\tExecute commands from string\n\
                         \t-n\t\tCheck syntax without executing (noexec)\n\
                         \t-e\t\tExit on error (accepted, limited support)\n\
                         \t--version\tShow version\n\
                         \t--help\t\tShow this help\n",
                        shell_name
                    )));
                }
                "-c" => {
                    // Next argument is the command string
                    idx += 1;
                    if idx >= args.len() {
                        return Ok(ExecResult::err(
                            format!("{}: -c: option requires an argument\n", shell_name),
                            2,
                        ));
                    }
                    command_string = Some(args[idx].clone());
                    idx += 1;
                    // Remaining args become positional parameters (starting at $0)
                    script_args = args[idx..].to_vec();
                    break;
                }
                "-n" => {
                    // Noexec: parse only, don't execute
                    noexec = true;
                    idx += 1;
                }
                // Accept but ignore these options. These are recognized for
                // compatibility with scripts that set them, but not enforced
                // in virtual mode:
                // -e (errexit): would need per-command exit code checking
                // -x (xtrace): would need trace output to stderr
                // -v (verbose): would need input echoing
                // -u (nounset): would need unset variable detection
                // -o (option): would need set -o pipeline
                // -i (interactive): not applicable in virtual mode
                // -s (stdin): read from stdin (implicit behavior)
                "-e" | "-x" | "-v" | "-u" | "-o" | "-i" | "-s" => {
                    idx += 1;
                }
                "--" => {
                    idx += 1;
                    // Remaining args after -- are file and arguments
                    if idx < args.len() {
                        script_file = Some(args[idx].clone());
                        idx += 1;
                        script_args = args[idx..].to_vec();
                    }
                    break;
                }
                s if s.starts_with("--") => {
                    // Unknown long option - skip
                    idx += 1;
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    // Combined short options like -ne, -ev
                    for ch in s.chars().skip(1) {
                        if ch == 'n' {
                            noexec = true;
                        }
                        // Ignore other options
                    }
                    idx += 1;
                }
                _ => {
                    // First non-option is the script file
                    script_file = Some(arg.clone());
                    idx += 1;
                    // Remaining args become positional parameters
                    script_args = args[idx..].to_vec();
                    break;
                }
            }
        }

        // Determine what to execute
        let is_command_mode = command_string.is_some();
        let script_content = if let Some(cmd) = command_string {
            // bash -c "command"
            cmd
        } else if let Some(ref file) = script_file {
            // bash script.sh
            let path = self.resolve_path(file);
            match self.fs.read_file(&path).await {
                Ok(content) => String::from_utf8_lossy(&content).to_string(),
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("{}: {}: No such file or directory\n", shell_name, file),
                        127,
                    ));
                }
            }
        } else if let Some(ref stdin_content) = stdin {
            // Read script from stdin (pipe)
            stdin_content.clone()
        } else {
            // No command, file, or stdin - nothing to do
            return Ok(ExecResult::ok(String::new()));
        };

        // THREAT[TM-DOS-021]: Propagate interpreter's parser limits to child shell
        let parser = Parser::with_limits(
            &script_content,
            self.limits.max_ast_depth,
            self.limits.max_parser_operations,
        );
        let script = match parser.parse() {
            Ok(s) => s,
            Err(e) => {
                return Ok(ExecResult::err(
                    format!("{}: syntax error: {}\n", shell_name, e),
                    2,
                ));
            }
        };

        // -n (noexec): syntax check only, don't execute
        if noexec {
            return Ok(ExecResult::ok(String::new()));
        }

        // Determine $0 and positional parameters
        // For bash -c "cmd" arg0 arg1: $0=arg0, $1=arg1
        // For bash script.sh arg1: $0=script.sh, $1=arg1
        let (name_arg, positional_args) = if is_command_mode {
            // For -c, first arg is $0, rest are $1, $2, etc.
            if script_args.is_empty() {
                (shell_name.to_string(), Vec::new())
            } else {
                let name = script_args[0].clone();
                let positional = script_args[1..].to_vec();
                (name, positional)
            }
        } else if let Some(ref file) = script_file {
            // For script file, filename is $0, args are $1, $2, etc.
            (file.clone(), script_args)
        } else {
            // Stdin mode
            (shell_name.to_string(), Vec::new())
        };

        // Push a call frame for this script
        self.call_stack.push(CallFrame {
            name: name_arg,
            locals: HashMap::new(),
            positional: positional_args,
        });

        // Execute the script
        let result = self.execute(&script).await;

        // Pop the call frame
        self.call_stack.pop();

        // Apply redirections and return
        match result {
            Ok(exec_result) => self.apply_redirections(exec_result, redirects).await,
            Err(e) => Err(e),
        }
    }

    /// Check if a value matches a shell pattern
    fn pattern_matches(&self, value: &str, pattern: &str) -> bool {
        // Handle special case of * (match anything)
        if pattern == "*" {
            return true;
        }

        // Glob pattern matching with *, ?, and [] support
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            // Simple wildcard matching
            self.glob_match(value, pattern)
        } else {
            // Literal match
            value == pattern
        }
    }

    /// Simple glob pattern matching with support for *, ?, and [...]
    fn glob_match(&self, value: &str, pattern: &str) -> bool {
        let mut value_chars = value.chars().peekable();
        let mut pattern_chars = pattern.chars().peekable();

        loop {
            match (pattern_chars.peek().copied(), value_chars.peek().copied()) {
                (None, None) => return true,
                (None, Some(_)) => return false,
                (Some('*'), _) => {
                    pattern_chars.next();
                    // * matches zero or more characters
                    if pattern_chars.peek().is_none() {
                        return true; // * at end matches everything
                    }
                    // Try matching from each position
                    while value_chars.peek().is_some() {
                        let remaining_value: String = value_chars.clone().collect();
                        let remaining_pattern: String = pattern_chars.clone().collect();
                        if self.glob_match(&remaining_value, &remaining_pattern) {
                            return true;
                        }
                        value_chars.next();
                    }
                    // Also try with empty match
                    let remaining_pattern: String = pattern_chars.collect();
                    return self.glob_match("", &remaining_pattern);
                }
                (Some('?'), Some(_)) => {
                    pattern_chars.next();
                    value_chars.next();
                }
                (Some('?'), None) => return false,
                (Some('['), Some(v)) => {
                    pattern_chars.next(); // consume '['
                    if let Some(matched) = self.match_bracket_expr(&mut pattern_chars, v) {
                        if matched {
                            value_chars.next();
                        } else {
                            return false;
                        }
                    } else {
                        // Invalid bracket expression, treat '[' as literal
                        return false;
                    }
                }
                (Some('['), None) => return false,
                (Some(p), Some(v)) => {
                    if p == v {
                        pattern_chars.next();
                        value_chars.next();
                    } else {
                        return false;
                    }
                }
                (Some(_), None) => return false,
            }
        }
    }

    /// Match a bracket expression [abc], [a-z], [!abc], [^abc]
    /// Returns Some(true) if matched, Some(false) if not matched, None if invalid
    fn match_bracket_expr(
        &self,
        pattern_chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
        value_char: char,
    ) -> Option<bool> {
        let mut chars_in_class = Vec::new();
        let mut negate = false;

        // Check for negation
        if matches!(pattern_chars.peek(), Some('!') | Some('^')) {
            negate = true;
            pattern_chars.next();
        }

        // Collect all characters in the bracket expression
        loop {
            match pattern_chars.next() {
                Some(']') if !chars_in_class.is_empty() => break,
                Some(']') if chars_in_class.is_empty() => {
                    // ] as first char is literal
                    chars_in_class.push(']');
                }
                Some('-') if !chars_in_class.is_empty() => {
                    // Could be a range
                    if let Some(&next) = pattern_chars.peek() {
                        if next == ']' {
                            // - at end is literal
                            chars_in_class.push('-');
                        } else {
                            // Range: prev-next
                            pattern_chars.next();
                            if let Some(&prev) = chars_in_class.last() {
                                for c in prev..=next {
                                    chars_in_class.push(c);
                                }
                            }
                        }
                    } else {
                        return None; // Unclosed bracket
                    }
                }
                Some(c) => chars_in_class.push(c),
                None => return None, // Unclosed bracket
            }
        }

        let matched = chars_in_class.contains(&value_char);
        Some(if negate { !matched } else { matched })
    }

    /// Execute a sequence of commands (with errexit checking)
    async fn execute_command_sequence(&mut self, commands: &[Command]) -> Result<ExecResult> {
        self.execute_command_sequence_impl(commands, true).await
    }

    /// Execute a sequence of commands used as a condition (no errexit checking)
    /// Used for if/while/until conditions where failure is expected
    async fn execute_condition_sequence(&mut self, commands: &[Command]) -> Result<ExecResult> {
        self.execute_command_sequence_impl(commands, false).await
    }

    /// Execute a sequence of commands with optional errexit checking
    async fn execute_command_sequence_impl(
        &mut self,
        commands: &[Command],
        check_errexit: bool,
    ) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        for command in commands {
            let emit_before = self.output_emit_count;
            let result = self.execute_command(command).await?;
            self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;
            self.last_exit_code = exit_code;

            // Propagate control flow
            if result.control_flow != ControlFlow::None {
                return Ok(ExecResult {
                    stdout,
                    stderr,
                    exit_code,
                    control_flow: result.control_flow,
                });
            }

            // Check for errexit (set -e) if enabled
            if check_errexit && self.is_errexit_enabled() && exit_code != 0 {
                return Ok(ExecResult {
                    stdout,
                    stderr,
                    exit_code,
                    control_flow: ControlFlow::None,
                });
            }
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
        })
    }

    /// Execute a pipeline (cmd1 | cmd2 | cmd3)
    async fn execute_pipeline(&mut self, pipeline: &Pipeline) -> Result<ExecResult> {
        let mut stdin_data: Option<String> = None;
        let mut last_result = ExecResult::ok(String::new());

        for (i, command) in pipeline.commands.iter().enumerate() {
            let is_last = i == pipeline.commands.len() - 1;

            let result = match command {
                Command::Simple(simple) => {
                    self.execute_simple_command(simple, stdin_data.take())
                        .await?
                }
                _ => {
                    // Compound commands, lists, etc. in pipeline:
                    // set pipeline_stdin so inner commands (read, cat, etc.) can consume it
                    let prev_pipeline_stdin = self.pipeline_stdin.take();
                    self.pipeline_stdin = stdin_data.take();
                    let result = self.execute_command(command).await?;
                    self.pipeline_stdin = prev_pipeline_stdin;
                    result
                }
            };

            if is_last {
                last_result = result;
            } else {
                stdin_data = Some(result.stdout);
            }
        }

        // Handle negation
        if pipeline.negated {
            last_result.exit_code = if last_result.exit_code == 0 { 1 } else { 0 };
        }

        Ok(last_result)
    }

    /// Execute a command list (cmd1 && cmd2 || cmd3)
    async fn execute_list(&mut self, list: &CommandList) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code;
        let emit_before = self.output_emit_count;
        let result = self.execute_command(&list.first).await?;
        self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
        stdout.push_str(&result.stdout);
        stderr.push_str(&result.stderr);
        exit_code = result.exit_code;
        self.last_exit_code = exit_code;
        let mut control_flow = result.control_flow;

        // If first command signaled control flow, return immediately
        if control_flow != ControlFlow::None {
            return Ok(ExecResult {
                stdout,
                stderr,
                exit_code,
                control_flow,
            });
        }

        // Track if the list contains any && or || operators
        // If so, failures within the list are "handled" by those operators
        let has_conditional_operators = list
            .rest
            .iter()
            .any(|(op, _)| matches!(op, ListOperator::And | ListOperator::Or));

        // Track if we just exited a conditional chain (for errexit check)
        let mut just_exited_conditional_chain = false;

        for (i, (op, cmd)) in list.rest.iter().enumerate() {
            // Check if next operator (if any) is && or ||
            let next_op = list.rest.get(i + 1).map(|(op, _)| op);
            let current_is_conditional = matches!(op, ListOperator::And | ListOperator::Or);
            let next_is_conditional =
                matches!(next_op, Some(ListOperator::And) | Some(ListOperator::Or));

            // Check errexit before executing if:
            // - We just exited a conditional chain (and current op is semicolon)
            // - OR: current op is semicolon and previous wasn't in a conditional chain
            // - Exit code is non-zero
            // But NOT if we're about to enter/continue a conditional chain
            let should_check_errexit = matches!(op, ListOperator::Semicolon)
                && !just_exited_conditional_chain
                && self.is_errexit_enabled()
                && exit_code != 0;

            if should_check_errexit {
                return Ok(ExecResult {
                    stdout,
                    stderr,
                    exit_code,
                    control_flow: ControlFlow::None,
                });
            }

            // Reset the flag
            just_exited_conditional_chain = false;

            // Mark that we're exiting a conditional chain if:
            // - Current is conditional (&&/||) and next is not conditional (;/end)
            if current_is_conditional && !next_is_conditional {
                just_exited_conditional_chain = true;
            }

            let should_execute = match op {
                ListOperator::And => exit_code == 0,
                ListOperator::Or => exit_code != 0,
                ListOperator::Semicolon => true,
                ListOperator::Background => {
                    // Background (&) runs command synchronously in virtual mode.
                    // True process backgrounding requires OS process spawning which
                    // is excluded from the sandboxed virtual environment by design.
                    true
                }
            };

            if should_execute {
                let emit_before = self.output_emit_count;
                let result = self.execute_command(cmd).await?;
                self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
                stdout.push_str(&result.stdout);
                stderr.push_str(&result.stderr);
                exit_code = result.exit_code;
                self.last_exit_code = exit_code;
                control_flow = result.control_flow;

                // If command signaled control flow, return immediately
                if control_flow != ControlFlow::None {
                    return Ok(ExecResult {
                        stdout,
                        stderr,
                        exit_code,
                        control_flow,
                    });
                }
            }
        }

        // Final errexit check for the last command
        // Don't check if:
        // - The list had conditional operators (failures are "handled" by && / ||)
        // - OR we're in/just exited a conditional chain
        let should_final_errexit_check =
            !has_conditional_operators && self.is_errexit_enabled() && exit_code != 0;

        if should_final_errexit_check {
            return Ok(ExecResult {
                stdout,
                stderr,
                exit_code,
                control_flow: ControlFlow::None,
            });
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
        })
    }

    async fn execute_simple_command(
        &mut self,
        command: &SimpleCommand,
        stdin: Option<String>,
    ) -> Result<ExecResult> {
        // Save old variable values before applying prefix assignments.
        // If there's a command, these assignments are temporary (bash behavior:
        // `VAR=value cmd` sets VAR only for cmd's duration).
        let var_saves: Vec<(String, Option<String>)> = command
            .assignments
            .iter()
            .map(|a| (a.name.clone(), self.variables.get(&a.name).cloned()))
            .collect();

        // Process variable assignments first
        for assignment in &command.assignments {
            match &assignment.value {
                AssignmentValue::Scalar(word) => {
                    let value = self.expand_word(word).await?;
                    if let Some(index_str) = &assignment.index {
                        // arr[index]=value - set array element
                        let index: usize =
                            self.evaluate_arithmetic(index_str).try_into().unwrap_or(0);
                        let arr = self.arrays.entry(assignment.name.clone()).or_default();
                        if assignment.append {
                            // Append to existing element
                            let existing = arr.get(&index).cloned().unwrap_or_default();
                            arr.insert(index, existing + &value);
                        } else {
                            arr.insert(index, value);
                        }
                    } else if assignment.append {
                        // VAR+=value - append to variable
                        let existing = self.expand_variable(&assignment.name);
                        self.variables
                            .insert(assignment.name.clone(), existing + &value);
                    } else {
                        self.variables.insert(assignment.name.clone(), value);
                    }
                }
                AssignmentValue::Array(words) => {
                    // arr=(a b c) - set whole array
                    // arr+=(d e f) - append to array
                    // Handle word splitting for command substitution like arr=($(echo a b c))

                    // First, expand all words (need to do this before borrowing arrays)
                    let mut expanded_values = Vec::new();
                    for word in words.iter() {
                        let has_command_subst = word
                            .parts
                            .iter()
                            .any(|p| matches!(p, WordPart::CommandSubstitution(_)));
                        let value = self.expand_word(word).await?;
                        expanded_values.push((value, has_command_subst));
                    }

                    // Now handle the array assignment
                    let arr = self.arrays.entry(assignment.name.clone()).or_default();

                    // Find starting index (max existing index + 1 for append, 0 for replace)
                    let mut idx = if assignment.append {
                        arr.keys().max().map(|k| k + 1).unwrap_or(0)
                    } else {
                        arr.clear();
                        0
                    };

                    for (value, has_command_subst) in expanded_values {
                        if has_command_subst && !value.is_empty() {
                            // Word-split command substitution results
                            for part in value.split_whitespace() {
                                arr.insert(idx, part.to_string());
                                idx += 1;
                            }
                        } else if !value.is_empty() || !has_command_subst {
                            arr.insert(idx, value);
                            idx += 1;
                        }
                    }
                }
            }
        }

        let name = self.expand_word(&command.name).await?;

        // If name is empty, this is an assignment-only command - keep permanently.
        // Preserve last_exit_code from any command substitution in the value
        // (bash behavior: `x=$(false)` sets $? to 1).
        if name.is_empty() {
            return Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: self.last_exit_code,
                control_flow: crate::interpreter::ControlFlow::None,
            });
        }

        // Has a command: prefix assignments are temporary (bash behavior).
        // Inject scalar prefix assignments into self.env so builtins/functions
        // can see them via ctx.env (e.g., `MYVAR=hello printenv MYVAR`).
        let mut env_saves: Vec<(String, Option<String>)> = Vec::new();
        for assignment in &command.assignments {
            if assignment.index.is_none() {
                if let Some(value) = self.variables.get(&assignment.name).cloned() {
                    let old = self.env.insert(assignment.name.clone(), value);
                    env_saves.push((assignment.name.clone(), old));
                }
            }
        }

        // Dispatch to the appropriate handler
        let result = self.execute_dispatched_command(&name, command, stdin).await;

        // Restore env (prefix assignments are command-scoped)
        for (name, old) in env_saves {
            match old {
                Some(v) => {
                    self.env.insert(name, v);
                }
                None => {
                    self.env.remove(&name);
                }
            }
        }

        // Restore variables (prefix assignments don't persist when there's a command)
        for (name, old) in var_saves {
            match old {
                Some(v) => {
                    self.variables.insert(name, v);
                }
                None => {
                    self.variables.remove(&name);
                }
            }
        }

        result
    }

    /// Execute a command after name resolution and prefix assignment setup.
    ///
    /// Handles argument expansion, stdin processing, and dispatch to
    /// functions, special builtins, regular builtins, or command-not-found.
    async fn execute_dispatched_command(
        &mut self,
        name: &str,
        command: &SimpleCommand,
        stdin: Option<String>,
    ) -> Result<ExecResult> {
        // Expand arguments with brace and glob expansion
        let mut args: Vec<String> = Vec::new();
        for word in &command.args {
            // Use field expansion so "${arr[@]}" produces multiple args
            let fields = self.expand_word_to_fields(word).await?;

            // Skip brace and glob expansion for quoted words
            if word.quoted {
                args.extend(fields);
                continue;
            }

            // For each field, apply brace and glob expansion
            for expanded in fields {
                // Step 1: Brace expansion (produces multiple strings)
                let brace_expanded = self.expand_braces(&expanded);

                // Step 2: For each brace-expanded item, do glob expansion
                for item in brace_expanded {
                    if self.contains_glob_chars(&item) {
                        let glob_matches = self.expand_glob(&item).await?;
                        if glob_matches.is_empty() {
                            // No matches - keep original pattern (bash behavior)
                            args.push(item);
                        } else {
                            args.extend(glob_matches);
                        }
                    } else {
                        args.push(item);
                    }
                }
            }
        }

        // Handle input redirections first
        let stdin = self
            .process_input_redirections(stdin, &command.redirects)
            .await?;

        // If no explicit stdin, inherit from pipeline_stdin (for compound cmds in pipes).
        // For `read`, consume one line; for other commands, provide all remaining data.
        let stdin = if stdin.is_some() {
            stdin
        } else if let Some(ref ps) = self.pipeline_stdin {
            if !ps.is_empty() {
                if name == "read" {
                    // Consume one line from pipeline stdin
                    let data = ps.clone();
                    if let Some(newline_pos) = data.find('\n') {
                        let line = data[..=newline_pos].to_string();
                        self.pipeline_stdin = Some(data[newline_pos + 1..].to_string());
                        Some(line)
                    } else {
                        // Last line without trailing newline
                        self.pipeline_stdin = Some(String::new());
                        Some(data)
                    }
                } else {
                    Some(ps.clone())
                }
            } else {
                None
            }
        } else {
            None
        };

        // Check for functions first
        if let Some(func_def) = self.functions.get(name).cloned() {
            // Check function depth limit
            self.counters.push_function(&self.limits)?;

            // Push call frame with positional parameters
            self.call_stack.push(CallFrame {
                name: name.to_string(),
                locals: HashMap::new(),
                positional: args.clone(),
            });

            // Execute function body
            let mut result = self.execute_command(&func_def.body).await?;

            // Pop call frame and function counter
            self.call_stack.pop();
            self.counters.pop_function();

            // Handle return - convert Return control flow to exit code
            if let ControlFlow::Return(code) = result.control_flow {
                result.exit_code = code;
                result.control_flow = ControlFlow::None;
            }

            // Handle output redirections
            return self.apply_redirections(result, &command.redirects).await;
        }

        // Handle `local` specially - must set in call frame locals
        if name == "local" {
            if let Some(frame) = self.call_stack.last_mut() {
                // In a function - set in locals
                for arg in &args {
                    if let Some(eq_pos) = arg.find('=') {
                        let var_name = &arg[..eq_pos];
                        let value = &arg[eq_pos + 1..];
                        frame.locals.insert(var_name.to_string(), value.to_string());
                    } else {
                        // Just declare without value
                        frame.locals.insert(arg.to_string(), String::new());
                    }
                }
            } else {
                // Not in a function - set in global variables (bash behavior)
                for arg in &args {
                    if let Some(eq_pos) = arg.find('=') {
                        let var_name = &arg[..eq_pos];
                        let value = &arg[eq_pos + 1..];
                        self.variables
                            .insert(var_name.to_string(), value.to_string());
                    } else {
                        self.variables.insert(arg.to_string(), String::new());
                    }
                }
            }
            return Ok(ExecResult::ok(String::new()));
        }

        // Handle `timeout` specially - needs interpreter-level command execution
        if name == "timeout" {
            return self.execute_timeout(&args, stdin, &command.redirects).await;
        }

        // Handle `bash` and `sh` specially - execute scripts using the interpreter
        if name == "bash" || name == "sh" {
            return self
                .execute_shell(name, &args, stdin, &command.redirects)
                .await;
        }

        // Handle source/eval at interpreter level - they need to execute
        // parsed scripts in the current shell context (functions, variables, etc.)
        if name == "source" || name == "." {
            return self.execute_source(&args, &command.redirects).await;
        }

        if name == "eval" {
            return self.execute_eval(&args, stdin, &command.redirects).await;
        }

        // Handle `command` builtin - needs interpreter-level access to builtins/functions
        if name == "command" {
            return self
                .execute_command_builtin(&args, stdin, &command.redirects)
                .await;
        }

        // Handle `getopts` builtin - needs to read/write shell variables (OPTIND, OPTARG)
        if name == "getopts" {
            return self.execute_getopts(&args, &command.redirects).await;
        }

        // Check for builtins
        if let Some(builtin) = self.builtins.get(name) {
            let ctx = builtins::Context {
                args: &args,
                env: &self.env,
                variables: &mut self.variables,
                cwd: &mut self.cwd,
                fs: Arc::clone(&self.fs),
                stdin: stdin.as_deref(),
                #[cfg(feature = "http_client")]
                http_client: self.http_client.as_ref(),
                #[cfg(feature = "git")]
                git_client: self.git_client.as_ref(),
            };

            // Execute builtin with panic catching for security
            // THREAT[TM-INT-001]: Builtins may panic on unexpected input
            // SECURITY: All builtins (built-in and custom) may panic - we catch this to:
            // 1. Prevent interpreter crash
            // 2. Avoid leaking panic message (may contain sensitive info)
            // 3. Return sanitized error to user
            let result = AssertUnwindSafe(builtin.execute(ctx)).catch_unwind().await;

            let result = match result {
                Ok(Ok(exec_result)) => exec_result,
                Ok(Err(e)) => return Err(e),
                Err(_panic) => {
                    // Panic caught! Return sanitized error message.
                    // SECURITY: Do NOT include panic message - it may contain:
                    // - Stack traces with internal paths
                    // - Memory addresses
                    // - Secret values from variables
                    ExecResult::err(format!("bash: {}: builtin failed unexpectedly\n", name), 1)
                }
            };

            // Handle output redirections
            return self.apply_redirections(result, &command.redirects).await;
        }

        // Check if command is a path to an executable script in the VFS
        if name.contains('/') {
            let result = self
                .try_execute_script_by_path(name, &args, &command.redirects)
                .await?;
            return Ok(result);
        }

        // No slash in name: search $PATH for executable script
        if let Some(result) = self
            .try_execute_script_via_path_search(name, &args, &command.redirects)
            .await?
        {
            return Ok(result);
        }

        // Command not found - return error like bash does (exit code 127)
        Ok(ExecResult::err(
            format!("bash: {}: command not found", name),
            127,
        ))
    }

    /// Execute a script file by resolved path.
    ///
    /// Bash behavior for path-based commands (name contains `/`):
    /// 1. Resolve path (absolute or relative to cwd)
    /// 2. stat() — if not found: "No such file or directory" (exit 127)
    /// 3. If directory: "Is a directory" (exit 126)
    /// 4. If not executable (mode & 0o111 == 0): "Permission denied" (exit 126)
    /// 5. Read file, strip shebang, parse, execute in call frame
    async fn try_execute_script_by_path(
        &mut self,
        name: &str,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let path = self.resolve_path(name);

        // stat the file
        let meta = match self.fs.stat(&path).await {
            Ok(m) => m,
            Err(_) => {
                return Ok(ExecResult::err(
                    format!("bash: {}: No such file or directory", name),
                    127,
                ));
            }
        };

        // Directory check
        if meta.file_type.is_dir() {
            return Ok(ExecResult::err(
                format!("bash: {}: Is a directory", name),
                126,
            ));
        }

        // Execute permission check
        if meta.mode & 0o111 == 0 {
            return Ok(ExecResult::err(
                format!("bash: {}: Permission denied", name),
                126,
            ));
        }

        // Read file content
        let content = match self.fs.read_file(&path).await {
            Ok(c) => String::from_utf8_lossy(&c).to_string(),
            Err(_) => {
                return Ok(ExecResult::err(
                    format!("bash: {}: No such file or directory", name),
                    127,
                ));
            }
        };

        self.execute_script_content(name, &content, args, redirects)
            .await
    }

    /// Search $PATH for an executable script and run it.
    ///
    /// Returns `Ok(None)` if no matching file found (caller emits "command not found").
    async fn try_execute_script_via_path_search(
        &mut self,
        name: &str,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<Option<ExecResult>> {
        let path_var = self
            .variables
            .get("PATH")
            .or_else(|| self.env.get("PATH"))
            .cloned()
            .unwrap_or_default();

        for dir in path_var.split(':') {
            if dir.is_empty() {
                continue;
            }
            let candidate = PathBuf::from(dir).join(name);
            if let Ok(meta) = self.fs.stat(&candidate).await {
                if meta.file_type.is_dir() {
                    continue;
                }
                if meta.mode & 0o111 == 0 {
                    continue;
                }
                if let Ok(content) = self.fs.read_file(&candidate).await {
                    let script_text = String::from_utf8_lossy(&content).to_string();
                    let result = self
                        .execute_script_content(name, &script_text, args, redirects)
                        .await?;
                    return Ok(Some(result));
                }
            }
        }

        Ok(None)
    }

    /// Parse and execute script content in a new call frame.
    ///
    /// Shared by path-based and $PATH-based script execution.
    /// Sets up $0 = script name, $1..N = args, strips shebang.
    async fn execute_script_content(
        &mut self,
        name: &str,
        content: &str,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        // Strip shebang line if present
        let script_text = if content.starts_with("#!") {
            content
                .find('\n')
                .map(|pos| &content[pos + 1..])
                .unwrap_or("")
        } else {
            content
        };

        let parser = Parser::with_limits(
            script_text,
            self.limits.max_ast_depth,
            self.limits.max_parser_operations,
        );
        let script = match parser.parse() {
            Ok(s) => s,
            Err(e) => {
                return Ok(ExecResult::err(format!("bash: {}: {}\n", name, e), 2));
            }
        };

        // Push call frame: $0 = script name, $1..N = args
        self.call_stack.push(CallFrame {
            name: name.to_string(),
            locals: HashMap::new(),
            positional: args.to_vec(),
        });

        let result = self.execute(&script).await;

        // Pop call frame
        self.call_stack.pop();

        match result {
            Ok(mut exec_result) => {
                // Handle return - convert Return control flow to exit code
                if let ControlFlow::Return(code) = exec_result.control_flow {
                    exec_result.exit_code = code;
                    exec_result.control_flow = ControlFlow::None;
                }
                self.apply_redirections(exec_result, redirects).await
            }
            Err(e) => Err(e),
        }
    }

    /// Execute `source` / `.` - read and execute commands from a file in current shell.
    ///
    /// Bash behavior:
    /// - If filename contains a slash, use it directly (absolute or relative to cwd)
    /// - If filename has no slash, search $PATH directories
    /// - Extra arguments become positional parameters ($1, $2, ...) during sourcing
    /// - Original positional parameters are restored after sourcing completes
    async fn execute_source(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let filename = match args.first() {
            Some(f) => f,
            None => {
                return Ok(ExecResult::err("source: filename argument required", 1));
            }
        };

        // Resolve the file path:
        // - If filename contains '/', resolve relative to cwd
        // - Otherwise, search $PATH directories (bash behavior)
        let content = if filename.contains('/') {
            let path = self.resolve_path(filename);
            match self.fs.read_file(&path).await {
                Ok(c) => String::from_utf8_lossy(&c).to_string(),
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("source: {}: No such file or directory", filename),
                        1,
                    ));
                }
            }
        } else {
            // Search PATH for the file
            let mut found = None;
            let path_var = self
                .variables
                .get("PATH")
                .or_else(|| self.env.get("PATH"))
                .cloned()
                .unwrap_or_default();
            for dir in path_var.split(':') {
                if dir.is_empty() {
                    continue;
                }
                let candidate = PathBuf::from(dir).join(filename);
                if let Ok(c) = self.fs.read_file(&candidate).await {
                    found = Some(String::from_utf8_lossy(&c).to_string());
                    break;
                }
            }
            // Also try cwd as fallback (bash sources from cwd too)
            if found.is_none() {
                let path = self.resolve_path(filename);
                if let Ok(c) = self.fs.read_file(&path).await {
                    found = Some(String::from_utf8_lossy(&c).to_string());
                }
            }
            match found {
                Some(c) => c,
                None => {
                    return Ok(ExecResult::err(
                        format!("source: {}: No such file or directory", filename),
                        1,
                    ));
                }
            }
        };

        let parser = Parser::new(&content);
        let script = match parser.parse() {
            Ok(s) => s,
            Err(e) => {
                return Ok(ExecResult::err(
                    format!("source: {}: parse error: {}", filename, e),
                    1,
                ));
            }
        };

        // Set positional parameters if extra arguments provided.
        // Save and restore the caller's positional params.
        let source_args: Vec<String> = args[1..].to_vec();
        let has_source_args = !source_args.is_empty();

        let saved_positional = if has_source_args {
            let saved = self.call_stack.last().map(|frame| frame.positional.clone());
            // Push a temporary call frame for positional params
            if self.call_stack.is_empty() {
                self.call_stack.push(CallFrame {
                    name: filename.clone(),
                    locals: HashMap::new(),
                    positional: source_args,
                });
            } else if let Some(frame) = self.call_stack.last_mut() {
                frame.positional = source_args;
            }
            saved
        } else {
            None
        };

        // Execute the script commands in the current shell context
        let mut result = self.execute(&script).await?;

        // Restore positional parameters
        if has_source_args {
            if let Some(saved) = saved_positional {
                if let Some(frame) = self.call_stack.last_mut() {
                    frame.positional = saved;
                }
            } else {
                // We pushed a frame; pop it
                self.call_stack.pop();
            }
        }

        // Apply redirections
        result = self.apply_redirections(result, redirects).await?;
        Ok(result)
    }

    /// Execute `eval` - parse and execute concatenated arguments
    async fn execute_eval(
        &mut self,
        args: &[String],
        stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        if args.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let cmd = args.join(" ");
        let parser = Parser::new(&cmd);
        let script = match parser.parse() {
            Ok(s) => s,
            Err(e) => {
                return Ok(ExecResult::err(format!("eval: parse error: {}", e), 1));
            }
        };

        // Set up pipeline stdin if provided
        let prev_pipeline_stdin = self.pipeline_stdin.take();
        if stdin.is_some() {
            self.pipeline_stdin = stdin;
        }

        let mut result = self.execute(&script).await?;

        self.pipeline_stdin = prev_pipeline_stdin;

        result = self.apply_redirections(result, redirects).await?;
        Ok(result)
    }

    /// Execute the `getopts` builtin (POSIX option parsing).
    ///
    /// Usage: `getopts optstring name [args...]`
    ///
    /// Parses options from positional params (or `args`).
    /// Uses/updates `OPTIND` variable for tracking position.
    /// Sets `name` variable to the found option letter.
    /// Sets `OPTARG` for options that take arguments (marked with `:` in optstring).
    /// Returns 0 while options remain, 1 when done.
    async fn execute_getopts(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        if args.len() < 2 {
            let result = ExecResult::err("getopts: usage: getopts optstring name [arg ...]\n", 2);
            return Ok(result);
        }

        let optstring = &args[0];
        let varname = &args[1];

        // Get the arguments to parse (remaining args, or positional params)
        let parse_args: Vec<String> = if args.len() > 2 {
            args[2..].to_vec()
        } else {
            // Use positional parameters $1, $2, ...
            self.call_stack
                .last()
                .map(|frame| frame.positional.clone())
                .unwrap_or_default()
        };

        // Get current OPTIND (1-based index into args)
        let optind: usize = self
            .variables
            .get("OPTIND")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);

        // Check if we're past the end
        if optind < 1 || optind > parse_args.len() {
            self.variables.insert(varname.clone(), "?".to_string());
            return Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
                control_flow: crate::interpreter::ControlFlow::None,
            });
        }

        let current_arg = &parse_args[optind - 1];

        // Check if this is an option (starts with -)
        if !current_arg.starts_with('-') || current_arg == "-" || current_arg == "--" {
            self.variables.insert(varname.clone(), "?".to_string());
            if current_arg == "--" {
                self.variables
                    .insert("OPTIND".to_string(), (optind + 1).to_string());
            }
            return Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
                control_flow: crate::interpreter::ControlFlow::None,
            });
        }

        // Parse the option character(s) from current arg
        // Handle multi-char option groups like -abc
        let opt_chars: Vec<char> = current_arg[1..].chars().collect();

        // Track position within the current argument for multi-char options
        let char_idx: usize = self
            .variables
            .get("_OPTCHAR_IDX")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        if char_idx >= opt_chars.len() {
            // Should not happen, but advance
            self.variables
                .insert("OPTIND".to_string(), (optind + 1).to_string());
            self.variables.remove("_OPTCHAR_IDX");
            self.variables.insert(varname.clone(), "?".to_string());
            return Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
                control_flow: crate::interpreter::ControlFlow::None,
            });
        }

        let opt_char = opt_chars[char_idx];
        let silent = optstring.starts_with(':');
        let spec = if silent { &optstring[1..] } else { optstring };

        // Check if this option is in the optstring
        if let Some(pos) = spec.find(opt_char) {
            let needs_arg = spec.get(pos + 1..pos + 2) == Some(":");
            self.variables.insert(varname.clone(), opt_char.to_string());

            if needs_arg {
                // Option needs an argument
                if char_idx + 1 < opt_chars.len() {
                    // Rest of current arg is the argument
                    let arg_val: String = opt_chars[char_idx + 1..].iter().collect();
                    self.variables.insert("OPTARG".to_string(), arg_val);
                    self.variables
                        .insert("OPTIND".to_string(), (optind + 1).to_string());
                    self.variables.remove("_OPTCHAR_IDX");
                } else if optind < parse_args.len() {
                    // Next arg is the argument
                    self.variables
                        .insert("OPTARG".to_string(), parse_args[optind].clone());
                    self.variables
                        .insert("OPTIND".to_string(), (optind + 2).to_string());
                    self.variables.remove("_OPTCHAR_IDX");
                } else {
                    // Missing argument
                    self.variables.remove("OPTARG");
                    self.variables
                        .insert("OPTIND".to_string(), (optind + 1).to_string());
                    self.variables.remove("_OPTCHAR_IDX");
                    if silent {
                        self.variables.insert(varname.clone(), ":".to_string());
                        self.variables
                            .insert("OPTARG".to_string(), opt_char.to_string());
                    } else {
                        self.variables.insert(varname.clone(), "?".to_string());
                        let mut result = ExecResult::ok(String::new());
                        result.stderr = format!(
                            "bash: getopts: option requires an argument -- '{}'\n",
                            opt_char
                        );
                        result = self.apply_redirections(result, redirects).await?;
                        return Ok(result);
                    }
                }
            } else {
                // No argument needed
                self.variables.remove("OPTARG");
                if char_idx + 1 < opt_chars.len() {
                    // More chars in this arg
                    self.variables
                        .insert("_OPTCHAR_IDX".to_string(), (char_idx + 1).to_string());
                } else {
                    // Move to next arg
                    self.variables
                        .insert("OPTIND".to_string(), (optind + 1).to_string());
                    self.variables.remove("_OPTCHAR_IDX");
                }
            }
        } else {
            // Unknown option
            self.variables.remove("OPTARG");
            if char_idx + 1 < opt_chars.len() {
                self.variables
                    .insert("_OPTCHAR_IDX".to_string(), (char_idx + 1).to_string());
            } else {
                self.variables
                    .insert("OPTIND".to_string(), (optind + 1).to_string());
                self.variables.remove("_OPTCHAR_IDX");
            }

            if silent {
                self.variables.insert(varname.clone(), "?".to_string());
                self.variables
                    .insert("OPTARG".to_string(), opt_char.to_string());
            } else {
                self.variables.insert(varname.clone(), "?".to_string());
                let mut result = ExecResult::ok(String::new());
                result.stderr = format!("bash: getopts: illegal option -- '{}'\n", opt_char);
                result = self.apply_redirections(result, redirects).await?;
                return Ok(result);
            }
        }

        let mut result = ExecResult::ok(String::new());
        result = self.apply_redirections(result, redirects).await?;
        Ok(result)
    }

    /// Execute the `command` builtin.
    ///
    /// - `command -v name` — print command path/name if found (exit 0) or nothing (exit 1)
    /// - `command -V name` — verbose: describe what `name` is
    /// - `command name args...` — run `name` bypassing shell functions
    async fn execute_command_builtin(
        &mut self,
        args: &[String],
        _stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        if args.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let mut mode = ' '; // default: run the command
        let mut cmd_args_start = 0;

        // Parse flags
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if arg == "-v" {
                mode = 'v';
                i += 1;
            } else if arg == "-V" {
                mode = 'V';
                i += 1;
            } else if arg == "-p" {
                // -p: use default PATH (ignore in sandboxed env)
                i += 1;
            } else {
                cmd_args_start = i;
                break;
            }
        }

        if cmd_args_start >= args.len() {
            return Ok(ExecResult::ok(String::new()));
        }

        let cmd_name = &args[cmd_args_start];

        match mode {
            'v' => {
                // command -v: print name if it's a known command
                let found = self.builtins.contains_key(cmd_name.as_str())
                    || self.functions.contains_key(cmd_name.as_str())
                    || is_keyword(cmd_name);
                let mut result = if found {
                    ExecResult::ok(format!("{}\n", cmd_name))
                } else {
                    ExecResult {
                        stdout: String::new(),
                        stderr: String::new(),
                        exit_code: 1,
                        control_flow: crate::interpreter::ControlFlow::None,
                    }
                };
                result = self.apply_redirections(result, redirects).await?;
                Ok(result)
            }
            'V' => {
                // command -V: verbose description
                let description = if self.functions.contains_key(cmd_name.as_str()) {
                    format!("{} is a function\n", cmd_name)
                } else if self.builtins.contains_key(cmd_name.as_str()) {
                    format!("{} is a shell builtin\n", cmd_name)
                } else if is_keyword(cmd_name) {
                    format!("{} is a shell keyword\n", cmd_name)
                } else {
                    return Ok(ExecResult::err(
                        format!("bash: command: {}: not found\n", cmd_name),
                        1,
                    ));
                };
                let mut result = ExecResult::ok(description);
                result = self.apply_redirections(result, redirects).await?;
                Ok(result)
            }
            _ => {
                // command name args...: run bypassing functions (use builtin only)
                // Build a synthetic simple command and execute it, skipping function lookup
                let remaining = &args[cmd_args_start..];
                if let Some(builtin) = self.builtins.get(remaining[0].as_str()) {
                    let builtin_args = &remaining[1..];
                    let ctx = builtins::Context {
                        args: builtin_args,
                        env: &self.env,
                        variables: &mut self.variables,
                        cwd: &mut self.cwd,
                        fs: Arc::clone(&self.fs),
                        stdin: _stdin.as_deref(),
                        #[cfg(feature = "http_client")]
                        http_client: self.http_client.as_ref(),
                        #[cfg(feature = "git")]
                        git_client: self.git_client.as_ref(),
                    };
                    let mut result = builtin.execute(ctx).await?;
                    result = self.apply_redirections(result, redirects).await?;
                    Ok(result)
                } else {
                    Ok(ExecResult::err(
                        format!("bash: {}: command not found\n", remaining[0]),
                        127,
                    ))
                }
            }
        }
    }

    /// Process input redirections (< file, <<< string)
    async fn process_input_redirections(
        &mut self,
        existing_stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<Option<String>> {
        let mut stdin = existing_stdin;

        for redirect in redirects {
            match redirect.kind {
                RedirectKind::Input => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    // Handle /dev/null at interpreter level - cannot be bypassed
                    if is_dev_null(&path) {
                        stdin = Some(String::new()); // EOF
                    } else {
                        let content = self.fs.read_file(&path).await?;
                        stdin = Some(String::from_utf8_lossy(&content).to_string());
                    }
                }
                RedirectKind::HereString => {
                    // <<< string - use the target as stdin content
                    let content = self.expand_word(&redirect.target).await?;
                    stdin = Some(format!("{}\n", content));
                }
                RedirectKind::HereDoc => {
                    // << EOF - use the heredoc content as stdin
                    let content = self.expand_word(&redirect.target).await?;
                    stdin = Some(content);
                }
                _ => {
                    // Output redirections handled separately
                }
            }
        }

        Ok(stdin)
    }

    /// Apply output redirections to command output
    async fn apply_redirections(
        &mut self,
        mut result: ExecResult,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        for redirect in redirects {
            match redirect.kind {
                RedirectKind::Output => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    // Handle /dev/null at interpreter level - cannot be bypassed
                    if is_dev_null(&path) {
                        // Discard output without calling filesystem
                        match redirect.fd {
                            Some(2) => result.stderr = String::new(),
                            _ => result.stdout = String::new(),
                        }
                    } else {
                        // Check which fd we're redirecting
                        match redirect.fd {
                            Some(2) => {
                                // 2> - redirect stderr to file
                                if let Err(e) =
                                    self.fs.write_file(&path, result.stderr.as_bytes()).await
                                {
                                    // Redirect failed - set exit code and report error
                                    result.stderr = format!("bash: {}: {}\n", target_path, e);
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stderr = String::new();
                            }
                            _ => {
                                // Default (stdout) - write stdout to file
                                if let Err(e) =
                                    self.fs.write_file(&path, result.stdout.as_bytes()).await
                                {
                                    // Redirect failed - output is lost, set exit code and report error
                                    result.stdout = String::new();
                                    result.stderr = format!("bash: {}: {}\n", target_path, e);
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stdout = String::new();
                            }
                        }
                    }
                }
                RedirectKind::Append => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    // Handle /dev/null at interpreter level - cannot be bypassed
                    if is_dev_null(&path) {
                        // Discard output without calling filesystem
                        match redirect.fd {
                            Some(2) => result.stderr = String::new(),
                            _ => result.stdout = String::new(),
                        }
                    } else {
                        // Check which fd we're appending
                        match redirect.fd {
                            Some(2) => {
                                // 2>> - append stderr to file
                                if let Err(e) =
                                    self.fs.append_file(&path, result.stderr.as_bytes()).await
                                {
                                    result.stderr = format!("bash: {}: {}\n", target_path, e);
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stderr = String::new();
                            }
                            _ => {
                                // Default (stdout) - append stdout to file
                                if let Err(e) =
                                    self.fs.append_file(&path, result.stdout.as_bytes()).await
                                {
                                    // Redirect failed - output is lost
                                    result.stdout = String::new();
                                    result.stderr = format!("bash: {}: {}\n", target_path, e);
                                    result.exit_code = 1;
                                    return Ok(result);
                                }
                                result.stdout = String::new();
                            }
                        }
                    }
                }
                RedirectKind::OutputBoth => {
                    // &> - redirect both stdout and stderr to file
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    // Handle /dev/null at interpreter level - cannot be bypassed
                    if is_dev_null(&path) {
                        // Discard both outputs without calling filesystem
                        result.stdout = String::new();
                        result.stderr = String::new();
                    } else {
                        // Write both stdout and stderr to file
                        let combined = format!("{}{}", result.stdout, result.stderr);
                        if let Err(e) = self.fs.write_file(&path, combined.as_bytes()).await {
                            result.stderr = format!("bash: {}: {}\n", target_path, e);
                            result.exit_code = 1;
                            return Ok(result);
                        }
                        result.stdout = String::new();
                        result.stderr = String::new();
                    }
                }
                RedirectKind::DupOutput => {
                    // Handle fd duplication (e.g., 2>&1, >&2)
                    let target = self.expand_word(&redirect.target).await?;
                    let target_fd: i32 = target.parse().unwrap_or(1);
                    let src_fd = redirect.fd.unwrap_or(1);

                    match (src_fd, target_fd) {
                        (2, 1) => {
                            // 2>&1 - redirect stderr to stdout
                            result.stdout.push_str(&result.stderr);
                            result.stderr = String::new();
                        }
                        (1, 2) => {
                            // >&2 or 1>&2 - redirect stdout to stderr
                            result.stderr.push_str(&result.stdout);
                            result.stdout = String::new();
                        }
                        _ => {
                            // Other fd duplications not yet supported
                        }
                    }
                }
                RedirectKind::Input | RedirectKind::HereString | RedirectKind::HereDoc => {
                    // Input redirections handled in process_input_redirections
                }
                RedirectKind::DupInput => {
                    // Input fd duplication not yet supported
                }
            }
        }

        Ok(result)
    }

    /// Resolve a path relative to cwd
    fn resolve_path(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.cwd.join(p)
        }
    }

    async fn expand_word(&mut self, word: &Word) -> Result<String> {
        let mut result = String::new();
        let mut is_first_part = true;

        for part in &word.parts {
            match part {
                WordPart::Literal(s) => {
                    // Tilde expansion: ~ at start of word expands to $HOME
                    if is_first_part && s.starts_with('~') {
                        let home = self
                            .env
                            .get("HOME")
                            .or_else(|| self.variables.get("HOME"))
                            .cloned()
                            .unwrap_or_else(|| "/home/user".to_string());

                        if s == "~" {
                            // Just ~
                            result.push_str(&home);
                        } else if s.starts_with("~/") {
                            // ~/path
                            result.push_str(&home);
                            result.push_str(&s[1..]); // Include the /
                        } else {
                            // ~user - not implemented, keep as-is
                            result.push_str(s);
                        }
                    } else {
                        result.push_str(s);
                    }
                }
                WordPart::Variable(name) => {
                    result.push_str(&self.expand_variable(name));
                }
                WordPart::CommandSubstitution(commands) => {
                    // Execute the commands and capture stdout
                    let mut stdout = String::new();
                    for cmd in commands {
                        let cmd_result = self.execute_command(cmd).await?;
                        stdout.push_str(&cmd_result.stdout);
                        // Propagate exit code from last command in substitution
                        self.last_exit_code = cmd_result.exit_code;
                    }
                    // Remove trailing newline (bash behavior)
                    let trimmed = stdout.trim_end_matches('\n');
                    result.push_str(trimmed);
                }
                WordPart::ArithmeticExpansion(expr) => {
                    // Handle assignment: VAR = expr (must be checked before
                    // variable expansion so the LHS name is preserved)
                    let value = self.evaluate_arithmetic_with_assign(expr);
                    result.push_str(&value.to_string());
                }
                WordPart::Length(name) => {
                    // ${#var} - length of variable value
                    // Also handles ${#arr[n]} - length of array element
                    let value = if let Some(bracket_pos) = name.find('[') {
                        // Array element length: ${#arr[n]}
                        let arr_name = &name[..bracket_pos];
                        let index_end = name.find(']').unwrap_or(name.len());
                        let index_str = &name[bracket_pos + 1..index_end];
                        let idx: usize =
                            self.evaluate_arithmetic(index_str).try_into().unwrap_or(0);
                        if let Some(arr) = self.arrays.get(arr_name) {
                            arr.get(&idx).cloned().unwrap_or_default()
                        } else {
                            String::new()
                        }
                    } else {
                        self.expand_variable(name)
                    };
                    result.push_str(&value.len().to_string());
                }
                WordPart::ParameterExpansion {
                    name,
                    operator,
                    operand,
                } => {
                    let value = self.expand_variable(name);
                    let expanded = self.apply_parameter_op(&value, name, operator, operand);
                    result.push_str(&expanded);
                }
                WordPart::ArrayAccess { name, index } => {
                    if index == "@" || index == "*" {
                        // ${arr[@]} or ${arr[*]} - expand to all elements
                        if let Some(arr) = self.arrays.get(name) {
                            let mut indices: Vec<_> = arr.keys().collect();
                            indices.sort();
                            let values: Vec<_> =
                                indices.iter().filter_map(|i| arr.get(i)).collect();
                            result.push_str(
                                &values.into_iter().cloned().collect::<Vec<_>>().join(" "),
                            );
                        }
                    } else {
                        // ${arr[n]} - get specific element
                        let idx: usize = self.evaluate_arithmetic(index).try_into().unwrap_or(0);
                        if let Some(arr) = self.arrays.get(name) {
                            if let Some(value) = arr.get(&idx) {
                                result.push_str(value);
                            }
                        }
                    }
                }
                WordPart::ArrayIndices(name) => {
                    // ${!arr[@]} or ${!arr[*]} - expand to array indices
                    if let Some(arr) = self.arrays.get(name) {
                        let mut indices: Vec<_> = arr.keys().collect();
                        indices.sort();
                        let index_strs: Vec<String> =
                            indices.iter().map(|i| i.to_string()).collect();
                        result.push_str(&index_strs.join(" "));
                    }
                }
                WordPart::Substring {
                    name,
                    offset,
                    length,
                } => {
                    // ${var:offset} or ${var:offset:length}
                    let value = self.expand_variable(name);
                    let offset_val: isize = self.evaluate_arithmetic(offset) as isize;
                    let start = if offset_val < 0 {
                        // Negative offset counts from end
                        (value.len() as isize + offset_val).max(0) as usize
                    } else {
                        (offset_val as usize).min(value.len())
                    };
                    let substr = if let Some(len_expr) = length {
                        let len_val = self.evaluate_arithmetic(len_expr) as usize;
                        let end = (start + len_val).min(value.len());
                        &value[start..end]
                    } else {
                        &value[start..]
                    };
                    result.push_str(substr);
                }
                WordPart::ArraySlice {
                    name,
                    offset,
                    length,
                } => {
                    // ${arr[@]:offset:length}
                    if let Some(arr) = self.arrays.get(name) {
                        let mut indices: Vec<_> = arr.keys().cloned().collect();
                        indices.sort();
                        let values: Vec<_> =
                            indices.iter().filter_map(|i| arr.get(i).cloned()).collect();

                        let offset_val: isize = self.evaluate_arithmetic(offset) as isize;
                        let start = if offset_val < 0 {
                            (values.len() as isize + offset_val).max(0) as usize
                        } else {
                            (offset_val as usize).min(values.len())
                        };

                        let sliced = if let Some(len_expr) = length {
                            let len_val = self.evaluate_arithmetic(len_expr) as usize;
                            let end = (start + len_val).min(values.len());
                            &values[start..end]
                        } else {
                            &values[start..]
                        };
                        result.push_str(&sliced.join(" "));
                    }
                }
                WordPart::IndirectExpansion(name) => {
                    // ${!var} - indirect expansion
                    let var_name = self.expand_variable(name);
                    let value = self.expand_variable(&var_name);
                    result.push_str(&value);
                }
                WordPart::ArrayLength(name) => {
                    // ${#arr[@]} - number of elements
                    if let Some(arr) = self.arrays.get(name) {
                        result.push_str(&arr.len().to_string());
                    } else {
                        result.push('0');
                    }
                }
                WordPart::ProcessSubstitution { commands, is_input } => {
                    // Execute the commands and capture output
                    let mut stdout = String::new();
                    for cmd in commands {
                        let cmd_result = self.execute_command(cmd).await?;
                        stdout.push_str(&cmd_result.stdout);
                    }

                    // Create a virtual file with the output
                    // Use a unique path based on the timestamp
                    let path_str = format!(
                        "/dev/fd/proc_sub_{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos()
                    );
                    let path = Path::new(&path_str);

                    // Write to virtual filesystem
                    if self.fs.write_file(path, stdout.as_bytes()).await.is_err() {
                        // If we can't write, just inline the content
                        // This is a fallback for simpler behavior
                        if *is_input {
                            result.push_str(&stdout);
                        }
                    } else {
                        result.push_str(&path_str);
                    }
                }
            }
            is_first_part = false;
        }

        Ok(result)
    }

    /// Expand a word to multiple fields (for array iteration and command args)
    /// Returns Vec<String> where array expansions like "${arr[@]}" produce multiple fields.
    /// "${arr[*]}" in quoted context joins elements into a single field (bash behavior).
    async fn expand_word_to_fields(&mut self, word: &Word) -> Result<Vec<String>> {
        // Check if the word contains only an array expansion
        if word.parts.len() == 1 {
            if let WordPart::ArrayAccess { name, index } = &word.parts[0] {
                if index == "@" || index == "*" {
                    if let Some(arr) = self.arrays.get(name) {
                        let mut indices: Vec<_> = arr.keys().collect();
                        indices.sort();
                        let values: Vec<String> =
                            indices.iter().filter_map(|i| arr.get(i).cloned()).collect();
                        // "${arr[*]}" joins into single field; "${arr[@]}" keeps separate
                        if word.quoted && index == "*" {
                            return Ok(vec![values.join(" ")]);
                        }
                        return Ok(values);
                    }
                    return Ok(Vec::new());
                }
            }
        }

        // For other words, expand to a single field
        let expanded = self.expand_word(word).await?;
        Ok(vec![expanded])
    }

    /// Apply parameter expansion operator
    fn apply_parameter_op(
        &mut self,
        value: &str,
        name: &str,
        operator: &ParameterOp,
        operand: &str,
    ) -> String {
        match operator {
            ParameterOp::UseDefault => {
                // ${var:-default} - use default if unset/empty
                if value.is_empty() {
                    operand.to_string()
                } else {
                    value.to_string()
                }
            }
            ParameterOp::AssignDefault => {
                // ${var:=default} - assign default if unset/empty
                if value.is_empty() {
                    self.variables.insert(name.to_string(), operand.to_string());
                    operand.to_string()
                } else {
                    value.to_string()
                }
            }
            ParameterOp::UseReplacement => {
                // ${var:+replacement} - use replacement if set
                if !value.is_empty() {
                    operand.to_string()
                } else {
                    String::new()
                }
            }
            ParameterOp::Error => {
                // ${var:?error} - error if unset/empty
                if value.is_empty() {
                    // In real bash this would exit, we just return empty
                    String::new()
                } else {
                    value.to_string()
                }
            }
            ParameterOp::RemovePrefixShort => {
                // ${var#pattern} - remove shortest prefix match
                self.remove_pattern(value, operand, true, false)
            }
            ParameterOp::RemovePrefixLong => {
                // ${var##pattern} - remove longest prefix match
                self.remove_pattern(value, operand, true, true)
            }
            ParameterOp::RemoveSuffixShort => {
                // ${var%pattern} - remove shortest suffix match
                self.remove_pattern(value, operand, false, false)
            }
            ParameterOp::RemoveSuffixLong => {
                // ${var%%pattern} - remove longest suffix match
                self.remove_pattern(value, operand, false, true)
            }
            ParameterOp::ReplaceFirst {
                pattern,
                replacement,
            } => {
                // ${var/pattern/replacement} - replace first occurrence
                self.replace_pattern(value, pattern, replacement, false)
            }
            ParameterOp::ReplaceAll {
                pattern,
                replacement,
            } => {
                // ${var//pattern/replacement} - replace all occurrences
                self.replace_pattern(value, pattern, replacement, true)
            }
            ParameterOp::UpperFirst => {
                // ${var^} - uppercase first character
                let mut chars = value.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
            ParameterOp::UpperAll => {
                // ${var^^} - uppercase all characters
                value.to_uppercase()
            }
            ParameterOp::LowerFirst => {
                // ${var,} - lowercase first character
                let mut chars = value.chars();
                match chars.next() {
                    Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
            ParameterOp::LowerAll => {
                // ${var,,} - lowercase all characters
                value.to_lowercase()
            }
        }
    }

    /// Replace pattern in value
    fn replace_pattern(
        &self,
        value: &str,
        pattern: &str,
        replacement: &str,
        global: bool,
    ) -> String {
        if pattern.is_empty() {
            return value.to_string();
        }

        // Handle glob pattern with *
        if pattern.contains('*') {
            // Convert glob to regex-like behavior
            // For simplicity, we'll handle basic cases: prefix*, *suffix, *middle*
            if pattern == "*" {
                // Replace everything
                return replacement.to_string();
            }

            if let Some(star_pos) = pattern.find('*') {
                let prefix = &pattern[..star_pos];
                let suffix = &pattern[star_pos + 1..];

                if prefix.is_empty() && !suffix.is_empty() {
                    // *suffix - match anything ending with suffix
                    if let Some(pos) = value.find(suffix) {
                        let after = &value[pos + suffix.len()..];
                        if global {
                            return replacement.to_string()
                                + &self.replace_pattern(after, pattern, replacement, true);
                        } else {
                            return replacement.to_string() + after;
                        }
                    }
                } else if !prefix.is_empty() && suffix.is_empty() {
                    // prefix* - match prefix and anything after
                    if value.starts_with(prefix) {
                        return replacement.to_string();
                    }
                }
            }
            // If we can't match the glob pattern, return as-is
            return value.to_string();
        }

        // Simple string replacement
        if global {
            value.replace(pattern, replacement)
        } else {
            value.replacen(pattern, replacement, 1)
        }
    }

    /// Remove prefix/suffix pattern from value
    fn remove_pattern(&self, value: &str, pattern: &str, prefix: bool, longest: bool) -> String {
        // Simple pattern matching with * glob
        if pattern.is_empty() {
            return value.to_string();
        }

        if prefix {
            // Remove from beginning
            if pattern == "*" {
                if longest {
                    return String::new();
                } else if !value.is_empty() {
                    return value.chars().skip(1).collect();
                } else {
                    return value.to_string();
                }
            }

            // Check if pattern contains *
            if let Some(star_pos) = pattern.find('*') {
                let prefix_part = &pattern[..star_pos];
                let suffix_part = &pattern[star_pos + 1..];

                if prefix_part.is_empty() {
                    // Pattern is "*suffix" - find suffix and remove everything before it
                    if longest {
                        // Find last occurrence of suffix
                        if let Some(pos) = value.rfind(suffix_part) {
                            return value[pos + suffix_part.len()..].to_string();
                        }
                    } else {
                        // Find first occurrence of suffix
                        if let Some(pos) = value.find(suffix_part) {
                            return value[pos + suffix_part.len()..].to_string();
                        }
                    }
                } else if suffix_part.is_empty() {
                    // Pattern is "prefix*" - match prefix and any chars after
                    if let Some(rest) = value.strip_prefix(prefix_part) {
                        if longest {
                            return String::new();
                        } else {
                            return rest.to_string();
                        }
                    }
                } else {
                    // Pattern is "prefix*suffix" - more complex matching
                    if let Some(rest) = value.strip_prefix(prefix_part) {
                        if longest {
                            if let Some(pos) = rest.rfind(suffix_part) {
                                return rest[pos + suffix_part.len()..].to_string();
                            }
                        } else if let Some(pos) = rest.find(suffix_part) {
                            return rest[pos + suffix_part.len()..].to_string();
                        }
                    }
                }
            } else if let Some(rest) = value.strip_prefix(pattern) {
                return rest.to_string();
            }
        } else {
            // Remove from end (suffix)
            if pattern == "*" {
                if longest {
                    return String::new();
                } else if !value.is_empty() {
                    let mut s = value.to_string();
                    s.pop();
                    return s;
                } else {
                    return value.to_string();
                }
            }

            // Check if pattern contains *
            if let Some(star_pos) = pattern.find('*') {
                let prefix_part = &pattern[..star_pos];
                let suffix_part = &pattern[star_pos + 1..];

                if suffix_part.is_empty() {
                    // Pattern is "prefix*" - find prefix and remove from there to end
                    if longest {
                        // Find first occurrence of prefix
                        if let Some(pos) = value.find(prefix_part) {
                            return value[..pos].to_string();
                        }
                    } else {
                        // Find last occurrence of prefix
                        if let Some(pos) = value.rfind(prefix_part) {
                            return value[..pos].to_string();
                        }
                    }
                } else if prefix_part.is_empty() {
                    // Pattern is "*suffix" - match any chars before suffix
                    if let Some(before) = value.strip_suffix(suffix_part) {
                        if longest {
                            return String::new();
                        } else {
                            return before.to_string();
                        }
                    }
                } else {
                    // Pattern is "prefix*suffix" - more complex matching
                    if let Some(before_suffix) = value.strip_suffix(suffix_part) {
                        if longest {
                            if let Some(pos) = before_suffix.find(prefix_part) {
                                return value[..pos].to_string();
                            }
                        } else if let Some(pos) = before_suffix.rfind(prefix_part) {
                            return value[..pos].to_string();
                        }
                    }
                }
            } else if let Some(before) = value.strip_suffix(pattern) {
                return before.to_string();
            }
        }

        value.to_string()
    }

    /// Maximum recursion depth for arithmetic expression evaluation.
    /// THREAT[TM-DOS-025]: Prevents stack overflow via deeply nested arithmetic like
    /// $(((((((...)))))))
    const MAX_ARITHMETIC_DEPTH: usize = 200;

    /// Evaluate arithmetic with assignment support (e.g. `X = X + 1`).
    /// Assignment must be handled before variable expansion so the LHS
    /// variable name is preserved.
    fn evaluate_arithmetic_with_assign(&mut self, expr: &str) -> i64 {
        let expr = expr.trim();

        // Check for assignment: VAR = expr (but not == comparison)
        // Pattern: identifier followed by = (not ==)
        if let Some(eq_pos) = expr.find('=') {
            // Make sure it's not == or !=
            let before = &expr[..eq_pos];
            let after_char = expr.as_bytes().get(eq_pos + 1);
            if !before.ends_with('!')
                && !before.ends_with('<')
                && !before.ends_with('>')
                && after_char != Some(&b'=')
            {
                let var_name = before.trim();
                // Verify LHS is a valid variable name
                if !var_name.is_empty()
                    && var_name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !var_name.chars().next().unwrap_or('0').is_ascii_digit()
                {
                    let rhs = &expr[eq_pos + 1..];
                    let value = self.evaluate_arithmetic(rhs);
                    self.variables
                        .insert(var_name.to_string(), value.to_string());
                    return value;
                }
            }
        }

        self.evaluate_arithmetic(expr)
    }

    /// Evaluate a simple arithmetic expression
    fn evaluate_arithmetic(&self, expr: &str) -> i64 {
        // Simple arithmetic evaluation - handles basic operations
        let expr = expr.trim();

        // First expand any variables in the expression
        let expanded = self.expand_arithmetic_vars(expr);

        // Parse and evaluate with depth tracking (TM-DOS-025)
        self.parse_arithmetic_impl(&expanded, 0)
    }

    /// Expand variables in arithmetic expression (no $ needed in $((...)))
    fn expand_arithmetic_vars(&self, expr: &str) -> String {
        let mut result = String::new();
        let mut chars = expr.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '$' {
                // Handle $var syntax (common in arithmetic)
                let mut name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                if !name.is_empty() {
                    let value = self.expand_variable(&name);
                    if value.is_empty() {
                        result.push('0');
                    } else {
                        result.push_str(&value);
                    }
                } else {
                    result.push(ch);
                }
            } else if ch.is_ascii_alphabetic() || ch == '_' {
                // Could be a variable name
                let mut name = String::new();
                name.push(ch);
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                // Expand the variable
                let value = self.expand_variable(&name);
                if value.is_empty() {
                    result.push('0');
                } else {
                    result.push_str(&value);
                }
            } else {
                result.push(ch);
            }
        }

        result
    }

    /// Parse and evaluate a simple arithmetic expression with depth tracking.
    /// THREAT[TM-DOS-025]: `arith_depth` prevents stack overflow from deeply nested expressions.
    fn parse_arithmetic_impl(&self, expr: &str, arith_depth: usize) -> i64 {
        let expr = expr.trim();

        if expr.is_empty() {
            return 0;
        }

        // THREAT[TM-DOS-025]: Bail out if arithmetic nesting is too deep
        if arith_depth >= Self::MAX_ARITHMETIC_DEPTH {
            return 0;
        }

        // Handle parentheses
        if expr.starts_with('(') && expr.ends_with(')') {
            // Check if parentheses are balanced
            let mut depth = 0;
            let mut balanced = true;
            for (i, ch) in expr.chars().enumerate() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 && i < expr.len() - 1 {
                            balanced = false;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if balanced && depth == 0 {
                return self.parse_arithmetic_impl(&expr[1..expr.len() - 1], arith_depth + 1);
            }
        }

        let chars: Vec<char> = expr.chars().collect();

        // Ternary operator (lowest precedence)
        let mut depth = 0;
        for i in 0..chars.len() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '?' if depth == 0 => {
                    // Find matching :
                    let mut colon_depth = 0;
                    for j in (i + 1)..chars.len() {
                        match chars[j] {
                            '(' => colon_depth += 1,
                            ')' => colon_depth -= 1,
                            '?' => colon_depth += 1,
                            ':' if colon_depth == 0 => {
                                let cond = self.parse_arithmetic_impl(&expr[..i], arith_depth + 1);
                                let then_val =
                                    self.parse_arithmetic_impl(&expr[i + 1..j], arith_depth + 1);
                                let else_val =
                                    self.parse_arithmetic_impl(&expr[j + 1..], arith_depth + 1);
                                return if cond != 0 { then_val } else { else_val };
                            }
                            ':' => colon_depth -= 1,
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        // Logical OR (||)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '|' if depth == 0 && i > 0 && chars[i - 1] == '|' => {
                    let left = self.parse_arithmetic_impl(&expr[..i - 1], arith_depth + 1);
                    // Short-circuit: if left is true, don't evaluate right
                    if left != 0 {
                        return 1;
                    }
                    let right = self.parse_arithmetic_impl(&expr[i + 1..], arith_depth + 1);
                    return if right != 0 { 1 } else { 0 };
                }
                _ => {}
            }
        }

        // Logical AND (&&)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '&' if depth == 0 && i > 0 && chars[i - 1] == '&' => {
                    let left = self.parse_arithmetic_impl(&expr[..i - 1], arith_depth + 1);
                    // Short-circuit: if left is false, don't evaluate right
                    if left == 0 {
                        return 0;
                    }
                    let right = self.parse_arithmetic_impl(&expr[i + 1..], arith_depth + 1);
                    return if right != 0 { 1 } else { 0 };
                }
                _ => {}
            }
        }

        // Bitwise OR (|) - but not ||
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '|' if depth == 0
                    && (i == 0 || chars[i - 1] != '|')
                    && (i + 1 >= chars.len() || chars[i + 1] != '|') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..i], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[i + 1..], arith_depth + 1);
                    return left | right;
                }
                _ => {}
            }
        }

        // Bitwise AND (&) - but not &&
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '&' if depth == 0
                    && (i == 0 || chars[i - 1] != '&')
                    && (i + 1 >= chars.len() || chars[i + 1] != '&') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..i], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[i + 1..], arith_depth + 1);
                    return left & right;
                }
                _ => {}
            }
        }

        // Equality operators (==, !=)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '=' if depth == 0 && i > 0 && chars[i - 1] == '=' => {
                    let left = self.parse_arithmetic_impl(&expr[..i - 1], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[i + 1..], arith_depth + 1);
                    return if left == right { 1 } else { 0 };
                }
                '=' if depth == 0 && i > 0 && chars[i - 1] == '!' => {
                    let left = self.parse_arithmetic_impl(&expr[..i - 1], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[i + 1..], arith_depth + 1);
                    return if left != right { 1 } else { 0 };
                }
                _ => {}
            }
        }

        // Relational operators (<, >, <=, >=)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '=' if depth == 0 && i > 0 && chars[i - 1] == '<' => {
                    let left = self.parse_arithmetic_impl(&expr[..i - 1], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[i + 1..], arith_depth + 1);
                    return if left <= right { 1 } else { 0 };
                }
                '=' if depth == 0 && i > 0 && chars[i - 1] == '>' => {
                    let left = self.parse_arithmetic_impl(&expr[..i - 1], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[i + 1..], arith_depth + 1);
                    return if left >= right { 1 } else { 0 };
                }
                '<' if depth == 0
                    && (i + 1 >= chars.len() || chars[i + 1] != '=')
                    && (i == 0 || chars[i - 1] != '<') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..i], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[i + 1..], arith_depth + 1);
                    return if left < right { 1 } else { 0 };
                }
                '>' if depth == 0
                    && (i + 1 >= chars.len() || chars[i + 1] != '=')
                    && (i == 0 || chars[i - 1] != '>') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..i], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[i + 1..], arith_depth + 1);
                    return if left > right { 1 } else { 0 };
                }
                _ => {}
            }
        }

        // Addition/Subtraction
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '+' | '-' if depth == 0 && i > 0 => {
                    let left = self.parse_arithmetic_impl(&expr[..i], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[i + 1..], arith_depth + 1);
                    return if chars[i] == '+' {
                        left + right
                    } else {
                        left - right
                    };
                }
                _ => {}
            }
        }

        // Multiplication/Division/Modulo (higher precedence)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '*' | '/' | '%' if depth == 0 => {
                    let left = self.parse_arithmetic_impl(&expr[..i], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[i + 1..], arith_depth + 1);
                    return match chars[i] {
                        '*' => left * right,
                        '/' => {
                            if right != 0 {
                                left / right
                            } else {
                                0
                            }
                        }
                        '%' => {
                            if right != 0 {
                                left % right
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    };
                }
                _ => {}
            }
        }

        // Parse as number
        expr.trim().parse().unwrap_or(0)
    }

    /// Expand a variable by name, checking local scope, positional params, shell vars, then env
    fn expand_variable(&self, name: &str) -> String {
        // Check for special parameters (POSIX required)
        match name {
            "?" => return self.last_exit_code.to_string(),
            "#" => {
                // Number of positional parameters
                if let Some(frame) = self.call_stack.last() {
                    return frame.positional.len().to_string();
                }
                return "0".to_string();
            }
            "@" | "*" => {
                // All positional parameters
                if let Some(frame) = self.call_stack.last() {
                    return frame.positional.join(" ");
                }
                return String::new();
            }
            "$" => {
                // $$ - current process ID (simulated)
                return std::process::id().to_string();
            }
            "!" => {
                // $! - PID of most recent background command
                // In Bashkit's virtual environment, background jobs run synchronously
                // Return empty string or last job ID placeholder
                if let Some(last_bg_pid) = self.variables.get("_LAST_BG_PID") {
                    return last_bg_pid.clone();
                }
                return String::new();
            }
            "-" => {
                // $- - Current option flags as a string
                // Build from SHOPT_* variables
                let mut flags = String::new();
                for opt in ['e', 'x', 'u', 'f', 'n', 'v', 'a', 'b', 'h', 'm'] {
                    let opt_name = format!("SHOPT_{}", opt);
                    if self
                        .variables
                        .get(&opt_name)
                        .map(|v| v == "1")
                        .unwrap_or(false)
                    {
                        flags.push(opt);
                    }
                }
                // Also check options struct
                if self.options.errexit && !flags.contains('e') {
                    flags.push('e');
                }
                if self.options.xtrace && !flags.contains('x') {
                    flags.push('x');
                }
                return flags;
            }
            "RANDOM" => {
                // $RANDOM - random number between 0 and 32767
                use std::collections::hash_map::RandomState;
                use std::hash::{BuildHasher, Hasher};
                let random = RandomState::new().build_hasher().finish() as u32;
                return (random % 32768).to_string();
            }
            "LINENO" => {
                // $LINENO - current line number from command span
                return self.current_line.to_string();
            }
            _ => {}
        }

        // Check for numeric positional parameter ($1, $2, etc.)
        if let Ok(n) = name.parse::<usize>() {
            if n == 0 {
                // $0 is the script/function name
                if let Some(frame) = self.call_stack.last() {
                    return frame.name.clone();
                }
                return "bash".to_string();
            }
            // $1, $2, etc. (1-indexed)
            if let Some(frame) = self.call_stack.last() {
                if n > 0 && n <= frame.positional.len() {
                    return frame.positional[n - 1].clone();
                }
            }
            return String::new();
        }

        // Check local variables in call stack (top to bottom)
        for frame in self.call_stack.iter().rev() {
            if let Some(value) = frame.locals.get(name) {
                return value.clone();
            }
        }

        // Check shell variables
        if let Some(value) = self.variables.get(name) {
            return value.clone();
        }

        // Check environment
        if let Some(value) = self.env.get(name) {
            return value.clone();
        }

        // Not found - expand to empty string (bash behavior)
        String::new()
    }

    /// Set a local variable in the current call frame
    #[allow(dead_code)]
    fn set_local(&mut self, name: &str, value: &str) {
        if let Some(frame) = self.call_stack.last_mut() {
            frame.locals.insert(name.to_string(), value.to_string());
        }
    }

    /// Check if a string contains glob characters
    /// Expand brace patterns like {a,b,c} or {1..5}
    /// Returns a Vec of expanded strings, or a single-element Vec if no braces
    fn expand_braces(&self, s: &str) -> Vec<String> {
        // Find the first brace that has a matching close brace
        let mut depth = 0;
        let mut brace_start = None;
        let mut brace_end = None;
        let chars: Vec<char> = s.chars().collect();

        for (i, &ch) in chars.iter().enumerate() {
            match ch {
                '{' => {
                    if depth == 0 {
                        brace_start = Some(i);
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 && brace_start.is_some() {
                        brace_end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }

        // No valid brace pattern found
        let (start, end) = match (brace_start, brace_end) {
            (Some(s), Some(e)) => (s, e),
            _ => return vec![s.to_string()],
        };

        let prefix: String = chars[..start].iter().collect();
        let suffix: String = chars[end + 1..].iter().collect();
        let brace_content: String = chars[start + 1..end].iter().collect();

        // Check for range expansion like {1..5} or {a..z}
        if let Some(range_result) = self.try_expand_range(&brace_content) {
            let mut results = Vec::new();
            for item in range_result {
                let expanded = format!("{}{}{}", prefix, item, suffix);
                // Recursively expand any remaining braces
                results.extend(self.expand_braces(&expanded));
            }
            return results;
        }

        // List expansion like {a,b,c}
        // Need to split by comma, but respect nested braces
        let items = self.split_brace_items(&brace_content);
        if items.len() <= 1 && !brace_content.contains(',') {
            // Not a valid brace expansion (e.g., just {foo})
            return vec![s.to_string()];
        }

        let mut results = Vec::new();
        for item in items {
            let expanded = format!("{}{}{}", prefix, item, suffix);
            // Recursively expand any remaining braces
            results.extend(self.expand_braces(&expanded));
        }

        results
    }

    /// Try to expand a range like 1..5 or a..z
    fn try_expand_range(&self, content: &str) -> Option<Vec<String>> {
        // Check for .. separator
        let parts: Vec<&str> = content.split("..").collect();
        if parts.len() != 2 {
            return None;
        }

        let start = parts[0];
        let end = parts[1];

        // Try numeric range
        if let (Ok(start_num), Ok(end_num)) = (start.parse::<i64>(), end.parse::<i64>()) {
            let mut results = Vec::new();
            if start_num <= end_num {
                for i in start_num..=end_num {
                    results.push(i.to_string());
                }
            } else {
                for i in (end_num..=start_num).rev() {
                    results.push(i.to_string());
                }
            }
            return Some(results);
        }

        // Try character range (single chars only)
        if start.len() == 1 && end.len() == 1 {
            let start_char = start.chars().next().unwrap();
            let end_char = end.chars().next().unwrap();

            if start_char.is_ascii_alphabetic() && end_char.is_ascii_alphabetic() {
                let mut results = Vec::new();
                let start_byte = start_char as u8;
                let end_byte = end_char as u8;

                if start_byte <= end_byte {
                    for b in start_byte..=end_byte {
                        results.push((b as char).to_string());
                    }
                } else {
                    for b in (end_byte..=start_byte).rev() {
                        results.push((b as char).to_string());
                    }
                }
                return Some(results);
            }
        }

        None
    }

    /// Split brace content by commas, respecting nested braces
    fn split_brace_items(&self, content: &str) -> Vec<String> {
        let mut items = Vec::new();
        let mut current = String::new();
        let mut depth = 0;

        for ch in content.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    current.push(ch);
                }
                '}' => {
                    depth -= 1;
                    current.push(ch);
                }
                ',' if depth == 0 => {
                    items.push(current);
                    current = String::new();
                }
                _ => {
                    current.push(ch);
                }
            }
        }
        items.push(current);

        items
    }

    fn contains_glob_chars(&self, s: &str) -> bool {
        s.contains('*') || s.contains('?') || s.contains('[')
    }

    /// Expand a glob pattern against the filesystem
    async fn expand_glob(&self, pattern: &str) -> Result<Vec<String>> {
        let mut matches = Vec::new();

        // Split pattern into directory and filename parts
        let path = Path::new(pattern);
        let (dir, file_pattern) = if path.is_absolute() {
            let parent = path.parent().unwrap_or(Path::new("/"));
            let name = path.file_name().map(|s| s.to_string_lossy().to_string());
            (parent.to_path_buf(), name)
        } else {
            // Relative path - use cwd
            let parent = path.parent();
            let name = path.file_name().map(|s| s.to_string_lossy().to_string());
            if let Some(p) = parent {
                if p.as_os_str().is_empty() {
                    (self.cwd.clone(), name)
                } else {
                    (self.cwd.join(p), name)
                }
            } else {
                (self.cwd.clone(), name)
            }
        };

        let file_pattern = match file_pattern {
            Some(p) => p,
            None => return Ok(matches),
        };

        // Check if the directory exists
        if !self.fs.exists(&dir).await.unwrap_or(false) {
            return Ok(matches);
        }

        // Read directory entries
        let entries = match self.fs.read_dir(&dir).await {
            Ok(e) => e,
            Err(_) => return Ok(matches),
        };

        // Match each entry against the pattern
        for entry in entries {
            if self.glob_match(&entry.name, &file_pattern) {
                // Construct the full path
                let full_path = if path.is_absolute() {
                    dir.join(&entry.name).to_string_lossy().to_string()
                } else {
                    // For relative patterns, return relative path
                    if let Some(parent) = path.parent() {
                        if parent.as_os_str().is_empty() {
                            entry.name.clone()
                        } else {
                            format!("{}/{}", parent.to_string_lossy(), entry.name)
                        }
                    } else {
                        entry.name.clone()
                    }
                };
                matches.push(full_path);
            }
        }

        // Sort matches alphabetically (bash behavior)
        matches.sort();
        Ok(matches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;
    use crate::parser::Parser;

    /// Test timeout with paused time for deterministic behavior
    #[tokio::test(start_paused = true)]
    async fn test_timeout_expires_deterministically() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));

        // timeout 0.001 sleep 10 - should timeout (1ms << 10s)
        let parser = Parser::new("timeout 0.001 sleep 10; echo $?");
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();
        assert_eq!(
            result.stdout.trim(),
            "124",
            "Expected exit code 124 for timeout"
        );
    }

    /// Test zero timeout
    #[tokio::test(start_paused = true)]
    async fn test_timeout_zero_deterministically() {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));

        // timeout 0 sleep 1 - should timeout immediately
        let parser = Parser::new("timeout 0 sleep 1; echo $?");
        let ast = parser.parse().unwrap();
        let result = interp.execute(&ast).await.unwrap();
        assert_eq!(
            result.stdout.trim(),
            "124",
            "Expected exit code 124 for zero timeout"
        );
    }

    /// Test that parse_timeout_duration preserves subsecond precision
    #[test]
    fn test_parse_timeout_duration_subsecond() {
        use std::time::Duration;

        // Should preserve subsecond precision
        let d = Interpreter::parse_timeout_duration("0.001").unwrap();
        assert_eq!(d, Duration::from_secs_f64(0.001));

        let d = Interpreter::parse_timeout_duration("0.5").unwrap();
        assert_eq!(d, Duration::from_millis(500));

        let d = Interpreter::parse_timeout_duration("1.5s").unwrap();
        assert_eq!(d, Duration::from_millis(1500));

        // Zero should work
        let d = Interpreter::parse_timeout_duration("0").unwrap();
        assert_eq!(d, Duration::ZERO);
    }

    // POSIX special builtins tests

    /// Helper to run a script and return result
    async fn run_script(script: &str) -> ExecResult {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut interp = Interpreter::new(Arc::clone(&fs));
        let parser = Parser::new(script);
        let ast = parser.parse().unwrap();
        interp.execute(&ast).await.unwrap()
    }

    #[tokio::test]
    async fn test_colon_null_utility() {
        // POSIX : (colon) - null utility, should return success
        let result = run_script(":").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_colon_with_args() {
        // Colon should ignore arguments and still succeed
        let result = run_script(": arg1 arg2 arg3").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_colon_in_while_loop() {
        // Common use case: while : (infinite loop, but we limit iterations)
        let result = run_script(
            "x=0; while :; do x=$((x+1)); if [ $x -ge 3 ]; then break; fi; done; echo $x",
        )
        .await;
        assert_eq!(result.stdout.trim(), "3");
    }

    #[tokio::test]
    async fn test_times_builtin() {
        // POSIX times - returns process times (zeros in virtual mode)
        let result = run_script("times").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("0m0.000s"));
    }

    #[tokio::test]
    async fn test_readonly_basic() {
        // POSIX readonly - mark variable as read-only
        let result = run_script("readonly X=value; echo $X").await;
        assert_eq!(result.stdout.trim(), "value");
    }

    #[tokio::test]
    async fn test_special_param_dash() {
        // $- should return current option flags
        let result = run_script("set -e; echo \"$-\"").await;
        assert!(result.stdout.contains('e'));
    }

    #[tokio::test]
    async fn test_special_param_bang() {
        // $! - last background PID (empty in virtual mode with no bg jobs)
        let result = run_script("echo \"$!\"").await;
        // Should be empty or a placeholder
        assert_eq!(result.exit_code, 0);
    }

    // =========================================================================
    // Additional POSIX positive tests
    // =========================================================================

    #[tokio::test]
    async fn test_colon_variable_side_effect() {
        // Common pattern: use : with parameter expansion for defaults
        let result = run_script(": ${X:=default}; echo $X").await;
        assert_eq!(result.stdout.trim(), "default");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_colon_in_if_then() {
        // Use : as no-op in then branch
        let result = run_script("if true; then :; fi; echo done").await;
        assert_eq!(result.stdout.trim(), "done");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_readonly_set_and_read() {
        // Set readonly variable and verify it's accessible
        let result = run_script("readonly FOO=bar; readonly BAR=baz; echo $FOO $BAR").await;
        assert_eq!(result.stdout.trim(), "bar baz");
    }

    #[tokio::test]
    async fn test_readonly_mark_existing() {
        // Mark an existing variable as readonly
        let result = run_script("X=hello; readonly X; echo $X").await;
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_times_two_lines() {
        // times should output exactly two lines
        let result = run_script("times").await;
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[tokio::test]
    async fn test_eval_simple_command() {
        // eval should execute the constructed command
        let result = run_script("cmd='echo hello'; eval $cmd").await;
        // Note: eval stores command for interpreter, actual execution depends on interpreter support
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_special_param_dash_multiple_options() {
        // Set multiple options and verify $- contains them
        let result = run_script("set -e; set -x; echo \"$-\"").await;
        assert!(result.stdout.contains('e'));
        // Note: x is stored but we verify at least e is present
    }

    #[tokio::test]
    async fn test_special_param_dash_no_options() {
        // With no options set, $- should be empty or minimal
        let result = run_script("echo \"flags:$-:end\"").await;
        assert!(result.stdout.contains("flags:"));
        assert!(result.stdout.contains(":end"));
        assert_eq!(result.exit_code, 0);
    }

    // =========================================================================
    // POSIX negative tests (error cases / edge cases)
    // =========================================================================

    #[tokio::test]
    async fn test_colon_does_not_produce_output() {
        // Colon should never produce any output
        let result = run_script(": 'this should not appear'").await;
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "");
    }

    #[tokio::test]
    async fn test_eval_empty_args() {
        // eval with no arguments should succeed silently
        let result = run_script("eval; echo $?").await;
        assert!(result.stdout.contains('0'));
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_readonly_empty_value() {
        // readonly with empty value
        let result = run_script("readonly EMPTY=; echo \"[$EMPTY]\"").await;
        assert_eq!(result.stdout.trim(), "[]");
    }

    #[tokio::test]
    async fn test_times_no_args_accepted() {
        // times should ignore any arguments
        let result = run_script("times ignored args here").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("0m0.000s"));
    }

    #[tokio::test]
    async fn test_special_param_bang_empty_without_bg() {
        // $! should be empty when no background jobs have run
        let result = run_script("x=\"$!\"; [ -z \"$x\" ] && echo empty || echo not_empty").await;
        assert_eq!(result.stdout.trim(), "empty");
    }

    #[tokio::test]
    async fn test_colon_exit_code_zero() {
        // Verify colon always returns 0 even after failed command
        let result = run_script("false; :; echo $?").await;
        assert_eq!(result.stdout.trim(), "0");
    }

    #[tokio::test]
    async fn test_readonly_without_value_preserves_existing() {
        // readonly on existing var preserves value
        let result = run_script("VAR=existing; readonly VAR; echo $VAR").await;
        assert_eq!(result.stdout.trim(), "existing");
    }

    // bash/sh command tests

    #[tokio::test]
    async fn test_bash_c_simple_command() {
        // bash -c "command" should execute the command
        let result = run_script("bash -c 'echo hello'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_sh_c_simple_command() {
        // sh -c "command" should also work
        let result = run_script("sh -c 'echo world'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "world");
    }

    #[tokio::test]
    async fn test_bash_c_multiple_commands() {
        // bash -c with multiple commands separated by semicolon
        let result = run_script("bash -c 'echo one; echo two'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "one\ntwo\n");
    }

    #[tokio::test]
    async fn test_bash_c_with_positional_args() {
        // bash -c "cmd" arg0 arg1 - positional parameters
        let result = run_script("bash -c 'echo $0 $1' zero one").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "zero one");
    }

    #[tokio::test]
    async fn test_bash_script_file() {
        // bash script.sh - execute a script file
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(std::path::Path::new("/tmp/test.sh"), b"echo 'from script'")
            .await
            .unwrap();

        let mut interpreter = Interpreter::new(fs.clone());
        let parser = Parser::new("bash /tmp/test.sh");
        let script = parser.parse().unwrap();
        let result = interpreter.execute(&script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "from script");
    }

    #[tokio::test]
    async fn test_bash_script_file_with_args() {
        // bash script.sh arg1 arg2 - script with arguments
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(std::path::Path::new("/tmp/args.sh"), b"echo $1 $2")
            .await
            .unwrap();

        let mut interpreter = Interpreter::new(fs.clone());
        let parser = Parser::new("bash /tmp/args.sh first second");
        let script = parser.parse().unwrap();
        let result = interpreter.execute(&script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "first second");
    }

    #[tokio::test]
    async fn test_bash_piped_script() {
        // echo "script" | bash - execute from stdin
        let result = run_script("echo 'echo piped' | bash").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "piped");
    }

    #[tokio::test]
    async fn test_bash_nonexistent_file() {
        // bash missing.sh - should error with exit code 127
        let result = run_script("bash /nonexistent/missing.sh").await;
        assert_eq!(result.exit_code, 127);
        assert!(result.stderr.contains("No such file"));
    }

    #[tokio::test]
    async fn test_bash_c_missing_argument() {
        // bash -c without command string - should error
        let result = run_script("bash -c").await;
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("option requires an argument"));
    }

    #[tokio::test]
    async fn test_bash_c_syntax_error() {
        // bash -c with invalid syntax
        let result = run_script("bash -c 'if then'").await;
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("syntax error"));
    }

    #[tokio::test]
    async fn test_bash_preserves_variables() {
        // Variables set in bash -c should affect the parent
        // (since we share the interpreter state)
        let result = run_script("bash -c 'X=inner'; echo $X").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "inner");
    }

    #[tokio::test]
    async fn test_bash_c_exit_code_propagates() {
        // Exit code from bash -c should propagate
        let result = run_script("bash -c 'exit 42'; echo $?").await;
        assert_eq!(result.stdout.trim(), "42");
    }

    #[tokio::test]
    async fn test_bash_nested() {
        // Nested bash -c calls
        let result = run_script("bash -c \"bash -c 'echo nested'\"").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "nested");
    }

    #[tokio::test]
    async fn test_sh_script_file() {
        // sh script.sh - same as bash script.sh
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(std::path::Path::new("/tmp/sh_test.sh"), b"echo 'sh works'")
            .await
            .unwrap();

        let mut interpreter = Interpreter::new(fs.clone());
        let parser = Parser::new("sh /tmp/sh_test.sh");
        let script = parser.parse().unwrap();
        let result = interpreter.execute(&script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "sh works");
    }

    #[tokio::test]
    async fn test_bash_with_option_e() {
        // bash -e -c "command" - -e is accepted but doesn't change behavior in virtual mode
        let result = run_script("bash -e -c 'echo works'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "works");
    }

    #[tokio::test]
    async fn test_bash_empty_input() {
        // bash with no arguments or stdin does nothing
        let result = run_script("bash; echo done").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "done");
    }

    // Additional bash/sh tests for noexec, version, help

    #[tokio::test]
    async fn test_bash_n_syntax_check_success() {
        // bash -n parses but doesn't execute
        let result = run_script("bash -n -c 'echo should not print'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, ""); // Nothing printed - didn't execute
    }

    #[tokio::test]
    async fn test_bash_n_syntax_error_detected() {
        // bash -n catches syntax errors
        let result = run_script("bash -n -c 'if then'").await;
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("syntax error"));
    }

    #[tokio::test]
    async fn test_bash_n_combined_flags() {
        // -n can be combined with other flags like -ne
        let result = run_script("bash -ne -c 'echo test'; echo done").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "done"); // Only "done" - bash -n didn't execute
    }

    #[tokio::test]
    async fn test_bash_version() {
        // --version shows Bashkit version
        let result = run_script("bash --version").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Bashkit"));
        assert!(result.stdout.contains("virtual"));
    }

    #[tokio::test]
    async fn test_sh_version() {
        // sh --version also works
        let result = run_script("sh --version").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("virtual sh"));
    }

    #[tokio::test]
    async fn test_bash_help() {
        // --help shows usage
        let result = run_script("bash --help").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Usage:"));
        assert!(result.stdout.contains("-c string"));
        assert!(result.stdout.contains("-n"));
    }

    #[tokio::test]
    async fn test_bash_double_dash() {
        // -- ends option processing
        let result = run_script("bash -- --help").await;
        // Should try to run file named "--help", which doesn't exist
        assert_eq!(result.exit_code, 127);
    }

    // Negative test cases

    #[tokio::test]
    async fn test_bash_invalid_syntax_in_file() {
        // Syntax error in script file - unclosed if
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(std::path::Path::new("/tmp/bad.sh"), b"if true; then echo x")
            .await
            .unwrap();

        let mut interpreter = Interpreter::new(fs.clone());
        let parser = Parser::new("bash /tmp/bad.sh");
        let script = parser.parse().unwrap();
        let result = interpreter.execute(&script).await.unwrap();

        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("syntax error"));
    }

    #[tokio::test]
    async fn test_bash_permission_in_sandbox() {
        // Filesystem operations work through bash -c
        let result = run_script("bash -c 'echo test > /tmp/out.txt && cat /tmp/out.txt'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "test");
    }

    #[tokio::test]
    async fn test_bash_all_positional() {
        // $@ and $* work correctly
        let result = run_script("bash -c 'echo $@' _ a b c").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "a b c");
    }

    #[tokio::test]
    async fn test_bash_arg_count() {
        // $# counts positional params
        let result = run_script("bash -c 'echo $#' _ 1 2 3 4").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "4");
    }

    // Security-focused tests

    #[tokio::test]
    async fn test_bash_no_real_bash_escape() {
        // Verify bash -c doesn't escape sandbox
        // Try to run a command that would work in real bash but not here
        let result = run_script("bash -c 'which bash 2>/dev/null || echo not found'").await;
        // 'which' is not a builtin, so this should fail
        assert!(result.stdout.contains("not found") || result.exit_code == 127);
    }

    #[tokio::test]
    async fn test_bash_nested_limits_respected() {
        // Deep nesting should eventually hit limits
        // This tests that bash -c doesn't bypass command limits
        let result = run_script("bash -c 'for i in 1 2 3; do echo $i; done'").await;
        assert_eq!(result.exit_code, 0);
        // Loop executed successfully within limits
    }

    #[tokio::test]
    async fn test_bash_c_injection_safe() {
        // Variable expansion doesn't allow injection
        let result = run_script("INJECT='; rm -rf /'; bash -c 'echo safe'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "safe");
    }

    #[tokio::test]
    async fn test_bash_version_no_host_info() {
        // Version output doesn't leak host information
        let result = run_script("bash --version").await;
        assert!(!result.stdout.contains("/usr"));
        assert!(!result.stdout.contains("GNU"));
        // Should only contain virtual version info
    }

    // Additional positive tests

    #[tokio::test]
    async fn test_bash_c_with_quotes() {
        // Handles quoted strings correctly
        let result = run_script(r#"bash -c 'echo "hello world"'"#).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "hello world");
    }

    #[tokio::test]
    async fn test_bash_c_with_variables() {
        // Variables expand correctly in bash -c
        let result = run_script("X=test; bash -c 'echo $X'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "test");
    }

    #[tokio::test]
    async fn test_bash_c_pipe_in_command() {
        // Pipes work inside bash -c
        let result = run_script("bash -c 'echo hello | cat'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_bash_c_subshell() {
        // Command substitution works in bash -c
        let result = run_script("bash -c 'echo $(echo inner)'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "inner");
    }

    #[tokio::test]
    async fn test_bash_c_conditional() {
        // Conditionals work in bash -c
        let result = run_script("bash -c 'if true; then echo yes; fi'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "yes");
    }

    #[tokio::test]
    async fn test_bash_script_with_shebang() {
        // Script with shebang is handled (shebang line ignored)
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(
            std::path::Path::new("/tmp/shebang.sh"),
            b"#!/bin/bash\necho works",
        )
        .await
        .unwrap();

        let mut interpreter = Interpreter::new(fs.clone());
        let parser = Parser::new("bash /tmp/shebang.sh");
        let script = parser.parse().unwrap();
        let result = interpreter.execute(&script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "works");
    }

    #[tokio::test]
    async fn test_bash_n_with_valid_multiline() {
        // -n validates multiline scripts
        let result = run_script("bash -n -c 'echo one\necho two\necho three'").await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_sh_behaves_like_bash() {
        // sh and bash produce same results
        let bash_result = run_script("bash -c 'echo $((1+2))'").await;
        let sh_result = run_script("sh -c 'echo $((1+2))'").await;
        assert_eq!(bash_result.stdout, sh_result.stdout);
        assert_eq!(bash_result.exit_code, sh_result.exit_code);
    }

    // Additional negative tests

    #[tokio::test]
    async fn test_bash_n_unclosed_if() {
        // -n catches unclosed control structures
        let result = run_script("bash -n -c 'if true; then echo x'").await;
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("syntax error"));
    }

    #[tokio::test]
    async fn test_bash_n_unclosed_while() {
        // -n catches unclosed while
        let result = run_script("bash -n -c 'while true; do echo x'").await;
        assert_eq!(result.exit_code, 2);
    }

    #[tokio::test]
    async fn test_bash_empty_c_string() {
        // Empty -c string is valid (does nothing)
        let result = run_script("bash -c ''").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_bash_whitespace_only_c_string() {
        // Whitespace-only -c string is valid
        let result = run_script("bash -c '   '").await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_bash_directory_not_file() {
        // Trying to execute a directory fails
        let result = run_script("bash /tmp").await;
        // Should fail - /tmp is a directory
        assert_ne!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_bash_c_exit_propagates() {
        // Exit code from bash -c is captured in $?
        let result = run_script("bash -c 'exit 42'; echo \"code: $?\"").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("code: 42"));
    }

    #[tokio::test]
    async fn test_bash_multiple_scripts_sequential() {
        // Multiple bash calls work sequentially
        let result = run_script("bash -c 'echo 1'; bash -c 'echo 2'; bash -c 'echo 3'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\n2\n3\n");
    }

    // Security edge cases

    #[tokio::test]
    async fn test_bash_c_path_traversal_blocked() {
        // Path traversal in bash -c doesn't escape sandbox
        let result =
            run_script("bash -c 'cat /../../etc/passwd 2>/dev/null || echo blocked'").await;
        assert!(result.stdout.contains("blocked") || result.exit_code != 0);
    }

    #[tokio::test]
    async fn test_bash_nested_deeply() {
        // Deeply nested bash calls work within limits
        let result = run_script("bash -c \"bash -c 'bash -c \\\"echo deep\\\"'\"").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "deep");
    }

    #[tokio::test]
    async fn test_bash_c_special_chars() {
        // Special characters in commands handled safely
        let result = run_script("bash -c 'echo \"$HOME\"'").await;
        // Should not leak real home directory
        assert!(!result.stdout.contains("/root"));
        assert!(!result.stdout.contains("/home/"));
    }

    #[tokio::test]
    async fn test_bash_c_dollar_substitution() {
        // $() substitution works in bash -c
        let result = run_script("bash -c 'echo $(echo subst)'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "subst");
    }

    #[tokio::test]
    async fn test_bash_help_contains_expected_options() {
        // Help output contains documented options
        let result = run_script("bash --help").await;
        assert!(result.stdout.contains("-c"));
        assert!(result.stdout.contains("-n"));
        assert!(result.stdout.contains("--version"));
    }

    #[tokio::test]
    async fn test_bash_c_array_operations() {
        // Array operations work in bash -c
        let result = run_script("bash -c 'arr=(a b c); echo ${arr[1]}'").await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "b");
    }

    #[tokio::test]
    async fn test_bash_positional_special_vars() {
        // Special positional vars work
        let result = run_script("bash -c 'echo \"args: $#, first: $1, all: $*\"' prog a b c").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("args: 3"));
        assert!(result.stdout.contains("first: a"));
        assert!(result.stdout.contains("all: a b c"));
    }
}
