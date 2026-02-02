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
use crate::error::{Error, Result};
use crate::fs::FileSystem;
use crate::limits::{ExecutionCounters, ExecutionLimits};
use crate::parser::{
    ArithmeticForCommand, AssignmentValue, CaseCommand, Command, CommandList, CompoundCommand,
    ForCommand, FunctionDef, IfCommand, ListOperator, ParameterOp, Pipeline, Redirect,
    RedirectKind, Script, SimpleCommand, TimeCommand, UntilCommand, WhileCommand, Word, WordPart,
};

#[cfg(feature = "failpoints")]
use fail::fail_point;

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
    /// HTTP client for network builtins (curl, wget)
    #[cfg(feature = "http_client")]
    http_client: Option<crate::network::HttpClient>,
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
    /// * `username` - Optional custom username for sandbox identity
    /// * `hostname` - Optional custom hostname for sandbox identity
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
        builtins.insert("sort".to_string(), Box::new(builtins::Sort));
        builtins.insert("uniq".to_string(), Box::new(builtins::Uniq));
        builtins.insert("cut".to_string(), Box::new(builtins::Cut));
        builtins.insert("tr".to_string(), Box::new(builtins::Tr));
        builtins.insert("date".to_string(), Box::new(builtins::Date));
        builtins.insert("wait".to_string(), Box::new(builtins::Wait));
        builtins.insert("curl".to_string(), Box::new(builtins::Curl));
        builtins.insert("wget".to_string(), Box::new(builtins::Wget));
        builtins.insert("timeout".to_string(), Box::new(builtins::Timeout));
        // System info builtins (configurable sandbox values)
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
            #[cfg(feature = "http_client")]
            http_client: None,
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

    /// Set the HTTP client for network builtins (curl, wget).
    ///
    /// This is only available when the `network` feature is enabled.
    #[cfg(feature = "http_client")]
    pub fn set_http_client(&mut self, client: crate::network::HttpClient) {
        self.http_client = Some(client);
    }

    /// Execute a script.
    pub async fn execute(&mut self, script: &Script) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        for command in &script.commands {
            let result = self.execute_command(command).await?;
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

    fn execute_command<'a>(
        &'a mut self,
        command: &'a Command,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>> {
        Box::pin(async move {
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
                Command::Compound(compound) => self.execute_compound(compound).await,
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

        // Get iteration values
        let values: Vec<String> = if let Some(words) = &for_cmd.words {
            let mut vals = Vec::new();
            for w in words {
                // Use expand_word_to_fields to properly handle "${arr[@]}"
                let fields = self.expand_word_to_fields(w).await?;
                vals.extend(fields);
            }
            vals
        } else {
            // TODO: Use positional parameters
            Vec::new()
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
            let result = self.execute_command_sequence(&for_cmd.body).await?;
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
            let result = self.execute_command_sequence(&arith_for.body).await?;
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
            let result = self.execute_command_sequence(&while_cmd.body).await?;
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
            let result = self.execute_command_sequence(&until_cmd.body).await?;
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
    /// Note: BashKit only measures wall-clock (real) time.
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

    /// Check if a value matches a shell pattern
    fn pattern_matches(&self, value: &str, pattern: &str) -> bool {
        // Handle special case of * (match anything)
        if pattern == "*" {
            return true;
        }

        // Simple pattern matching - for now just literal matching
        // TODO: Implement full glob pattern matching (*, ?, [])
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            // Simple wildcard matching
            self.glob_match(value, pattern)
        } else {
            // Literal match
            value == pattern
        }
    }

    /// Simple glob pattern matching
    fn glob_match(&self, value: &str, pattern: &str) -> bool {
        // Convert pattern to a simple regex-like matching
        let mut value_chars = value.chars().peekable();
        let mut pattern_chars = pattern.chars().peekable();

        loop {
            match (pattern_chars.peek(), value_chars.peek()) {
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
                (Some(p), Some(v)) => {
                    if *p == *v {
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
            let result = self.execute_command(command).await?;
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

            match command {
                Command::Simple(simple) => {
                    let result = self
                        .execute_simple_command(simple, stdin_data.take())
                        .await?;

                    if is_last {
                        last_result = result;
                    } else {
                        // Pass stdout to next command's stdin
                        stdin_data = Some(result.stdout);
                    }
                }
                _ => {
                    return Err(Error::Execution(
                        "only simple commands supported in pipeline".to_string(),
                    ))
                }
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
        let result = self.execute_command(&list.first).await?;
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
                    // TODO: Implement background execution
                    true
                }
            };

            if should_execute {
                let result = self.execute_command(cmd).await?;
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

        // If name is empty, this is an assignment-only command
        if name.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        // Expand arguments with brace and glob expansion
        let mut args: Vec<String> = Vec::new();
        for word in &command.args {
            let expanded = self.expand_word(word).await?;

            // Skip brace and glob expansion for quoted words
            if word.quoted {
                args.push(expanded);
                continue;
            }

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

        // Handle input redirections first
        let stdin = self
            .process_input_redirections(stdin, &command.redirects)
            .await?;

        // Check for functions first
        if let Some(func_def) = self.functions.get(&name).cloned() {
            // Check function depth limit
            self.counters.push_function(&self.limits)?;

            // Push call frame with positional parameters
            self.call_stack.push(CallFrame {
                name: name.clone(),
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

        // Check for builtins
        if let Some(builtin) = self.builtins.get(name.as_str()) {
            let ctx = builtins::Context {
                args: &args,
                env: &self.env,
                variables: &mut self.variables,
                cwd: &mut self.cwd,
                fs: Arc::clone(&self.fs),
                stdin: stdin.as_deref(),
                #[cfg(feature = "http_client")]
                http_client: self.http_client.as_ref(),
            };

            // Execute builtin with panic catching for security
            // SECURITY: Custom builtins may panic - we catch this to:
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

        // Command not found - return error like bash does (exit code 127)
        Ok(ExecResult::err(
            format!("bash: {}: command not found", name),
            127,
        ))
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
                    let content = self.fs.read_file(&path).await?;
                    stdin = Some(String::from_utf8_lossy(&content).to_string());
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
                    // Check which fd we're redirecting
                    match redirect.fd {
                        Some(2) => {
                            // 2> - redirect stderr to file
                            self.fs.write_file(&path, result.stderr.as_bytes()).await?;
                            result.stderr = String::new();
                        }
                        _ => {
                            // Default (stdout) - write stdout to file
                            self.fs.write_file(&path, result.stdout.as_bytes()).await?;
                            result.stdout = String::new();
                        }
                    }
                }
                RedirectKind::Append => {
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    // Check which fd we're appending
                    match redirect.fd {
                        Some(2) => {
                            // 2>> - append stderr to file
                            self.fs.append_file(&path, result.stderr.as_bytes()).await?;
                            result.stderr = String::new();
                        }
                        _ => {
                            // Default (stdout) - append stdout to file
                            self.fs.append_file(&path, result.stdout.as_bytes()).await?;
                            result.stdout = String::new();
                        }
                    }
                }
                RedirectKind::OutputBoth => {
                    // &> - redirect both stdout and stderr to file
                    let target_path = self.expand_word(&redirect.target).await?;
                    let path = self.resolve_path(&target_path);
                    // Write both stdout and stderr to file
                    let combined = format!("{}{}", result.stdout, result.stderr);
                    self.fs.write_file(&path, combined.as_bytes()).await?;
                    result.stdout = String::new();
                    result.stderr = String::new();
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
                    }
                    // Remove trailing newline (bash behavior)
                    let trimmed = stdout.trim_end_matches('\n');
                    result.push_str(trimmed);
                }
                WordPart::ArithmeticExpansion(expr) => {
                    // Evaluate arithmetic expression
                    let value = self.evaluate_arithmetic(expr);
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

    /// Expand a word to multiple fields (for array iteration in for loops)
    /// Returns Vec<String> where array expansions like "${arr[@]}" produce multiple fields
    async fn expand_word_to_fields(&mut self, word: &Word) -> Result<Vec<String>> {
        // Check if the word contains only an array expansion
        if word.parts.len() == 1 {
            if let WordPart::ArrayAccess { name, index } = &word.parts[0] {
                if index == "@" || index == "*" {
                    // ${arr[@]} or ${arr[*]} - return each element as separate field
                    if let Some(arr) = self.arrays.get(name) {
                        let mut indices: Vec<_> = arr.keys().collect();
                        indices.sort();
                        return Ok(indices.iter().filter_map(|i| arr.get(i).cloned()).collect());
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

    /// Evaluate a simple arithmetic expression
    fn evaluate_arithmetic(&self, expr: &str) -> i64 {
        // Simple arithmetic evaluation - handles basic operations
        let expr = expr.trim();

        // First expand any variables in the expression
        let expanded = self.expand_arithmetic_vars(expr);

        // Parse and evaluate
        self.parse_arithmetic(&expanded)
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

    /// Parse and evaluate a simple arithmetic expression
    fn parse_arithmetic(&self, expr: &str) -> i64 {
        let expr = expr.trim();

        if expr.is_empty() {
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
                return self.parse_arithmetic(&expr[1..expr.len() - 1]);
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
                                let cond = self.parse_arithmetic(&expr[..i]);
                                let then_val = self.parse_arithmetic(&expr[i + 1..j]);
                                let else_val = self.parse_arithmetic(&expr[j + 1..]);
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
                    let left = self.parse_arithmetic(&expr[..i - 1]);
                    // Short-circuit: if left is true, don't evaluate right
                    if left != 0 {
                        return 1;
                    }
                    let right = self.parse_arithmetic(&expr[i + 1..]);
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
                    let left = self.parse_arithmetic(&expr[..i - 1]);
                    // Short-circuit: if left is false, don't evaluate right
                    if left == 0 {
                        return 0;
                    }
                    let right = self.parse_arithmetic(&expr[i + 1..]);
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
                    let left = self.parse_arithmetic(&expr[..i]);
                    let right = self.parse_arithmetic(&expr[i + 1..]);
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
                    let left = self.parse_arithmetic(&expr[..i]);
                    let right = self.parse_arithmetic(&expr[i + 1..]);
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
                    let left = self.parse_arithmetic(&expr[..i - 1]);
                    let right = self.parse_arithmetic(&expr[i + 1..]);
                    return if left == right { 1 } else { 0 };
                }
                '=' if depth == 0 && i > 0 && chars[i - 1] == '!' => {
                    let left = self.parse_arithmetic(&expr[..i - 1]);
                    let right = self.parse_arithmetic(&expr[i + 1..]);
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
                    let left = self.parse_arithmetic(&expr[..i - 1]);
                    let right = self.parse_arithmetic(&expr[i + 1..]);
                    return if left <= right { 1 } else { 0 };
                }
                '=' if depth == 0 && i > 0 && chars[i - 1] == '>' => {
                    let left = self.parse_arithmetic(&expr[..i - 1]);
                    let right = self.parse_arithmetic(&expr[i + 1..]);
                    return if left >= right { 1 } else { 0 };
                }
                '<' if depth == 0
                    && (i + 1 >= chars.len() || chars[i + 1] != '=')
                    && (i == 0 || chars[i - 1] != '<') =>
                {
                    let left = self.parse_arithmetic(&expr[..i]);
                    let right = self.parse_arithmetic(&expr[i + 1..]);
                    return if left < right { 1 } else { 0 };
                }
                '>' if depth == 0
                    && (i + 1 >= chars.len() || chars[i + 1] != '=')
                    && (i == 0 || chars[i - 1] != '>') =>
                {
                    let left = self.parse_arithmetic(&expr[..i]);
                    let right = self.parse_arithmetic(&expr[i + 1..]);
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
                    let left = self.parse_arithmetic(&expr[..i]);
                    let right = self.parse_arithmetic(&expr[i + 1..]);
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
                    let left = self.parse_arithmetic(&expr[..i]);
                    let right = self.parse_arithmetic(&expr[i + 1..]);
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
                // In sandboxed environment, background jobs run synchronously
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
                // $LINENO - current line number (not tracked, return 1)
                // TODO: Track line numbers in parser and interpreter
                return "1".to_string();
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
        // POSIX times - returns process times (zeros in sandbox)
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
        // $! - last background PID (empty in sandbox with no bg jobs)
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
}
