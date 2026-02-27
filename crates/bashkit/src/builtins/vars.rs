//! Variable manipulation builtins: set, unset, local, shift, readonly, eval, times
//!
//! POSIX special built-in utilities for variable management.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// unset builtin - remove variables
pub struct Unset;

#[async_trait]
impl Builtin for Unset {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        for name in ctx.args {
            ctx.variables.remove(name);
            // Note: env is immutable in our model - environment variables
            // are inherited and can't be unset by the shell
        }
        Ok(ExecResult::ok(String::new()))
    }
}

/// set builtin - set/display shell options and positional parameters
///
/// Supports:
/// - `set -e` / `set +e` - errexit
/// - `set -u` / `set +u` - nounset
/// - `set -x` / `set +x` - xtrace
/// - `set -o option` / `set +o option` - long option names
/// - `set --` - set positional parameters
pub struct Set;

/// Map long option names to their SHOPT_* variable names
fn option_name_to_var(name: &str) -> Option<&'static str> {
    match name {
        "errexit" => Some("SHOPT_e"),
        "nounset" => Some("SHOPT_u"),
        "xtrace" => Some("SHOPT_x"),
        "verbose" => Some("SHOPT_v"),
        "pipefail" => Some("SHOPT_pipefail"),
        "noclobber" => Some("SHOPT_C"),
        "noglob" => Some("SHOPT_f"),
        "noexec" => Some("SHOPT_n"),
        _ => None,
    }
}

/// All known `set -o` options with their variable names, in display order.
const SET_O_OPTIONS: &[(&str, &str)] = &[
    ("errexit", "SHOPT_e"),
    ("noglob", "SHOPT_f"),
    ("noclobber", "SHOPT_C"),
    ("noexec", "SHOPT_n"),
    ("nounset", "SHOPT_u"),
    ("pipefail", "SHOPT_pipefail"),
    ("verbose", "SHOPT_v"),
    ("xtrace", "SHOPT_x"),
];

/// Format option display for `set -o` (human-readable).
fn format_set_dash_o(variables: &std::collections::HashMap<String, String>) -> String {
    let mut output = String::new();
    for (name, var) in SET_O_OPTIONS {
        let enabled = variables.get(*var).map(|v| v == "1").unwrap_or(false);
        let state = if enabled { "on" } else { "off" };
        output.push_str(&format!("{:<15}\t{}\n", name, state));
    }
    output
}

/// Format option display for `set +o` (re-executable).
fn format_set_plus_o(variables: &std::collections::HashMap<String, String>) -> String {
    let mut output = String::new();
    for (name, var) in SET_O_OPTIONS {
        let enabled = variables.get(*var).map(|v| v == "1").unwrap_or(false);
        let flag = if enabled { "-o" } else { "+o" };
        output.push_str(&format!("set {} {}\n", flag, name));
    }
    output
}

#[async_trait]
impl Builtin for Set {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            // Display all variables
            let mut output = String::new();
            for (name, value) in ctx.variables.iter() {
                output.push_str(&format!("{}={}\n", name, value));
            }
            return Ok(ExecResult::ok(output));
        }

        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            if arg == "--" {
                // Everything after `--` becomes positional parameters.
                // Encode as unit-separator-delimited string for the interpreter
                // to pick up (same pattern as _SHIFT_COUNT).
                let positional: Vec<&str> = ctx.args[i + 1..].iter().map(|s| s.as_str()).collect();
                ctx.variables
                    .insert("_SET_POSITIONAL".to_string(), positional.join("\x1F"));
                break;
            } else if (arg.starts_with('-') || arg.starts_with('+'))
                && arg.len() > 1
                && (arg.as_bytes()[1] == b'o' && arg.len() == 2)
            {
                // -o / +o: either display options or set/unset a named option
                let enable = arg.starts_with('-');
                if i + 1 < ctx.args.len() {
                    // -o option_name / +o option_name
                    i += 1;
                    if let Some(var) = option_name_to_var(&ctx.args[i]) {
                        ctx.variables
                            .insert(var.to_string(), if enable { "1" } else { "0" }.to_string());
                    }
                } else {
                    // Bare -o or +o: display options
                    let output = if enable {
                        format_set_dash_o(ctx.variables)
                    } else {
                        format_set_plus_o(ctx.variables)
                    };
                    return Ok(ExecResult::ok(output));
                }
            } else if arg.starts_with('-') || arg.starts_with('+') {
                let enable = arg.starts_with('-');
                for opt in arg.chars().skip(1) {
                    let opt_name = format!("SHOPT_{}", opt);
                    ctx.variables
                        .insert(opt_name, if enable { "1" } else { "0" }.to_string());
                }
            }
            i += 1;
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// shift builtin - shift positional parameters
pub struct Shift;

#[async_trait]
impl Builtin for Shift {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Number of positions to shift (default 1)
        let n: usize = ctx.args.first().and_then(|s| s.parse().ok()).unwrap_or(1);

        // In real bash, this shifts the positional parameters
        // For now, we store the shift count for the interpreter to handle
        ctx.variables
            .insert("_SHIFT_COUNT".to_string(), n.to_string());

        Ok(ExecResult::ok(String::new()))
    }
}

/// local builtin - declare local variables in functions
pub struct Local;

#[async_trait]
impl Builtin for Local {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Local sets variables in the current function scope
        // The actual scoping is handled by the interpreter's call stack
        for arg in ctx.args {
            if let Some(eq_pos) = arg.find('=') {
                let name = &arg[..eq_pos];
                let value = &arg[eq_pos + 1..];
                // Mark as local by setting it
                ctx.variables.insert(name.to_string(), value.to_string());
            } else {
                // Just declare without value
                ctx.variables.insert(arg.to_string(), String::new());
            }
        }
        Ok(ExecResult::ok(String::new()))
    }
}

/// readonly builtin - POSIX special built-in to mark variables as read-only.
///
/// Usage:
/// - `readonly VAR` - mark existing variable as readonly
/// - `readonly VAR=value` - set and mark as readonly
/// - `readonly -p` - print all readonly variables
///
/// Note: Readonly enforcement is tracked via _READONLY_* marker variables.
/// The interpreter checks these markers before allowing variable assignment.
pub struct Readonly;

#[async_trait]
impl Builtin for Readonly {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Handle -p flag to print readonly variables
        if ctx.args.first().map(|s| s.as_str()) == Some("-p") {
            let mut output = String::new();
            for (name, _) in ctx.variables.iter() {
                if let Some(var_name) = name.strip_prefix("_READONLY_") {
                    if let Some(value) = ctx.variables.get(var_name) {
                        output.push_str(&format!("declare -r {}=\"{}\"\n", var_name, value));
                    }
                }
            }
            return Ok(ExecResult::ok(output));
        }

        for arg in ctx.args {
            if let Some(eq_pos) = arg.find('=') {
                let name = &arg[..eq_pos];
                let value = &arg[eq_pos + 1..];
                // Set the variable
                ctx.variables.insert(name.to_string(), value.to_string());
                // Mark as readonly
                ctx.variables
                    .insert(format!("_READONLY_{}", name), "1".to_string());
            } else {
                // Just mark existing variable as readonly
                ctx.variables
                    .insert(format!("_READONLY_{}", arg), "1".to_string());
            }
        }
        Ok(ExecResult::ok(String::new()))
    }
}

/// times builtin - POSIX special built-in to display process times.
///
/// Prints accumulated user and system times for the shell and its children.
/// In Bashkit's virtual environment, returns zeros since we don't track real CPU time.
///
/// Output format:
/// ```text
/// 0m0.000s 0m0.000s
/// 0m0.000s 0m0.000s
/// ```
/// First line: shell user/system time. Second line: children user/system time.
pub struct Times;

#[async_trait]
impl Builtin for Times {
    async fn execute(&self, _ctx: Context<'_>) -> Result<ExecResult> {
        // In Bashkit's virtual environment, we don't have real process times
        // Return zeros as per POSIX format
        let output = "0m0.000s 0m0.000s\n0m0.000s 0m0.000s\n".to_string();
        Ok(ExecResult::ok(output))
    }
}

/// eval builtin - POSIX special built-in to construct and execute commands.
///
/// Concatenates arguments with spaces, then parses and executes the result.
/// This enables dynamic command construction.
///
/// Example:
/// ```bash
/// cmd="echo hello"
/// eval $cmd  # prints "hello"
/// ```
///
/// Note: eval stores the command in _EVAL_CMD for the interpreter to execute.
/// The interpreter handles the actual parsing and execution.
pub struct Eval;

#[async_trait]
impl Builtin for Eval {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        // Concatenate all arguments with spaces
        let cmd = ctx.args.join(" ");

        // Store for interpreter to execute
        // The interpreter will parse and execute this command
        ctx.variables.insert("_EVAL_CMD".to_string(), cmd);

        Ok(ExecResult::ok(String::new()))
    }
}

/// Known shopt option names. Maps to SHOPT_* variables.
const SHOPT_OPTIONS: &[&str] = &[
    "autocd",
    "cdspell",
    "checkhash",
    "checkjobs",
    "checkwinsize",
    "cmdhist",
    "compat31",
    "compat32",
    "compat40",
    "compat41",
    "compat42",
    "compat43",
    "compat44",
    "direxpand",
    "dirspell",
    "dotglob",
    "execfail",
    "expand_aliases",
    "extdebug",
    "extglob",
    "extquote",
    "failglob",
    "force_fignore",
    "globasciiranges",
    "globstar",
    "gnu_errfmt",
    "histappend",
    "histreedit",
    "histverify",
    "hostcomplete",
    "huponexit",
    "inherit_errexit",
    "interactive_comments",
    "lastpipe",
    "lithist",
    "localvar_inherit",
    "localvar_unset",
    "login_shell",
    "mailwarn",
    "no_empty_cmd_completion",
    "nocaseglob",
    "nocasematch",
    "nullglob",
    "progcomp",
    "progcomp_alias",
    "promptvars",
    "restricted_shell",
    "shift_verbose",
    "sourcepath",
    "xpg_echo",
];

/// shopt builtin - set/unset bash-specific shell options.
///
/// Usage:
/// - `shopt` - list all options with on/off status
/// - `shopt -s opt` - set (enable) option
/// - `shopt -u opt` - unset (disable) option
/// - `shopt -q opt` - query option (exit code only, no output)
/// - `shopt -p [opt]` - print in reusable `shopt -s/-u` format
/// - `shopt opt` - show status of specific option
///
/// Options stored as SHOPT_<name> variables ("1" = on, absent/other = off).
pub struct Shopt;

#[async_trait]
impl Builtin for Shopt {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            // List all options with their status
            let mut output = String::new();
            for opt in SHOPT_OPTIONS {
                let key = format!("SHOPT_{}", opt);
                let on = ctx.variables.get(&key).map(|v| v == "1").unwrap_or(false);
                output.push_str(&format!("{:<32}{}\n", opt, if on { "on" } else { "off" }));
            }
            return Ok(ExecResult::ok(output));
        }

        let mut mode: Option<char> = None; // 's'=set, 'u'=unset, 'q'=query, 'p'=print
        let mut opts: Vec<String> = Vec::new();

        for arg in ctx.args {
            if arg.starts_with('-') && opts.is_empty() {
                for ch in arg.chars().skip(1) {
                    match ch {
                        's' | 'u' | 'q' | 'p' => mode = Some(ch),
                        _ => {
                            return Ok(ExecResult::err(
                                format!("bash: shopt: -{}: invalid option\n", ch),
                                2,
                            ));
                        }
                    }
                }
            } else {
                opts.push(arg.to_string());
            }
        }

        match mode {
            Some('s') => {
                // Set options
                for opt in &opts {
                    if !SHOPT_OPTIONS.contains(&opt.as_str()) {
                        return Ok(ExecResult::err(
                            format!("bash: shopt: {}: invalid shell option name\n", opt),
                            1,
                        ));
                    }
                    ctx.variables
                        .insert(format!("SHOPT_{}", opt), "1".to_string());
                }
                Ok(ExecResult::ok(String::new()))
            }
            Some('u') => {
                // Unset options
                for opt in &opts {
                    if !SHOPT_OPTIONS.contains(&opt.as_str()) {
                        return Ok(ExecResult::err(
                            format!("bash: shopt: {}: invalid shell option name\n", opt),
                            1,
                        ));
                    }
                    ctx.variables.remove(&format!("SHOPT_{}", opt));
                }
                Ok(ExecResult::ok(String::new()))
            }
            Some('q') => {
                // Query: exit 0 if all named options are on, 1 otherwise
                let all_on = opts.iter().all(|opt| {
                    let key = format!("SHOPT_{}", opt);
                    ctx.variables.get(&key).map(|v| v == "1").unwrap_or(false)
                });
                Ok(ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: if all_on { 0 } else { 1 },
                    control_flow: crate::interpreter::ControlFlow::None,
                })
            }
            Some('p') => {
                // Print in reusable format
                let mut output = String::new();
                let list = if opts.is_empty() {
                    SHOPT_OPTIONS
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                } else {
                    opts.clone()
                };
                for opt in &list {
                    let key = format!("SHOPT_{}", opt);
                    let on = ctx.variables.get(&key).map(|v| v == "1").unwrap_or(false);
                    output.push_str(&format!("shopt {} {}\n", if on { "-s" } else { "-u" }, opt));
                }
                Ok(ExecResult::ok(output))
            }
            None => {
                // No flag: show status of named options
                if opts.is_empty() {
                    // Same as listing all
                    let mut output = String::new();
                    for opt in SHOPT_OPTIONS {
                        let key = format!("SHOPT_{}", opt);
                        let on = ctx.variables.get(&key).map(|v| v == "1").unwrap_or(false);
                        output.push_str(&format!("{:<32}{}\n", opt, if on { "on" } else { "off" }));
                    }
                    return Ok(ExecResult::ok(output));
                }
                let mut output = String::new();
                let mut any_invalid = false;
                for opt in &opts {
                    if !SHOPT_OPTIONS.contains(&opt.as_str()) {
                        output.push_str(&format!(
                            "bash: shopt: {}: invalid shell option name\n",
                            opt
                        ));
                        any_invalid = true;
                        continue;
                    }
                    let key = format!("SHOPT_{}", opt);
                    let on = ctx.variables.get(&key).map(|v| v == "1").unwrap_or(false);
                    output.push_str(&format!("{:<32}{}\n", opt, if on { "on" } else { "off" }));
                }
                if any_invalid {
                    Ok(ExecResult {
                        stdout: String::new(),
                        stderr: output,
                        exit_code: 1,
                        control_flow: crate::interpreter::ControlFlow::None,
                    })
                } else {
                    Ok(ExecResult::ok(output))
                }
            }
            _ => Ok(ExecResult::ok(String::new())),
        }
    }
}
