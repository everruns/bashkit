//! Variable manipulation builtins: set, unset, local, shift, readonly, eval, times
//!
//! POSIX special built-in utilities for variable management.

use async_trait::async_trait;

use super::{Builtin, BuiltinSideEffect, Context};
use crate::error::Result;
use crate::interpreter::{ExecResult, is_hidden_variable, is_internal_variable, is_valid_var_name};

/// unset builtin - remove variables
pub struct Unset;

#[async_trait]
impl Builtin for Unset {
    // THREAT[TM-INJ-009]: Block unset of internal variables and readonly variables
    async fn execute(&self, mut ctx: Context<'_>) -> Result<ExecResult> {
        let mut stderr = String::new();
        let mut exit_code = 0;
        for name in ctx.args {
            // Block unsetting internal marker variables (_READONLY_, _NAMEREF_, etc.)
            if is_internal_variable(name) {
                stderr.push_str(&format!(
                    "bash: unset: {name}: cannot unset: readonly variable\n"
                ));
                exit_code = 1;
                continue;
            }
            // Block unsetting readonly variables — attribute lookup is now a
            // single map probe instead of a format!("_READONLY_{}", ...).
            let is_readonly = ctx
                .shell
                .as_ref()
                .map(|s| s.is_var_readonly(name))
                .unwrap_or(false);
            if is_readonly {
                stderr.push_str(&format!(
                    "bash: unset: {name}: cannot unset: readonly variable\n"
                ));
                exit_code = 1;
                continue;
            }
            ctx.variables.remove(name);
            // Clear any non-readonly attributes / nameref binding for this name.
            if let Some(shell) = ctx.shell.as_mut() {
                shell.var_attrs.remove(name);
                shell.namerefs.remove(name);
            }
            // Note: env is immutable in our model - environment variables
            // are inherited and can't be unset by the shell
        }
        Ok(ExecResult {
            stderr: stderr.into(),
            exit_code,
            ..Default::default()
        })
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

/// Every `set -o` option in bash's listing order: (name, letter, variable,
/// on by default). Options with no effect in the sandbox (`hashall`,
/// `keyword`, `monitor`, ...) are still recorded so `$-`, `set -o` and
/// `shopt -o` report them like bash.
pub(crate) const SET_O_OPTIONS: &[(&str, Option<char>, &str, bool)] = &[
    ("allexport", Some('a'), "SHOPT_a", false),
    ("braceexpand", Some('B'), "SHOPT_B", true),
    ("emacs", None, "SHOPT_emacs", false),
    ("errexit", Some('e'), "SHOPT_e", false),
    ("errtrace", Some('E'), "SHOPT_E", false),
    ("functrace", Some('T'), "SHOPT_T", false),
    ("hashall", Some('h'), "SHOPT_h", true),
    ("histexpand", Some('H'), "SHOPT_H", false),
    ("history", None, "SHOPT_history", false),
    ("ignoreeof", None, "SHOPT_ignoreeof", false),
    (
        "interactive-comments",
        None,
        "SHOPT_interactive_comments",
        true,
    ),
    ("keyword", Some('k'), "SHOPT_k", false),
    ("monitor", Some('m'), "SHOPT_m", false),
    ("noclobber", Some('C'), "SHOPT_C", false),
    ("noexec", Some('n'), "SHOPT_n", false),
    ("noglob", Some('f'), "SHOPT_f", false),
    ("nolog", None, "SHOPT_nolog", false),
    ("notify", Some('b'), "SHOPT_b", false),
    ("nounset", Some('u'), "SHOPT_u", false),
    ("onecmd", Some('t'), "SHOPT_t", false),
    ("physical", Some('P'), "SHOPT_P", false),
    ("pipefail", None, "SHOPT_pipefail", false),
    ("posix", None, "SHOPT_posix", false),
    ("privileged", Some('p'), "SHOPT_p", false),
    ("verbose", Some('v'), "SHOPT_v", false),
    ("vi", None, "SHOPT_vi", false),
    ("xtrace", Some('x'), "SHOPT_x", false),
];

/// Record one `set -o` option. The `emacs` and `vi` editing modes exclude
/// each other: turning one on turns the other off.
fn set_o_store(variables: &mut std::collections::HashMap<String, String>, var: &str, on: bool) {
    if on {
        match var {
            "SHOPT_vi" => {
                variables.insert("SHOPT_emacs".to_string(), "0".to_string());
            }
            "SHOPT_emacs" => {
                variables.insert("SHOPT_vi".to_string(), "0".to_string());
            }
            _ => {}
        }
    }
    variables.insert(var.to_string(), if on { "1" } else { "0" }.to_string());
}

/// Whether `set -o` option `name` is on (`test -o name`); `None` when
/// bash has no such option.
pub(crate) fn set_o_option_on(
    variables: &std::collections::HashMap<String, String>,
    name: &str,
) -> Option<bool> {
    let (_, _, var, default) = set_option_by_name(name)?;
    Some(set_option_on(variables, var, *default))
}

/// The variable behind `set -o NAME` (`bash -o NAME`).
pub(crate) fn set_o_var_by_name(name: &str) -> Option<&'static str> {
    set_option_by_name(name).map(|o| o.2)
}

/// The variable behind `set -X` (`bash -X`).
pub(crate) fn set_o_var_by_letter(c: char) -> Option<&'static str> {
    set_option_by_letter(c).map(|o| o.2)
}

/// `$SHELLOPTS`: the `set -o` options that are on, colon-separated.
pub(crate) fn shellopts_value(variables: &std::collections::HashMap<String, String>) -> String {
    SET_O_OPTIONS
        .iter()
        .filter(|(_, _, var, default)| set_option_on(variables, var, *default))
        .map(|(name, ..)| *name)
        .collect::<Vec<_>>()
        .join(":")
}

/// `$BASHOPTS`: the `shopt` options that are on, colon-separated.
pub(crate) fn bashopts_value(variables: &std::collections::HashMap<String, String>) -> String {
    SHOPT_OPTIONS
        .iter()
        .filter(|(name, _)| shopt_on(variables, name))
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(":")
}

/// Letters in the order bash prints them in `$-`.
const DOLLAR_DASH_ORDER: &str = "abefhikmnptuvxBCEHPT";

fn set_option_by_name(
    name: &str,
) -> Option<&'static (&'static str, Option<char>, &'static str, bool)> {
    SET_O_OPTIONS.iter().find(|o| o.0 == name)
}

fn set_option_by_letter(
    c: char,
) -> Option<&'static (&'static str, Option<char>, &'static str, bool)> {
    SET_O_OPTIONS.iter().find(|o| o.1 == Some(c))
}

fn set_option_on(
    variables: &std::collections::HashMap<String, String>,
    var: &str,
    default: bool,
) -> bool {
    match variables.get(var).map(String::as_str) {
        Some("1") => true,
        Some("0") => false,
        _ => default,
    }
}

/// The value of `$-`: set option letters in bash order, then `c` (every
/// bashkit script runs like `bash -c`).
pub(crate) fn dollar_dash(variables: &std::collections::HashMap<String, String>) -> String {
    let mut out = String::new();
    for c in DOLLAR_DASH_ORDER.chars() {
        if let Some((_, _, var, default)) = set_option_by_letter(c)
            && set_option_on(variables, var, *default)
        {
            out.push(c);
        }
    }
    out.push('c');
    out
}

/// One `set -o` listing line (human-readable).
fn format_dash_o_line(variables: &std::collections::HashMap<String, String>, name: &str) -> String {
    let (_, _, var, default) = set_option_by_name(name).expect("known option");
    let state = if set_option_on(variables, var, *default) {
        "on"
    } else {
        "off"
    };
    format!("{:<15}\t{}\n", name, state)
}

/// One `set +o` listing line (re-executable).
fn format_plus_o_line(variables: &std::collections::HashMap<String, String>, name: &str) -> String {
    let (_, _, var, default) = set_option_by_name(name).expect("known option");
    let flag = if set_option_on(variables, var, *default) {
        "-o"
    } else {
        "+o"
    };
    format!("set {} {}\n", flag, name)
}

/// Format option display for `set -o` / `set +o`.
fn format_set_o(variables: &std::collections::HashMap<String, String>, dash: bool) -> String {
    SET_O_OPTIONS
        .iter()
        .map(|(name, ..)| {
            if dash {
                format_dash_o_line(variables, name)
            } else {
                format_plus_o_line(variables, name)
            }
        })
        .collect()
}

const SET_USAGE: &str =
    "set: usage: set [-abefhkmnptuvxBCEHPT] [-o option-name] [--] [-] [arg ...]\n";

impl Set {
    /// Create a SetPositional side effect.
    fn positional_effect(positional: &[&str]) -> BuiltinSideEffect {
        BuiltinSideEffect::SetPositional(positional.iter().map(|s| s.to_string()).collect())
    }
}

#[async_trait]
impl Builtin for Set {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            // Display all variables, filtering internal/hidden markers (TM-INF-017)
            let mut output = String::new();
            for (name, value) in ctx.variables.iter() {
                if !is_hidden_variable(name) {
                    output.push_str(&format!("{}={}\n", name, value));
                }
            }
            return Ok(ExecResult::ok(output));
        }

        // Parse everything first: an invalid option changes nothing (bash).
        let mut changes: Vec<(&'static str, bool)> = Vec::new();
        let mut listing: Option<bool> = None;
        let mut positional: Option<&[String]> = None;
        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            if arg == "--" {
                // Everything after `--` becomes positional parameters.
                positional = Some(&ctx.args[i + 1..]);
                break;
            }
            if arg == "-" {
                // `set -` turns off -x and -v and ends the options.
                changes.push(("SHOPT_x", false));
                changes.push(("SHOPT_v", false));
                if i + 1 < ctx.args.len() {
                    positional = Some(&ctx.args[i + 1..]);
                }
                break;
            }
            if !(arg.starts_with('-') || arg.starts_with('+')) || arg.len() < 2 {
                // Non-flag arg: this and everything after become positional params
                positional = Some(&ctx.args[i..]);
                break;
            }
            let enable = arg.starts_with('-');
            let sign = if enable { '-' } else { '+' };
            for opt in arg.chars().skip(1) {
                if opt == 'o' {
                    // `-o name`; a cluster's `o` takes the next argument, and
                    // `-o` with nothing after it lists the options.
                    if i + 1 < ctx.args.len() {
                        i += 1;
                        let name = &ctx.args[i];
                        match set_option_by_name(name) {
                            Some((_, _, var, _)) => changes.push((var, enable)),
                            None => {
                                return Ok(ExecResult::err(
                                    format!("bash: set: {name}: invalid option name\n"),
                                    2,
                                ));
                            }
                        }
                    } else {
                        listing = Some(enable);
                    }
                } else if let Some((_, _, var, _)) = set_option_by_letter(opt) {
                    changes.push((var, enable));
                } else {
                    return Ok(ExecResult::err(
                        format!("bash: set: {sign}{opt}: invalid option\n{SET_USAGE}"),
                        2,
                    ));
                }
            }
            i += 1;
        }

        for (var, on) in changes {
            set_o_store(ctx.variables, var, on);
        }
        let mut result = match listing {
            Some(dash) => ExecResult::ok(format_set_o(ctx.variables, dash)),
            None => ExecResult::ok(String::new()),
        };
        if let Some(args) = positional {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            result.side_effects.push(Self::positional_effect(&args));
        }
        Ok(result)
    }
}

/// shift builtin - shift positional parameters
pub struct Shift;

#[async_trait]
impl Builtin for Shift {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Number of positions to shift (default 1). A count above `$#` is
        // applied by the interpreter, which fails it with status 1.
        let n: usize = match ctx.args.first() {
            None => 1,
            Some(arg) => match arg.parse::<i64>() {
                Ok(v) if v >= 0 => usize::try_from(v).unwrap_or(usize::MAX),
                Ok(_) => {
                    return Ok(ExecResult::err(
                        format!("bash: shift: {arg}: shift count out of range\n"),
                        1,
                    ));
                }
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("bash: shift: {arg}: numeric argument required\n"),
                        1,
                    ));
                }
            },
        };

        let mut result = ExecResult::ok(String::new());
        result
            .side_effects
            .push(BuiltinSideEffect::ShiftPositional(n));
        Ok(result)
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
                // Validate variable name
                if !is_valid_var_name(name) {
                    return Ok(ExecResult::err(
                        format!("local: `{}': not a valid identifier\n", arg),
                        1,
                    ));
                }
                // THREAT[TM-INJ-009]: Block internal variable prefix injection via local
                if is_internal_variable(name) {
                    continue;
                }
                // Mark as local by setting it
                ctx.variables.insert(name.to_string(), value.to_string());
            } else {
                // THREAT[TM-INJ-009]: Block internal variable prefix injection via local
                if is_internal_variable(arg) {
                    continue;
                }
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
    async fn execute(&self, mut ctx: Context<'_>) -> Result<ExecResult> {
        // Handle -p flag to print readonly variables
        if ctx.args.first().map(|s| s.as_str()) == Some("-p") {
            let mut output = String::new();
            // Readonly markers live in `shell.var_attrs` now, not in
            // `variables` under the `_READONLY_X` prefix.
            if let Some(shell) = ctx.shell.as_ref() {
                let mut names: Vec<&str> = shell.readonly_names().collect();
                names.sort_unstable();
                for var_name in names {
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
                // THREAT[TM-INJ-013]: Block internal variable prefix injection via readonly
                if is_internal_variable(name) {
                    continue;
                }
                // Set the variable
                ctx.variables.insert(name.to_string(), value.to_string());
                if let Some(shell) = ctx.shell.as_mut() {
                    shell.mark_var_readonly(name);
                }
            } else {
                // THREAT[TM-INJ-013]: Block internal variable prefix injection via readonly
                if is_internal_variable(arg) {
                    continue;
                }
                // Just mark existing variable as readonly
                if let Some(shell) = ctx.shell.as_mut() {
                    shell.mark_var_readonly(arg);
                }
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

/// eval builtin stub — real execution is in `Interpreter::execute_eval`.
///
/// `eval` is a POSIX special builtin: it concatenates its arguments and then
/// parses and executes the result in the current shell context. That requires
/// the interpreter (parsing, execution, redirects), so the interpreter
/// intercepts `eval` before builtin dispatch (see `Interpreter::execute_eval`).
/// This stub exists only so the builtin name is registered (e.g. for
/// `type eval` lookups). It should never actually execute.
///
/// Historically this stub passed the command to the interpreter through a
/// stringly-typed `_EVAL_CMD` shell variable. That channel was removed: the
/// signal lived in the user-visible variable namespace and so had to be
/// guarded against injection. The interpreter now owns `eval` end to end.
pub struct Eval;

#[async_trait]
impl Builtin for Eval {
    async fn execute(&self, _ctx: Context<'_>) -> Result<ExecResult> {
        // Unreachable: interpreter intercepts eval before builtin dispatch.
        // If somehow reached, return error so it's obvious.
        Ok(ExecResult::err(
            "eval: internal error: should be handled by interpreter",
            1,
        ))
    }
}

/// Every `shopt` option in bash 5.2's listing order, with its default for a
/// non-interactive shell. Stored as `SHOPT_<name>`: "1" on, "0" off, absent
/// means the default. Options with no effect in the sandbox (`checkwinsize`,
/// `cdable_vars`, ...) are still recorded and reported like bash.
pub(crate) const SHOPT_OPTIONS: &[(&str, bool)] = &[
    ("autocd", false),
    ("assoc_expand_once", false),
    ("cdable_vars", false),
    ("cdspell", false),
    ("checkhash", false),
    ("checkjobs", false),
    ("checkwinsize", true),
    ("cmdhist", true),
    ("compat31", false),
    ("compat32", false),
    ("compat40", false),
    ("compat41", false),
    ("compat42", false),
    ("compat43", false),
    ("compat44", false),
    ("complete_fullquote", true),
    ("direxpand", false),
    ("dirspell", false),
    ("dotglob", false),
    ("execfail", false),
    ("expand_aliases", false),
    ("extdebug", false),
    ("extglob", false),
    ("extquote", true),
    ("failglob", false),
    ("force_fignore", true),
    ("globasciiranges", true),
    ("globskipdots", true),
    ("globstar", false),
    ("gnu_errfmt", false),
    ("histappend", false),
    ("histreedit", false),
    ("histverify", false),
    ("hostcomplete", true),
    ("huponexit", false),
    ("inherit_errexit", false),
    ("interactive_comments", true),
    ("lastpipe", false),
    ("lithist", false),
    ("localvar_inherit", false),
    ("localvar_unset", false),
    ("login_shell", false),
    ("mailwarn", false),
    ("no_empty_cmd_completion", false),
    ("nocaseglob", false),
    ("nocasematch", false),
    ("noexpand_translation", false),
    ("nullglob", false),
    ("patsub_replacement", true),
    ("progcomp", true),
    ("progcomp_alias", false),
    ("promptvars", true),
    ("restricted_shell", false),
    ("shift_verbose", false),
    ("sourcepath", true),
    ("varredir_close", false),
    ("xpg_echo", false),
];

pub(crate) fn shopt_known(name: &str) -> bool {
    SHOPT_OPTIONS.iter().any(|(n, _)| *n == name)
}

/// Whether shopt option `name` is on (its default when never set).
pub(crate) fn shopt_on(variables: &std::collections::HashMap<String, String>, name: &str) -> bool {
    let default = SHOPT_OPTIONS
        .iter()
        .find(|(n, _)| *n == name)
        .is_some_and(|(_, d)| *d);
    match variables.get(&format!("SHOPT_{name}")).map(String::as_str) {
        Some("1") => true,
        Some("0") => false,
        _ => default,
    }
}

/// `shopt -o`: the same verbs applied to `set -o` options.
fn shopt_set_o(
    variables: &mut std::collections::HashMap<String, String>,
    mode: Option<char>,
    opts: &[String],
) -> ExecResult {
    let names: Vec<&str> = if opts.is_empty() {
        SET_O_OPTIONS.iter().map(|(name, ..)| *name).collect()
    } else {
        opts.iter().map(String::as_str).collect()
    };
    // Each unknown name is reported and skipped; the rest still apply.
    // bash keeps status 0 for `-s`/`-u` and fails the other verbs.
    let mut out = String::new();
    let mut err = String::new();
    let mut status = 0;
    for name in names {
        let Some((_, _, var, default)) = set_option_by_name(name) else {
            err.push_str(&format!("bash: shopt: {name}: invalid option name\n"));
            if !matches!(mode, Some('s' | 'u')) {
                status = 1;
            }
            continue;
        };
        match mode {
            Some('s') => set_o_store(variables, var, true),
            Some('u') => set_o_store(variables, var, false),
            Some('q') => {
                if !set_option_on(variables, var, *default) {
                    status = 1;
                }
            }
            Some('p') => out.push_str(&format_plus_o_line(variables, name)),
            _ => {
                // Naming options reports their state in the status too.
                if !opts.is_empty() && !set_option_on(variables, var, *default) {
                    status = 1;
                }
                out.push_str(&format_dash_o_line(variables, name));
            }
        }
    }
    let mut result = ExecResult::ok(out);
    result.stderr = err.into();
    result.exit_code = status;
    result
}

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
        let mut mode: Option<char> = None; // 's'=set, 'u'=unset, 'q'=query, 'p'=print
        let mut print = false; // -p alongside -s/-u: list those options
        let mut opts: Vec<String> = Vec::new();
        let mut set_o = false; // -o: operate on `set -o` options

        for arg in ctx.args {
            if arg.starts_with('-') && arg.len() > 1 && opts.is_empty() {
                for ch in arg.chars().skip(1) {
                    match ch {
                        'o' => set_o = true,
                        'p' => print = true,
                        's' | 'u' | 'q' => mode = Some(ch),
                        _ => {
                            return Ok(ExecResult::err(
                                format!(
                                    "bash: shopt: -{}: invalid option\nshopt: usage: shopt [-pqsu] [-o] [optname ...]\n",
                                    ch
                                ),
                                2,
                            ));
                        }
                    }
                }
            } else {
                opts.push(arg.to_string());
            }
        }
        if mode.is_none() && print {
            mode = Some('p');
        }

        if set_o {
            return Ok(shopt_set_o(ctx.variables, mode, &opts));
        }

        // `shopt -s`/`-u` with no names lists the options that are on/off.
        if opts.is_empty() && matches!(mode, Some('s' | 'u')) {
            let want = mode == Some('s');
            let mut out = String::new();
            for (opt, _) in SHOPT_OPTIONS {
                let on = shopt_on(ctx.variables, opt);
                if on == want {
                    out.push_str(&Self::line(opt, on, print));
                }
            }
            return Ok(ExecResult::ok(out));
        }

        // Each unknown name is reported and skipped (status 1); the
        // valid names around it still apply.
        let mut err = String::new();
        opts.retain(|opt| {
            let known = shopt_known(opt);
            if !known {
                err.push_str(&format!("bash: shopt: {opt}: invalid shell option name\n"));
            }
            known
        });
        let mut result = Self::run_valid(ctx, mode, opts);
        if !err.is_empty() {
            result.stderr = err.into();
            result.exit_code = 1;
        }
        Ok(result)
    }
}

impl Shopt {
    fn line(opt: &str, on: bool, reusable: bool) -> String {
        if reusable {
            format!("shopt {} {}\n", if on { "-s" } else { "-u" }, opt)
        } else {
            format!("{:<15}\t{}\n", opt, if on { "on" } else { "off" })
        }
    }

    /// Run `shopt` on names already checked against `SHOPT_OPTIONS`.
    /// Listing or querying named options fails (status 1) when any of
    /// them is off, as in bash.
    fn run_valid(ctx: Context<'_>, mode: Option<char>, opts: Vec<String>) -> ExecResult {
        match mode {
            Some(c @ ('s' | 'u')) => {
                for opt in &opts {
                    ctx.variables.insert(
                        format!("SHOPT_{}", opt),
                        if c == 's' { "1" } else { "0" }.to_string(),
                    );
                }
                ExecResult::ok(String::new())
            }
            _ => {
                let names: Vec<&str> = if opts.is_empty() {
                    SHOPT_OPTIONS.iter().map(|(n, _)| *n).collect()
                } else {
                    opts.iter().map(String::as_str).collect()
                };
                let mut output = String::new();
                let mut all_on = true;
                for opt in names {
                    let on = shopt_on(ctx.variables, opt);
                    all_on &= on;
                    if mode != Some('q') {
                        output.push_str(&Self::line(opt, on, mode == Some('p')));
                    }
                }
                let code = if opts.is_empty() || all_on { 0 } else { 1 };
                ExecResult::with_code(output, code)
            }
        }
    }
}
