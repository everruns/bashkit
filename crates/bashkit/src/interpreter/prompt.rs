//! Prompt string decoding: bash's `decode_prompt_string` for `${x@P}` and the
//! PS1/PS2/PS4 an interactive `bash -i` prints.
//!
//! Decisions:
//! - Two passes, as in bash: backslash escapes (`\u \h \w \t \!` ...) are
//!   decoded first, then, with `shopt promptvars` on (the default), the
//!   result is expanded like an unquoted here-document body (parameters,
//!   `$(...)`, arithmetic; `"` stays literal). Text an escape substitutes
//!   (a directory named `$foo`) is backslash-quoted so the second pass
//!   leaves it alone.
//! - Identity comes from the sandbox, never the host: `\u` is the configured
//!   user (`whoami`), `\h`/`\H` the configured hostname, times use the
//!   virtual `date` clock and `TZ`, `\l` is `tty` (bashkit has no terminal
//!   device), `\s`/`\v`/`\V` follow `$0` and `$BASH_VERSION`.
//! - `\[` and `\]` (readline's non-printing markers) expand to nothing.
//! - Reparses share request fuel, input accounting and cancellation. Cumulative
//!   prompt depth follows `max_ast_depth`, capped at eight for native stack safety;
//!   fresh shallow parsers must never reset the recursive polling limit.

use super::Interpreter;
use crate::error::Result;
use crate::parser::Parser;

impl Interpreter {
    /// `${name@P}`: decode `value` as a prompt string, then expand it.
    /// Boxed here, out of line, so `expand_word_inner` holds only a pointer
    /// (stack budget, see `stack_overflow_regression_tests`).
    #[inline(never)]
    pub(super) fn expand_prompt_string(
        &mut self,
        value: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + '_>> {
        Box::pin(self.expand_prompt_work(value))
    }

    async fn expand_prompt_work(&mut self, value: String) -> Result<String> {
        // THREAT[TM-DOS-133]: Charge before decoding or recursively polling.
        self.execution_budget
            .consume_work(u64::try_from(value.len()).unwrap_or(u64::MAX).max(1))?;
        self.execution_budget.consume_input(value.len())?;
        self.counters.push_prompt(&self.limits)?;
        let result = self.expand_prompt_body(&value).await;
        self.counters.pop_prompt();
        result
    }

    async fn expand_prompt_body(&mut self, value: &str) -> Result<String> {
        let decoded = self.decode_prompt_escapes(value);
        if !crate::builtins::shopt_on(&self.scoped.variables, "promptvars") {
            return Ok(unquote_prompt_text(&decoded));
        }
        let word = Parser::parse_prompt_body(
            &decoded,
            self.limits.max_ast_depth,
            self.limits.max_parser_operations,
            self.execution_budget.clone(),
        )?;
        self.expand_word(&word).await
    }

    /// First pass of prompt expansion: backslash escapes only.
    #[inline(never)]
    pub(super) fn decode_prompt_escapes(&self, prompt: &str) -> String {
        let mut out = String::with_capacity(prompt.len());
        let chars: Vec<char> = prompt.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c != '\\' || i + 1 >= chars.len() {
                out.push(c);
                i += 1;
                continue;
            }
            let esc = chars[i + 1];
            i += 2;
            match esc {
                'a' => out.push('\x07'),
                'e' => out.push('\x1b'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                '[' | ']' => {}
                '\\' => out.push_str("\\\\"),
                '$' => {
                    if self.expand_variable("EUID") == "0" {
                        out.push('#');
                    } else {
                        out.push_str("\\$");
                    }
                }
                '0'..='7' => {
                    // Up to three octal digits, wrapped to a byte (`\555` is `m`).
                    let mut n: u32 = 0;
                    let mut digits = 0;
                    let mut j = i - 1;
                    while digits < 3 && j < chars.len() && ('0'..='7').contains(&chars[j]) {
                        n = n * 8 + chars[j].to_digit(8).unwrap_or(0);
                        digits += 1;
                        j += 1;
                    }
                    let byte = (n & 0xff) as u8;
                    if byte == 0 {
                        // A NUL ends the prompt in bash; keep the text as written.
                        out.push('\\');
                        out.push(esc);
                    } else {
                        i = j;
                        push_quoted(&mut out, &(byte as char).to_string());
                    }
                }
                'u' => push_quoted(&mut out, &self.prompt_user),
                'h' => {
                    let host = self.prompt_host.split('.').next().unwrap_or("");
                    push_quoted(&mut out, host);
                }
                'H' => push_quoted(&mut out, &self.prompt_host.clone()),
                'w' | 'W' => {
                    let dir = self.prompt_dir(esc == 'W');
                    push_quoted(&mut out, &dir);
                }
                's' => {
                    let arg0 = self.expand_variable("0");
                    let base = arg0.rsplit('/').next().unwrap_or("").to_string();
                    push_quoted(&mut out, &base);
                }
                'v' | 'V' => {
                    let version = self.expand_variable("BASH_VERSION");
                    let full = version.split('(').next().unwrap_or("").to_string();
                    let text = if esc == 'V' {
                        full
                    } else {
                        full.splitn(3, '.').take(2).collect::<Vec<_>>().join(".")
                    };
                    push_quoted(&mut out, &text);
                }
                'j' => {
                    let count = self.jobs.lock().list().len();
                    out.push_str(&count.to_string());
                }
                'l' => out.push_str("tty"),
                '!' => out.push_str(&self.history_number().to_string()),
                '#' => out.push_str(&self.command_number.to_string()),
                't' => self.push_prompt_time(&mut out, "%H:%M:%S"),
                'T' => self.push_prompt_time(&mut out, "%I:%M:%S"),
                '@' => self.push_prompt_time(&mut out, "%I:%M %p"),
                'A' => self.push_prompt_time(&mut out, "%H:%M"),
                'd' => self.push_prompt_time(&mut out, "%a %b %d"),
                'D' if chars.get(i) == Some(&'{') => {
                    let rest = &chars[i + 1..];
                    let end = rest.iter().position(|&ch| ch == '}').unwrap_or(rest.len());
                    let fmt: String = rest[..end].iter().collect();
                    i += 1 + end + usize::from(end < rest.len());
                    let fmt = if fmt.is_empty() {
                        "%X".to_string()
                    } else {
                        fmt
                    };
                    self.push_prompt_time(&mut out, &fmt);
                }
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
        }
        out
    }

    /// `\w` (`$PWD` with `$HOME` shown as `~`) or `\W` (its last component).
    fn prompt_dir(&self, basename: bool) -> String {
        let pwd = self.expand_variable("PWD");
        let home = self.expand_variable("HOME");
        let home = home.trim_end_matches('/');
        let tilde = !home.is_empty()
            && (pwd == home
                || pwd
                    .strip_prefix(home)
                    .is_some_and(|rest| rest.starts_with('/')));
        if basename {
            if tilde && pwd == home {
                return "~".to_string();
            }
            if pwd == "/" {
                return pwd;
            }
            return pwd
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
        }
        if tilde {
            format!("~{}", &pwd[home.len()..])
        } else {
            pwd
        }
    }

    fn push_prompt_time(&self, out: &mut String, format: &str) {
        let tz = self.expand_variable("TZ");
        let tz = (!tz.is_empty()).then_some(tz);
        if let Ok(text) = self.date_clock.strftime(tz.as_ref(), None, format) {
            push_quoted(out, &text);
        }
    }
}

/// Text an escape substituted, protected from the second (expansion) pass.
fn push_quoted(out: &mut String, text: &str) {
    for c in text.chars() {
        if matches!(c, '$' | '`' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
}

/// `shopt -u promptvars`: no expansion pass, only the quoting comes off.
fn unquote_prompt_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && matches!(chars.peek(), Some('$' | '`' | '\\')) {
            out.push(chars.next().unwrap_or_default());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, ExecutionLimits, InMemoryFs};
    use std::sync::{Arc, atomic::Ordering};

    #[tokio::test]
    async fn prompt_checks_cancellation_before_decoding() {
        let mut interpreter = Interpreter::new(Arc::new(InMemoryFs::new()));
        interpreter.begin_execution_budget();
        interpreter.cancelled.store(true, Ordering::Relaxed);
        assert!(matches!(
            interpreter.expand_prompt_string("literal".into()).await,
            Err(Error::Cancelled)
        ));
        assert_eq!(interpreter.counters.prompt_depth, 0);
    }

    #[tokio::test]
    async fn prompt_checks_shared_deadline_before_decoding() {
        let mut interpreter = Interpreter::new(Arc::new(InMemoryFs::new()));
        interpreter.set_limits(ExecutionLimits::new().timeout(std::time::Duration::ZERO));
        interpreter.begin_execution_budget();
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        assert!(matches!(
            interpreter.expand_prompt_string("literal".into()).await,
            Err(Error::ResourceLimit(crate::LimitExceeded::Timeout(_)))
        ));
        assert_eq!(interpreter.counters.prompt_depth, 0);
    }

    #[tokio::test]
    async fn prompt_error_unwinds_depth() {
        let mut interpreter = Interpreter::new(Arc::new(InMemoryFs::new()));
        interpreter.begin_execution_budget();
        interpreter.set_env("x", "${x@P}");
        assert!(matches!(
            interpreter.expand_prompt_string("${x@P}".into()).await,
            Err(Error::ResourceLimit(crate::LimitExceeded::MaxPromptDepth(
                8
            )))
        ));
        assert_eq!(interpreter.counters.prompt_depth, 0);
        assert_eq!(
            interpreter.expand_prompt_string("ok".into()).await.unwrap(),
            "ok"
        );
    }

    #[tokio::test]
    async fn prompt_child_parser_cannot_hide_budget_exhaustion() {
        let value = "$(echo $(echo $(echo ok)))";
        let mut interpreter = Interpreter::new(Arc::new(InMemoryFs::new()));
        interpreter.set_limits(ExecutionLimits::new().max_work_units(value.len() as u64 + 2));
        interpreter.begin_execution_budget();
        let result = interpreter.expand_prompt_string(value.into()).await;
        assert!(
            matches!(
                result,
                Err(Error::ResourceLimit(crate::LimitExceeded::ExecutionBudget(
                    crate::ExecutionBudgetExceeded::WorkUnits { .. }
                )))
            ),
            "{result:?}"
        );
        assert_eq!(interpreter.counters.prompt_depth, 0);
    }

    #[test]
    fn quoting_round_trips_without_expansion() {
        let mut s = String::new();
        push_quoted(&mut s, "a$b\\c`d");
        assert_eq!(unquote_prompt_text(&s), "a$b\\c`d");
    }
}
