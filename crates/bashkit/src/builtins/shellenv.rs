//! Process-environment builtins: umask, ulimit, locale, enable
//!
//! Decision: umask and ulimit state lives in internal shell variables
//! (`_UMASK`, `_ULIMIT_<opt>`), so subshells and `$(...)` inherit it and
//! their changes stay local, with no extra interpreter state. The values are
//! virtual: the umask is reported but does not yet change VFS file modes, and
//! ulimit values are fixed sandbox defaults that never expose host limits
//! (TM-INF-008). Real resource caps are `ExecutionLimits`. See L-ENV-001.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

const UMASK_VAR: &str = "_UMASK";
const DEFAULT_UMASK: u32 = 0o022;

fn current_umask(ctx: &Context<'_>) -> u32 {
    ctx.variables
        .get(UMASK_VAR)
        .and_then(|v| u32::from_str_radix(v, 8).ok())
        .unwrap_or(DEFAULT_UMASK)
}

fn symbolic_umask(mask: u32) -> String {
    let perms = |shift: u32| {
        let allowed = !mask >> shift & 0o7;
        let mut s = String::new();
        if allowed & 4 != 0 {
            s.push('r');
        }
        if allowed & 2 != 0 {
            s.push('w');
        }
        if allowed & 1 != 0 {
            s.push('x');
        }
        s
    };
    format!("u={},g={},o={}", perms(6), perms(3), perms(0))
}

/// Apply a symbolic umask like `u=rwx,g=rx,o=` or `g-w` to `mask`.
fn apply_symbolic_umask(spec: &str, mask: u32) -> Option<u32> {
    // Work on the allowed bits, then invert.
    let mut allowed = !mask & 0o777;
    for clause in spec.split(',') {
        let op_at = clause.find(['=', '+', '-'])?;
        let (who, rest) = clause.split_at(op_at);
        let op = rest.chars().next()?;
        let mut bits = 0;
        for c in rest[1..].chars() {
            bits |= match c {
                'r' => 4,
                'w' => 2,
                'x' => 1,
                _ => return None,
            };
        }
        let mut shifts = Vec::new();
        for c in if who.is_empty() { "a" } else { who }.chars() {
            match c {
                'u' => shifts.push(6),
                'g' => shifts.push(3),
                'o' => shifts.push(0),
                'a' => shifts.extend([6, 3, 0]),
                _ => return None,
            }
        }
        for s in shifts {
            match op {
                '=' => allowed = (allowed & !(0o7 << s)) | (bits << s),
                '+' => allowed |= bits << s,
                _ => allowed &= !(bits << s),
            }
        }
    }
    Some(!allowed & 0o777)
}

/// `umask` builtin.
pub struct Umask;

#[async_trait]
impl Builtin for Umask {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (mut symbolic, mut reusable) = (false, false);
        let mut value: Option<&str> = None;
        for arg in ctx.args {
            match arg.as_str() {
                "-S" => symbolic = true,
                "-p" => reusable = true,
                "--" => {}
                a if a.starts_with('-') && a.len() > 1 && value.is_none() => {
                    return Ok(ExecResult::err(
                        format!(
                            "bash: umask: {a}: invalid option\numask: usage: umask [-p] [-S] [mode]\n"
                        ),
                        2,
                    ));
                }
                // bash uses the first operand and ignores the rest.
                a if value.is_none() => value = Some(a),
                _ => {}
            }
        }
        let mask = current_umask(&ctx);
        let Some(v) = value else {
            let shown = if symbolic {
                symbolic_umask(mask)
            } else {
                format!("{mask:04o}")
            };
            let line = if reusable {
                let flag = if symbolic { "-S " } else { "" };
                format!("umask {flag}{shown}\n")
            } else {
                format!("{shown}\n")
            };
            return Ok(ExecResult::ok(line));
        };
        let new_mask = if v.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
            u32::from_str_radix(v, 8).ok().filter(|m| *m <= 0o777)
        } else {
            apply_symbolic_umask(v, mask)
        };
        match new_mask {
            Some(m) => {
                ctx.variables
                    .insert(UMASK_VAR.to_string(), format!("{m:04o}"));
                Ok(ExecResult::ok(String::new()))
            }
            None if v.bytes().all(|b| b.is_ascii_digit()) => Ok(ExecResult::err(
                format!("bash: umask: {v}: octal number out of range\n"),
                1,
            )),
            None => Ok(ExecResult::err(
                format!("bash: umask: `{}': invalid symbolic mode operator\n", v),
                1,
            )),
        }
    }
}

/// (option, label, unit, default) for each ulimit resource, in `-a` order.
const ULIMITS: &[(char, &str, &str, &str)] = &[
    (
        'R',
        "real-time non-blocking time",
        "microseconds",
        "unlimited",
    ),
    ('c', "core file size", "blocks", "0"),
    ('d', "data seg size", "kbytes", "unlimited"),
    ('e', "scheduling priority", "", "0"),
    ('f', "file size", "blocks", "unlimited"),
    ('i', "pending signals", "", "4096"),
    ('l', "max locked memory", "kbytes", "8192"),
    ('m', "max memory size", "kbytes", "unlimited"),
    ('n', "open files", "", "1024"),
    ('p', "pipe size", "512 bytes", "8"),
    ('q', "POSIX message queues", "bytes", "819200"),
    ('r', "real-time priority", "", "0"),
    ('s', "stack size", "kbytes", "8192"),
    ('t', "cpu time", "seconds", "unlimited"),
    ('u', "max user processes", "", "4096"),
    ('v', "virtual memory", "kbytes", "unlimited"),
    ('x', "file locks", "", "unlimited"),
];

fn limit_key(opt: char, hard: bool) -> String {
    format!("_ULIMIT_{}{opt}", if hard { "H" } else { "S" })
}

fn limit_value(ctx: &Context<'_>, opt: char, hard: bool) -> String {
    if let Some(v) = ctx.variables.get(&limit_key(opt, hard)) {
        return v.clone();
    }
    let default = ULIMITS
        .iter()
        .find(|l| l.0 == opt)
        .map_or("unlimited", |l| l.3);
    // Hard limits default to the soft value, except open files (like Linux).
    if hard && opt == 'n' {
        "1048576".to_string()
    } else {
        default.to_string()
    }
}

/// One `ulimit -a` row, laid out like bash: `%-20s %19s value`.
fn ulimit_line(opt: char, value: &str) -> String {
    let (label, unit) = ULIMITS
        .iter()
        .find(|l| l.0 == opt)
        .map_or(("", ""), |l| (l.1, l.2));
    let head = if unit.is_empty() {
        format!("(-{opt})")
    } else {
        format!("({unit}, -{opt})")
    };
    format!("{label:<20} {head:>19} {value}\n")
}

fn as_number(v: &str) -> Option<u128> {
    if v == "unlimited" {
        Some(u128::MAX)
    } else {
        v.parse().ok()
    }
}

/// `ulimit` builtin.
pub struct Ulimit;

#[async_trait]
impl Builtin for Ulimit {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (mut hard, mut soft, mut all) = (false, false, false);
        let mut opts: Vec<char> = Vec::new();
        let mut value: Option<String> = None;
        for arg in ctx.args {
            if let Some(flags) = arg.strip_prefix('-').filter(|f| !f.is_empty()) {
                for c in flags.chars() {
                    match c {
                        'H' => hard = true,
                        'S' => soft = true,
                        'a' => all = true,
                        c if ULIMITS.iter().any(|l| l.0 == c) => opts.push(c),
                        _ => {
                            return Ok(ExecResult::err(
                                format!(
                                    "bash: ulimit: -{c}: invalid option\nulimit: usage: ulimit [-SHabcdefiklmnpqrstuvxPRT] [limit]\n"
                                ),
                                2,
                            ));
                        }
                    }
                }
            } else {
                value = Some(arg.clone());
            }
        }
        if all {
            let mut out = String::new();
            for (opt, ..) in ULIMITS {
                out.push_str(&ulimit_line(*opt, &limit_value(&ctx, *opt, hard && !soft)));
            }
            return Ok(ExecResult::ok(out));
        }
        if opts.is_empty() {
            opts.push('f');
        }
        let Some(v) = value else {
            let multi = opts.len() > 1;
            let mut out = String::new();
            for opt in opts {
                let v = limit_value(&ctx, opt, hard && !soft);
                if multi {
                    out.push_str(&ulimit_line(opt, &v));
                } else {
                    out.push_str(&format!("{v}\n"));
                }
            }
            return Ok(ExecResult::ok(out));
        };
        let normalized = match v.as_str() {
            "unlimited" | "hard" | "soft" => v.clone(),
            n if n.bytes().all(|b| b.is_ascii_digit()) => n.to_string(),
            _ => {
                return Ok(ExecResult::err(
                    format!("bash: ulimit: {v}: invalid number\n"),
                    1,
                ));
            }
        };
        // bash scales kbytes/blocks limits to bytes and rejects a value that
        // no longer fits a 64-bit limit.
        if normalized.bytes().all(|b| b.is_ascii_digit()) {
            let n: u128 = normalized.parse().unwrap_or(u128::MAX);
            for opt in &opts {
                let unit = ULIMITS.iter().find(|l| l.0 == *opt).map_or("", |l| l.2);
                let factor: u128 = if matches!(unit, "kbytes" | "blocks") {
                    1024
                } else {
                    1
                };
                if n.saturating_mul(factor) >= u64::MAX as u128 {
                    return Ok(ExecResult::err(
                        format!("bash: ulimit: {v}: limit out of range\n"),
                        1,
                    ));
                }
            }
        }
        let (set_soft, set_hard) = if hard || soft {
            (soft, hard)
        } else {
            (true, true)
        };
        for opt in opts {
            let label = ULIMITS.iter().find(|l| l.0 == opt).map_or("", |l| l.1);
            let cur_hard = limit_value(&ctx, opt, true);
            let new = match normalized.as_str() {
                "hard" => cur_hard.clone(),
                "soft" => limit_value(&ctx, opt, false),
                n => n.to_string(),
            };
            // Raising past the hard limit needs privilege the sandbox lacks.
            if as_number(&new) > as_number(&cur_hard) {
                return Ok(ExecResult::err(
                    format!(
                        "bash: ulimit: {label}: cannot modify limit: Operation not permitted\n"
                    ),
                    1,
                ));
            }
            if set_hard {
                ctx.variables.insert(limit_key(opt, true), new.clone());
            }
            if set_soft {
                ctx.variables.insert(limit_key(opt, false), new);
            }
        }
        Ok(ExecResult::ok(String::new()))
    }
}

/// `locale` builtin - report the sandbox's locale (POSIX unless LANG/LC_* set).
pub struct Locale;

const LC_CATEGORIES: &[&str] = &[
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "LC_PAPER",
    "LC_NAME",
    "LC_ADDRESS",
    "LC_TELEPHONE",
    "LC_MEASUREMENT",
    "LC_IDENTIFICATION",
];

#[async_trait]
impl Builtin for Locale {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: locale [-a]\nGet locale-specific information.\n\n  -a, --all-locales\twrite names of available locales\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("locale (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        if ctx.args.iter().any(|a| a == "-a" || a == "--all-locales") {
            return Ok(ExecResult::ok("C\nC.utf8\nPOSIX\n".to_string()));
        }
        if let Some(bad) = ctx.args.first() {
            return Ok(ExecResult::err(
                format!("locale: unsupported argument '{bad}'\n"),
                1,
            ));
        }
        let get = |k: &str| {
            ctx.env
                .get(k)
                .or_else(|| ctx.variables.get(k))
                .filter(|v| !v.is_empty())
                .cloned()
        };
        let lang = get("LANG");
        let lc_all = get("LC_ALL");
        let mut out = format!(
            "LANG={}\nLANGUAGE={}\n",
            lang.clone().unwrap_or_default(),
            get("LANGUAGE").unwrap_or_default()
        );
        for cat in LC_CATEGORIES {
            let explicit = get(cat);
            let value = lc_all
                .clone()
                .or(explicit.clone())
                .or(lang.clone())
                .unwrap_or_else(|| "POSIX".to_string());
            if explicit.is_some() && lc_all.is_none() {
                out.push_str(&format!("{cat}={value}\n"));
            } else {
                out.push_str(&format!("{cat}=\"{value}\"\n"));
            }
        }
        out.push_str(&format!("LC_ALL={}\n", lc_all.unwrap_or_default()));
        Ok(ExecResult::ok(out))
    }
}

/// `enable` builtin - list builtins (enabling/disabling is not supported).
pub struct Enable;

#[async_trait]
impl Builtin for Enable {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_ref() else {
            return Ok(ExecResult::err(
                "bash: enable: not available here\n".to_string(),
                1,
            ));
        };
        let mut names: Vec<String> = shell.builtins.keys().cloned().collect();
        if let Some(host) = shell.host_builtins {
            names.extend(host.names());
        }
        names.sort();
        names.dedup();
        let mut operands = Vec::new();
        for arg in ctx.args {
            match arg.as_str() {
                "-a" | "-p" | "-s" => {}
                "-n" | "-d" | "-f" => {
                    return Ok(ExecResult::err(
                        format!(
                            "bash: enable: {arg}: disabling or loading builtins is not supported\n"
                        ),
                        1,
                    ));
                }
                a if a.starts_with('-') => {
                    return Ok(ExecResult::err(
                        format!(
                            "bash: enable: {a}: invalid option\nenable: usage: enable [-a] [-dnps] [-f filename] [name ...]\n"
                        ),
                        2,
                    ));
                }
                a => operands.push(a),
            }
        }
        if operands.is_empty() {
            let out: String = names.iter().map(|n| format!("enable {n}\n")).collect();
            return Ok(ExecResult::ok(out));
        }
        let mut err = String::new();
        for name in operands {
            if !names.iter().any(|n| n == name) {
                err.push_str(&format!("bash: enable: {name}: not a shell builtin\n"));
            }
        }
        Ok(ExecResult {
            exit_code: i32::from(!err.is_empty()),
            stderr: err.into(),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn umask_symbolic_round_trip() {
        assert_eq!(symbolic_umask(0o022), "u=rwx,g=rx,o=rx");
        assert_eq!(symbolic_umask(0o077), "u=rwx,g=,o=");
        assert_eq!(apply_symbolic_umask("u=rwx,g=,o=", 0o022), Some(0o077));
        assert_eq!(apply_symbolic_umask("g-w", 0o002), Some(0o022));
        assert_eq!(apply_symbolic_umask("o+w", 0o022), Some(0o020));
        assert_eq!(apply_symbolic_umask("q=r", 0o022), None);
    }
}
