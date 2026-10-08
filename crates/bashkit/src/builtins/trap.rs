//! Trap builtin — register signal/event handlers.
//!
//! Directly mutates traps via [`ShellRef`](super::ShellRef).
//!
//! Handlers are keyed by canonical name: `EXIT`, `DEBUG`, `ERR`, `RETURN`,
//! or a signal name without its `SIG` prefix (`INT`). Specs are accepted the
//! way bash takes them (`0`, `2`, `int`, `SIGINT`) and listed in bash's
//! order (EXIT, signals by number, DEBUG, ERR, RETURN) with re-readable
//! single quoting.

use async_trait::async_trait;

use super::helpers::single_quote;
use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Linux signal names, numbered from 1.
const SIGNALS: [&str; 31] = [
    "HUP", "INT", "QUIT", "ILL", "TRAP", "ABRT", "BUS", "FPE", "KILL", "USR1", "SEGV", "USR2",
    "PIPE", "ALRM", "TERM", "STKFLT", "CHLD", "CONT", "STOP", "TSTP", "TTIN", "TTOU", "URG",
    "XCPU", "XFSZ", "VTALRM", "PROF", "WINCH", "IO", "PWR", "SYS",
];

/// Pseudo-signals after the real ones, in bash's listing order.
const PSEUDO: [&str; 3] = ["DEBUG", "ERR", "RETURN"];

/// Canonical key and listing rank for a trap spec, `None` if invalid.
fn canonical(spec: &str) -> Option<(usize, String)> {
    if !spec.is_empty() && spec.bytes().all(|b| b.is_ascii_digit()) {
        let n: usize = spec.parse().ok()?;
        return match n {
            0 => Some((0, "EXIT".to_string())),
            n if n <= SIGNALS.len() => Some((n, SIGNALS[n - 1].to_string())),
            _ => None,
        };
    }
    let upper = spec.to_ascii_uppercase();
    let bare = upper.strip_prefix("SIG").unwrap_or(&upper);
    if bare == "EXIT" {
        return Some((0, "EXIT".to_string()));
    }
    if let Some(i) = SIGNALS.iter().position(|s| *s == bare) {
        return Some((i + 1, bare.to_string()));
    }
    // DEBUG/ERR/RETURN take no `SIG` prefix.
    PSEUDO
        .iter()
        .position(|s| *s == upper)
        .map(|i| (SIGNALS.len() + 1 + i, upper))
}

/// Listing name of a canonical key (`INT` → `SIGINT`).
fn display_name(key: &str) -> String {
    if key == "EXIT" || PSEUDO.contains(&key) {
        key.to_string()
    } else {
        format!("SIG{key}")
    }
}

fn format_trap(key: &str, cmd: &str) -> String {
    format!("trap -- {} {}\n", single_quote(cmd), display_name(key))
}

fn invalid_spec(spec: &str) -> String {
    format!("bash: trap: {spec}: invalid signal specification\n")
}

/// `trap` builtin — register signal/event handlers.
///
/// Usage:
/// - `trap` / `trap -p` — list all traps
/// - `trap -p SIGNAL...` — print the given traps
/// - `trap -l` — list signal names
/// - `trap COMMAND SIGNAL...` — set trap handler (`''` ignores the signal)
/// - `trap - SIGNAL...` / `trap SIGNAL` — reset to the default
pub struct Trap;

#[async_trait]
impl Builtin for Trap {
    async fn execute(&self, mut ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_mut() else {
            return Ok(ExecResult::ok(String::new()));
        };

        let mut args: &[String] = ctx.args;
        let mut print = false;
        while let Some(first) = args.first() {
            match first.as_str() {
                "--" => {
                    args = &args[1..];
                    break;
                }
                "-p" => {
                    print = true;
                    args = &args[1..];
                }
                "-l" => {
                    let mut output = String::new();
                    for (i, name) in SIGNALS.iter().enumerate() {
                        output.push_str(&format!("{:2}) SIG{name}\n", i + 1));
                    }
                    return Ok(ExecResult::ok(output));
                }
                // `-` alone is a handler (reset); any other `-x` is a bad option.
                opt if opt.len() > 1 && opt.starts_with('-') => {
                    let c = opt[1..].chars().next().unwrap_or('-');
                    return Ok(ExecResult::err(
                        format!(
                            "trap: -{c}: invalid option\ntrap: usage: trap [-lp] [[action] signal_spec ...]\n"
                        ),
                        2,
                    ));
                }
                _ => break,
            }
        }

        if args.is_empty() || print {
            let mut output = String::new();
            let mut stderr = String::new();
            if args.is_empty() {
                let mut sorted: Vec<_> = shell
                    .traps
                    .iter()
                    .map(|(key, cmd)| {
                        let rank = canonical(key).map_or(usize::MAX, |(r, _)| r);
                        (rank, key, cmd)
                    })
                    .collect();
                sorted.sort();
                for (_, key, cmd) in sorted {
                    output.push_str(&format_trap(key, cmd));
                }
            } else {
                for spec in args {
                    match canonical(spec) {
                        Some((_, key)) => {
                            if let Some(cmd) = shell.traps.get(&key) {
                                output.push_str(&format_trap(&key, cmd));
                            }
                        }
                        None => stderr.push_str(&invalid_spec(spec)),
                    }
                }
            }
            let code = i32::from(!stderr.is_empty());
            return Ok(ExecResult {
                stdout: output.into(),
                stderr: stderr.into(),
                exit_code: code,
                ..Default::default()
            });
        }

        // `trap SIGNAL` and `trap N ...` (first operand a number) reset
        // every operand; otherwise the first operand is the handler.
        // (` 42 `: bash skips blanks around the number.)
        let first = args[0].trim_matches([' ', '\t']);
        let reset_all =
            args.len() == 1 || (!first.is_empty() && first.bytes().all(|b| b.is_ascii_digit()));
        let (handler, specs) = if reset_all {
            (None, args)
        } else {
            let cmd = &args[0];
            (if cmd == "-" { None } else { Some(cmd) }, &args[1..])
        };
        let mut stderr = String::new();
        for spec in specs {
            match canonical(spec) {
                Some((_, key)) => {
                    if key == "ERR" {
                        *shell.err_trap_dormant = false;
                    }
                    match handler {
                        Some(cmd) => {
                            shell.traps.insert(key, cmd.clone());
                        }
                        None => {
                            shell.traps.remove(&key);
                        }
                    }
                }
                None => stderr.push_str(&invalid_spec(spec)),
            }
        }
        let code = i32::from(!stderr.is_empty());
        Ok(ExecResult {
            stderr: stderr.into(),
            exit_code: code,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_accepts_bash_spellings() {
        assert_eq!(canonical("0"), Some((0, "EXIT".to_string())));
        assert_eq!(canonical("exit"), Some((0, "EXIT".to_string())));
        assert_eq!(canonical("2"), Some((2, "INT".to_string())));
        assert_eq!(canonical("sigint"), Some((2, "INT".to_string())));
        assert_eq!(canonical("TERM"), Some((15, "TERM".to_string())));
        assert_eq!(canonical("ERR").map(|(_, k)| k), Some("ERR".to_string()));
        assert_eq!(canonical("SIGERR"), None);
        assert_eq!(canonical("FOO"), None);
        assert_eq!(canonical("99"), None);
        assert_eq!(canonical("-1"), None);
    }

    #[test]
    fn listing_quotes_and_prefixes() {
        assert_eq!(format_trap("INT", ""), "trap -- '' SIGINT\n");
        assert_eq!(format_trap("EXIT", "it's"), "trap -- 'it'\\''s' EXIT\n");
    }
}
