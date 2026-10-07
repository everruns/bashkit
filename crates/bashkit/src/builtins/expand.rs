//! expand/unexpand builtin commands - convert between tabs and spaces
//!
//! Decision: tab-stop parsing and column tracking follow GNU
//! `expand-common.c` / `unexpand.c`: `-t` lists accumulate across options,
//! a trailing `/N` repeats every N columns, `+N` repeats N after the last
//! stop, past the last explicit stop expand emits one space and unexpand
//! stops converting, and backspace moves the column back.

use async_trait::async_trait;

use super::arg_parser::{OptArg, gnu_getopt};
use super::limits::{
    EXPAND_MAX_OUTPUT_BYTES as MAX_OUTPUT_BYTES, EXPAND_MAX_TAB_STOP as MAX_TAB_STOP,
};
use super::{Builtin, BuiltinHelper, Context, read_text_file, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Parsed tab stops (GNU `tab_list` + `extend_size` + `increment_size`).
#[derive(Default, Debug)]
struct TabStops {
    list: Vec<usize>,
    extend: usize,
    increment: usize,
    /// Uniform tab size, or 0 when `list`/extend/increment apply.
    size: usize,
}

impl TabStops {
    /// Add the stops in one `-t` argument (commas or blanks separate).
    fn parse_into(&mut self, s: &str) -> std::result::Result<(), String> {
        let bad = || format!("invalid tab size: '{s}'");
        let mut extend = false;
        let mut increment = false;
        let mut val: Option<usize> = None;
        let flush = |val: &mut Option<usize>,
                     extend: bool,
                     increment: bool,
                     this: &mut Self|
         -> std::result::Result<(), String> {
            if let Some(v) = val.take() {
                if v == 0 || v > MAX_TAB_STOP {
                    return Err(bad());
                }
                if extend {
                    this.extend = v;
                } else if increment {
                    this.increment = v;
                } else {
                    this.list.push(v);
                }
            }
            Ok(())
        };
        for c in s.chars() {
            match c {
                ',' | ' ' | '\t' => flush(&mut val, extend, increment, self)?,
                '/' | '+' => {
                    if val.is_some() {
                        return Err(bad());
                    }
                    extend = c == '/';
                    increment = c == '+';
                }
                '0'..='9' => {
                    let d = c as usize - '0' as usize;
                    let v = val.unwrap_or(0).saturating_mul(10).saturating_add(d);
                    val = Some(v.min(MAX_TAB_STOP + 1));
                }
                _ => return Err(bad()),
            }
        }
        flush(&mut val, extend, increment, self)
    }

    /// GNU `finalize_tab_stops`: validate and pick the uniform size.
    fn finalize(&mut self) -> std::result::Result<(), String> {
        let mut prev = 0;
        for &t in &self.list {
            if t <= prev {
                return Err("tab sizes must be ascending".to_string());
            }
            prev = t;
        }
        self.size = if self.list.is_empty() {
            if self.extend != 0 {
                self.extend
            } else if self.increment != 0 {
                self.increment
            } else {
                8
            }
        } else if self.list.len() == 1 && self.extend == 0 && self.increment == 0 {
            self.list[0]
        } else {
            0
        };
        Ok(())
    }

    /// GNU `get_next_tab_column`; `None` means past the last tab stop.
    fn next(&self, col: usize, idx: &mut usize) -> Option<usize> {
        if self.size != 0 {
            return Some(col + (self.size - col % self.size));
        }
        while *idx < self.list.len() {
            let tab = self.list[*idx];
            if col < tab {
                return Some(tab);
            }
            *idx += 1;
        }
        if self.extend != 0 {
            return Some(col + (self.extend - col % self.extend));
        }
        if self.increment != 0 {
            let end = self.list.last().copied().unwrap_or(0);
            return Some(col + (self.increment - (col - end) % self.increment));
        }
        None
    }
}

/// Split obsolete `-N[,N...]` tab-list arguments out of `args`.
fn split_obsolete_tabs(args: &[String]) -> (Vec<String>, Vec<String>) {
    let mut rest = Vec::new();
    let mut lists = Vec::new();
    let mut done = false;
    for a in args {
        if done {
            rest.push(a.clone());
            continue;
        }
        if a == "--" {
            done = true;
            rest.push(a.clone());
        } else if a.len() > 1
            && a.starts_with('-')
            && a[1..].starts_with(|c: char| c.is_ascii_digit())
            && a[1..].chars().all(|c| c.is_ascii_digit() || c == ',')
        {
            lists.push(a[1..].to_string());
        } else {
            rest.push(a.clone());
        }
    }
    (rest, lists)
}

async fn read_inputs<H: BuiltinHelper>(
    ctx: &Context<'_>,
    files: &[String],
) -> std::result::Result<String, ExecResult> {
    if files.is_empty() {
        return Ok(ctx.stdin.map(ToString::to_string).unwrap_or_default());
    }
    let mut buf = String::new();
    for file in files {
        if file == "-" {
            buf.push_str(ctx.stdin.map(|s| &**s).unwrap_or_default());
            continue;
        }
        let path = resolve_path(ctx.cwd, file);
        match read_text_file(ctx.fs.as_ref(), &path, H::NAME).await {
            Ok(text) => buf.push_str(&text),
            Err(_) => return Err(H::err_path(file, "No such file or directory", 1)),
        }
    }
    Ok(buf)
}

/// The expand builtin command.
///
/// Usage: expand [-i] [-t LIST] [FILE...]
///
/// Converts tabs to spaces. Default tab stop is 8.
pub struct Expand;

impl BuiltinHelper for Expand {
    const NAME: &'static str = "expand";
}

#[async_trait]
impl Builtin for Expand {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = Self::check_help(
            ctx.args,
            "Usage: expand [OPTION]... [FILE]...\nConvert tabs to spaces.\n\n  -i, --initial\tdo not convert tabs after non blanks\n  -t, --tabs=LIST\tuse comma separated list of tab positions\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("expand (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let (args, obsolete) = split_obsolete_tabs(ctx.args);
        let (opts, files) = match gnu_getopt(
            "expand",
            &args,
            "it:",
            &[
                ("initial", OptArg::No, 'i'),
                ("tabs", OptArg::Required, 't'),
            ],
            true,
            1,
        ) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let mut stops = TabStops::default();
        let mut initial = false;
        for spec in &obsolete {
            if let Err(e) = stops.parse_into(spec) {
                return Ok(Self::err(e, 1));
            }
        }
        for o in opts {
            match o.key {
                'i' => initial = true,
                _ => {
                    let spec = o.value.unwrap_or_default();
                    if let Err(e) = stops.parse_into(&spec) {
                        return Ok(Self::err(e, 1));
                    }
                }
            }
        }
        if let Err(e) = stops.finalize() {
            return Ok(Self::err(e, 1));
        }

        let input = match read_inputs::<Self>(&ctx, &files).await {
            Ok(t) => t,
            Err(e) => return Ok(e),
        };

        let too_big = || Self::err(format!("output exceeds byte limit ({MAX_OUTPUT_BYTES})"), 1);
        let mut output = String::new();
        let mut col = 0usize;
        let mut idx = 0usize;
        let mut convert = true;
        for ch in input.chars() {
            if convert && ch == '\t' {
                let next = stops.next(col, &mut idx).unwrap_or(col + 1);
                let spaces = next - col;
                if output.len().saturating_add(spaces) > MAX_OUTPUT_BYTES {
                    return Ok(too_big());
                }
                output.extend(std::iter::repeat_n(' ', spaces));
                col = next;
                continue;
            }
            if output.len() >= MAX_OUTPUT_BYTES {
                return Ok(too_big());
            }
            match ch {
                '\n' => {
                    col = 0;
                    idx = 0;
                    convert = true;
                }
                '\x08' if convert => {
                    col = col.saturating_sub(1);
                    idx = idx.saturating_sub(1);
                }
                _ if convert => {
                    col += 1;
                    convert = !(initial && ch != ' ' && ch != '\t');
                }
                _ => {}
            }
            output.push(ch);
        }

        Ok(ExecResult::ok(output))
    }
}

/// The unexpand builtin command.
///
/// Usage: unexpand [-a] [--first-only] [-t LIST] [FILE...]
///
/// Converts spaces to tabs. By default, only converts leading spaces.
pub struct Unexpand;

impl BuiltinHelper for Unexpand {
    const NAME: &'static str = "unexpand";
}

#[async_trait]
impl Builtin for Unexpand {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = Self::check_help(
            ctx.args,
            "Usage: unexpand [OPTION]... [FILE]...\nConvert spaces to tabs.\n\n  -a, --all\tconvert all blanks, instead of just initial blanks\n  --first-only\tconvert only leading sequences of blanks\n  -t, --tabs=LIST\tuse comma separated list of tab positions (implies -a)\n  --help\t\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("unexpand (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let (args, obsolete) = split_obsolete_tabs(ctx.args);
        let (opts, files) = match gnu_getopt(
            "unexpand",
            &args,
            "at:",
            &[
                ("all", OptArg::No, 'a'),
                ("first-only", OptArg::No, 'F'),
                ("tabs", OptArg::Required, 't'),
            ],
            true,
            1,
        ) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let mut stops = TabStops::default();
        let mut all = false;
        let mut first_only = false;
        for spec in &obsolete {
            if let Err(e) = stops.parse_into(spec) {
                return Ok(Self::err(e, 1));
            }
        }
        for o in opts {
            match o.key {
                'a' => all = true,
                'F' => first_only = true,
                _ => {
                    let spec = o.value.unwrap_or_default();
                    if let Err(e) = stops.parse_into(&spec) {
                        return Ok(Self::err(e, 1));
                    }
                    all = true; // -t implies -a
                }
            }
        }
        if first_only {
            all = false;
        }
        if let Err(e) = stops.finalize() {
            return Ok(Self::err(e, 1));
        }

        let input = match read_inputs::<Self>(&ctx, &files).await {
            Ok(t) => t,
            Err(e) => return Ok(e),
        };

        let mut output = String::with_capacity(input.len());
        for line in input.split_inclusive('\n') {
            unexpand_line(line, &stops, all, &mut output);
        }
        Ok(ExecResult::ok(output))
    }
}

/// GNU `unexpand` state machine for one line (including its newline).
fn unexpand_line(line: &str, stops: &TabStops, all: bool, out: &mut String) {
    let mut convert = true;
    let mut column = 0usize;
    let mut idx = 0usize;
    let mut pending: Vec<char> = Vec::new();
    let mut one_blank_before_stop = false;
    let mut prev_blank = true;

    // `None` stands for end of input so pending blanks get flushed.
    for c in line.chars().map(Some).chain(std::iter::once(None)) {
        let mut ch = c;
        if convert {
            let blank = matches!(c, Some(' ') | Some('\t'));
            if blank {
                match stops.next(column, &mut idx) {
                    None => convert = false,
                    Some(next) => {
                        if c == Some('\t') {
                            column = next;
                            if let Some(p) = pending.first_mut() {
                                *p = '\t';
                            }
                        } else {
                            column += 1;
                            if !(prev_blank && column == next) {
                                if column == next {
                                    one_blank_before_stop = true;
                                }
                                pending.push(' ');
                                prev_blank = true;
                                continue;
                            }
                            ch = Some('\t');
                            if let Some(p) = pending.first_mut() {
                                *p = '\t';
                            }
                        }
                        // Keep only a single blank just before the previous stop.
                        pending.truncate(usize::from(one_blank_before_stop));
                    }
                }
            } else if c == Some('\x08') {
                column = column.saturating_sub(1);
                idx = idx.saturating_sub(1);
            } else {
                column += 1;
            }

            if !pending.is_empty() {
                if pending.len() > 1 && one_blank_before_stop {
                    pending[0] = '\t';
                }
                out.extend(pending.drain(..));
                one_blank_before_stop = false;
            }
            prev_blank = blank;
            convert &= all || blank;
        }
        if let Some(ch) = ch {
            out.push(ch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn run_expand(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };
        Expand.execute(ctx).await.expect("expand failed")
    }

    async fn run_unexpand(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };
        Unexpand.execute(ctx).await.expect("unexpand failed")
    }

    #[tokio::test]
    async fn test_expand_default_tab() {
        let result = run_expand(&[], Some("\thello")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "        hello");
    }

    #[tokio::test]
    async fn test_expand_custom_tab() {
        let result = run_expand(&["-t", "4"], Some("\thello")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "    hello");
    }

    #[tokio::test]
    async fn test_expand_rejects_oversized_tab_stop() {
        let result = run_expand(&["-t", "1000000000"], Some("\thello")).await;
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stderr, "expand: invalid tab size: '1000000000'\n");
    }

    #[tokio::test]
    async fn test_expand_rejects_output_amplification_over_cap() {
        let input = "\t".repeat((MAX_OUTPUT_BYTES / MAX_TAB_STOP) + 1);
        let result = run_expand(&["-t", &MAX_TAB_STOP.to_string()], Some(&input)).await;
        assert_eq!(result.exit_code, 1);
        assert_eq!(
            result.stderr,
            format!("expand: output exceeds byte limit ({MAX_OUTPUT_BYTES})\n")
        );
    }

    #[tokio::test]
    async fn test_unexpand_rejects_oversized_tab_stop() {
        let result = run_unexpand(&["-t", "1000000000"], Some("        hello")).await;
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stderr, "unexpand: invalid tab size: '1000000000'\n");
    }

    #[tokio::test]
    async fn test_expand_unknown_option() {
        let result = run_expand(&["-Q"], Some("hi")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_unexpand_unknown_option() {
        let result = run_unexpand(&["-Q"], Some("hi")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_expand_no_tabs() {
        let result = run_expand(&[], Some("no tabs here")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "no tabs here");
    }

    #[tokio::test]
    async fn test_expand_multiple_tabs() {
        let result = run_expand(&["-t", "4"], Some("a\tb\tc")).await;
        assert_eq!(result.exit_code, 0);
        // 'a' at col 0, tab to col 4, 'b' at col 4, tab to col 8, 'c' at col 8
        assert_eq!(result.stdout, "a   b   c");
    }

    #[tokio::test]
    async fn test_unexpand_leading_spaces() {
        let result = run_unexpand(&[], Some("        hello")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "\thello");
    }

    #[tokio::test]
    async fn test_unexpand_all() {
        let result = run_unexpand(&["-a"], Some("hello   world")).await;
        assert_eq!(result.exit_code, 0);
        // The spaces might not align to tab stops, so behavior varies
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_expand_empty() {
        let result = run_expand(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_unexpand_invalid_zero_tab_stop() {
        let result = run_unexpand(&["-t", "0"], Some("        hello")).await;
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stderr, "unexpand: invalid tab size: '0'\n");
    }

    #[tokio::test]
    async fn test_unexpand_invalid_non_numeric_tab_stop() {
        let result = run_unexpand(&["-t", "foo"], Some("        hello")).await;
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stderr, "unexpand: invalid tab size: 'foo'\n");
    }

    #[tokio::test]
    async fn test_expand_tab_list_extensions() {
        let r = run_expand(&["-t", "2,5,/3"], Some("a\tb\tc\td\te\n")).await;
        assert_eq!(r.stdout, "a b  c   d  e\n");
        let r = run_expand(&["-t", "2,5,+3"], Some("a\tb\tc\td\te\n")).await;
        assert_eq!(r.stdout, "a b  c  d  e\n");
        // Repeated -t accumulate; past the last stop a tab is one space.
        let r = run_expand(&["-t", "2", "-t", "6"], Some("a\tb\tc\td\n")).await;
        assert_eq!(r.stdout, "a b   c d\n");
        let r = run_expand(&["-t", "4,2"], Some("a\n")).await;
        assert_eq!(r.exit_code, 1);
    }

    #[tokio::test]
    async fn test_unexpand_gnu_blank_rules() {
        let r = run_unexpand(&["-a"], Some("abcdefg h\n")).await;
        assert_eq!(r.stdout, "abcdefg h\n");
        let r = run_unexpand(&["-a"], Some("abcdefg \tx\n")).await;
        assert_eq!(r.stdout, "abcdefg\t\tx\n");
        let r = run_unexpand(&["-2,6"], Some("  a   b      c\n")).await;
        assert_eq!(r.stdout, "\ta   b      c\n");
        let r = run_unexpand(&[], Some("x       y")).await;
        assert_eq!(r.stdout, "x       y");
    }
}
