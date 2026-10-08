//! read builtin - read a line of input
//!
//! With no stdin (no pipe or redirect) inside a terminal session, `read`
//! waits for a line typed on the terminal: `-p` prints the prompt there,
//! `-s` turns echo off, `-n N` returns after N characters, `-t SECS` gives
//! up with status 142. Outside a terminal no stdin is EOF (status 1).
//!
//! Options are read only before the first name (bash's getopt), so in
//! `read a -r` the `-r` is a name. Names are checked as they are assigned:
//! the ones before an invalid name get their fields, the rest are left
//! alone, and read fails with status 1. `-N n` reads exactly n characters
//! with no delimiter and no field splitting.

use async_trait::async_trait;

use super::{Builtin, BuiltinSideEffect, Context};
use crate::error::Result;
use crate::interpreter::{ExecResult, is_internal_variable, is_valid_var_name};

/// read builtin - read a line of input into variables
pub struct Read;

#[async_trait]
impl Builtin for Read {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Get the input to read from stdin
        #[cfg_attr(not(feature = "terminal"), allow(unused_mut))]
        let mut input = ctx.stdin.map(|s| s.to_string());

        // Parse flags
        let mut raw_mode = false; // -r: don't interpret backslashes
        let mut array_mode = false; // -a: read into array
        let mut delimiter = None::<char>; // -d: custom delimiter
        let mut nchars = None::<usize>; // -n: read N chars
        let mut exact_nchars = false; // -N: exactly N chars, no splitting
        let mut prompt = None::<String>; // -p prompt
        let mut silent = false; // -s: no echo (terminal input only)
        let mut timeout = None::<f64>; // -t SECS (terminal input only)
        let mut var_args = Vec::new();
        let mut args_iter = ctx.args.iter();
        let mut names_started = false;
        while let Some(arg) = args_iter.next() {
            if !names_started && arg == "--" {
                names_started = true;
            } else if !names_started && arg.starts_with('-') && arg.len() > 1 {
                let mut chars = arg[1..].chars();
                while let Some(flag) = chars.next() {
                    match flag {
                        'r' => raw_mode = true,
                        'a' => array_mode = true,
                        'd' => {
                            // -d delim: use first char of next arg as delimiter
                            let rest: String = chars.collect();
                            let delim_str = if rest.is_empty() {
                                args_iter.next().map(|s| s.as_str()).unwrap_or("")
                            } else {
                                &rest
                            };
                            delimiter = delim_str.chars().next();
                            break;
                        }
                        'n' | 'N' => {
                            exact_nchars = flag == 'N';
                            let rest: String = chars.collect();
                            let n_str = if rest.is_empty() {
                                args_iter.next().map(|s| s.as_str()).unwrap_or("0")
                            } else {
                                &rest
                            };
                            nchars = n_str.parse().ok();
                            break;
                        }
                        'p' => {
                            let rest: String = chars.collect();
                            prompt = Some(if rest.is_empty() {
                                args_iter.next().cloned().unwrap_or_default()
                            } else {
                                rest
                            });
                            break;
                        }
                        's' => silent = true,
                        't' => {
                            let rest: String = chars.collect();
                            let t_str = if rest.is_empty() {
                                args_iter.next().map(|s| s.as_str()).unwrap_or("")
                            } else {
                                &rest
                            };
                            timeout = t_str.parse().ok().filter(|t: &f64| t.is_finite());
                            break;
                        }
                        'u' => {
                            // -u fd: accept and ignore
                            let rest: String = chars.collect();
                            if rest.is_empty() {
                                args_iter.next();
                            }
                            break;
                        }
                        'e' | 'i' => {}
                        _ => {}
                    }
                }
            } else {
                names_started = true;
                var_args.push(arg.as_str());
            }
        }
        if array_mode
            && let Some(name) = var_args.first()
            && !valid_read_name(name, true)
        {
            return Ok(ExecResult::err(invalid_name(name), 1));
        }
        #[cfg(feature = "terminal")]
        if input.is_none()
            && let Some(tty) = ctx.execution_extension::<crate::terminal::Tty>()
        {
            let tty = tty
                .try_with(Clone::clone)
                .map_err(|_| crate::error::Error::Cancelled)?;
            match read_from_terminal(&tty, prompt.as_deref(), silent, nchars, timeout).await? {
                TerminalRead::Line(line) => input = Some(line),
                TerminalRead::Eof => {}
                TerminalRead::TimedOut => {
                    let mut result = ExecResult::err("", 142);
                    for var_name in if var_args.is_empty() {
                        vec!["REPLY"]
                    } else {
                        var_args.clone()
                    } {
                        if !is_internal_variable(var_name) {
                            result.side_effects.push(BuiltinSideEffect::SetVariable {
                                name: var_name.to_string(),
                                value: String::new(),
                            });
                        }
                    }
                    return Ok(result);
                }
            }
        }
        #[cfg(not(feature = "terminal"))]
        let _ = (prompt, silent, timeout);

        // Split line by IFS (default: space, tab, newline)
        // IFS whitespace chars (space, tab, newline) collapse runs and trim.
        // Non-whitespace IFS chars preserve empty fields between consecutive delimiters.
        // Check shell variables first (IFS=","), then env, then default.
        let ifs = ctx
            .variables
            .get("IFS")
            .or_else(|| ctx.env.get("IFS"))
            .map(|s| s.as_str())
            .unwrap_or(" \t\n");

        // EOF with no data: clear all target variables to empty and return 1.
        // This prevents the common `while read line || [[ -n "$line" ]]`
        // pattern from looping infinitely on the stale last value.
        let input = match input.filter(|s| !s.is_empty()) {
            Some(s) => s,
            None => {
                let var_names: Vec<&str> = if var_args.is_empty() {
                    vec!["REPLY"]
                } else {
                    var_args
                };
                let mut result = ExecResult::err("", 1);
                for var_name in &var_names {
                    if !valid_read_name(var_name, false) {
                        result.stderr = invalid_name(var_name).into();
                        break;
                    }
                    if is_internal_variable(var_name) {
                        continue;
                    }
                    result.side_effects.push(BuiltinSideEffect::SetVariable {
                        name: var_name.to_string(),
                        value: String::new(),
                    });
                }
                return Ok(result);
            }
        };

        // One record up to the delimiter (newline by default) or N chars.
        // Data that ends before the delimiter is still assigned, but the
        // status is 1, as in bash.
        // -N ignores the delimiter (consumed_len uses NUL the same way).
        let delim = if exact_nchars {
            '\0'
        } else {
            delimiter.unwrap_or('\n')
        };
        let (line, terminated) = read_record(&input, delim, nchars, raw_mode, ifs);
        let status = if terminated { 0 } else { 1 };

        struct ReadField<'a> {
            text: &'a str,
            start: usize,
        }

        let words: Vec<ReadField<'_>> = if exact_nchars {
            // -N: the characters read are one field, delimiters and IFS
            // included.
            if line.is_empty() {
                Vec::new()
            } else {
                vec![ReadField {
                    text: &line,
                    start: 0,
                }]
            }
        } else if ifs.is_empty() {
            // Empty IFS means no word splitting
            vec![ReadField {
                text: &line,
                start: 0,
            }]
        } else {
            let ifs_ws: Vec<char> = ifs.chars().filter(|c| " \t\n".contains(*c)).collect();
            let ifs_non_ws: Vec<char> = ifs.chars().filter(|c| !" \t\n".contains(*c)).collect();

            if ifs_non_ws.is_empty() {
                // All IFS chars are whitespace: collapse runs, trim
                let mut fields = Vec::new();
                let mut field_start = None::<usize>;
                for (i, ch) in line.char_indices() {
                    if ifs.contains(ch) {
                        if let Some(start) = field_start.take() {
                            fields.push(ReadField {
                                text: &line[start..i],
                                start,
                            });
                        }
                    } else if field_start.is_none() {
                        field_start = Some(i);
                    }
                }
                if let Some(start) = field_start {
                    fields.push(ReadField {
                        text: &line[start..],
                        start,
                    });
                }
                fields
            } else {
                // Mixed IFS: split on all IFS chars, collapse whitespace runs,
                // preserve empty fields for consecutive non-whitespace delimiters.
                let mut fields: Vec<ReadField<'_>> = Vec::new();
                let mut field_start = 0usize;
                let mut i = 0usize;

                while i < line.len() {
                    let mut iter = line[i..].char_indices();
                    let (_, ch) = iter.next().expect("valid char boundary");
                    let ch_len = ch.len_utf8();
                    if !ifs.contains(ch) {
                        i += ch_len;
                        continue;
                    }

                    if ifs_non_ws.contains(&ch) {
                        fields.push(ReadField {
                            text: &line[field_start..i],
                            start: field_start,
                        });
                        i += ch_len;
                        while i < line.len() {
                            let mut ws_iter = line[i..].char_indices();
                            let (_, ws_ch) = ws_iter.next().expect("valid char boundary");
                            if ifs_ws.contains(&ws_ch) {
                                i += ws_ch.len_utf8();
                            } else {
                                break;
                            }
                        }
                        field_start = i;
                    } else {
                        let pushed_field = field_start != i;
                        if pushed_field {
                            fields.push(ReadField {
                                text: &line[field_start..i],
                                start: field_start,
                            });
                        }
                        i += ch_len;
                        while i < line.len() {
                            let mut ws_iter = line[i..].char_indices();
                            let (_, ws_ch) = ws_iter.next().expect("valid char boundary");
                            if ifs_ws.contains(&ws_ch) {
                                i += ws_ch.len_utf8();
                            } else {
                                break;
                            }
                        }

                        // IFS whitespace adjacent to a non-whitespace IFS delimiter
                        // is one delimiter sequence, not an empty field.
                        if pushed_field && i < line.len() {
                            let mut next_iter = line[i..].char_indices();
                            let (_, next_ch) = next_iter.next().expect("valid char boundary");
                            if ifs_non_ws.contains(&next_ch) {
                                i += next_ch.len_utf8();
                                while i < line.len() {
                                    let mut ws_iter = line[i..].char_indices();
                                    let (_, ws_ch) = ws_iter.next().expect("valid char boundary");
                                    if ifs_ws.contains(&ws_ch) {
                                        i += ws_ch.len_utf8();
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }

                        field_start = i;
                    }
                }

                // A single trailing delimiter ends the last field; it does
                // not start an empty one (`a,b,` is two fields).
                if field_start < line.len() {
                    fields.push(ReadField {
                        text: &line[field_start..],
                        start: field_start,
                    });
                }
                fields
            }
        };

        if array_mode {
            // -a: read all words into array variable
            let arr_name = var_args.first().copied().unwrap_or("REPLY");
            // THREAT[TM-INJ-009]: Block internal variable prefix injection via read -a
            if is_internal_variable(arr_name) {
                return Ok(ExecResult::ok(String::new()));
            }
            let mut result = ExecResult::ok(String::new());
            result.exit_code = status;
            result.side_effects.push(BuiltinSideEffect::SetArray {
                name: arr_name.to_string(),
                elements: words.iter().map(|w| unprotect(w.text)).collect(),
            });
            return Ok(result);
        }

        if var_args.is_empty() {
            let mut result = ExecResult::ok(String::new());
            result.exit_code = status;
            result.side_effects.push(BuiltinSideEffect::SetVariable {
                name: "REPLY".to_string(),
                value: unprotect(&line),
            });
            return Ok(result);
        }

        let var_names = var_args;

        // Assign words to variables via side effects (respects local scoping)
        let mut result = ExecResult::ok(String::new());
        result.exit_code = status;
        for (i, var_name) in var_names.iter().enumerate() {
            if !valid_read_name(var_name, false) {
                result.stderr = invalid_name(var_name).into();
                result.exit_code = 1;
                break;
            }
            // THREAT[TM-INJ-009]: Block internal variable prefix injection via read
            if is_internal_variable(var_name) {
                continue;
            }
            let value = if i == var_names.len() - 1 && words.len() == i + 1 {
                // Only one field left: it is the value, without the
                // delimiter that ended it (`IFS=, read x y <<< a,b,` gives `b`).
                unprotect(words[i].text)
            } else if i == var_names.len() - 1 {
                // Bash gives the final variable the unsplit remaining input,
                // then strips trailing IFS whitespace. Preserve original
                // separators without keeping whitespace Bash trims.
                words
                    .get(i)
                    .map(|field| {
                        unprotect(
                            line[field.start..]
                                .trim_end_matches(|ch| ifs.contains(ch) && " \t\n".contains(ch)),
                        )
                    })
                    .unwrap_or_default()
            } else if i < words.len() {
                unprotect(words[i].text)
            } else {
                // Not enough words - set to empty
                String::new()
            };
            result.side_effects.push(BuiltinSideEffect::SetVariable {
                name: var_name.to_string(),
                value,
            });
        }

        Ok(result)
    }
}

/// A name `read` can assign: an identifier, or `name[subscript]` unless
/// reading into an array (`-a`).
fn valid_read_name(name: &str, array: bool) -> bool {
    match name.find('[') {
        Some(b) if !array => {
            is_valid_var_name(&name[..b]) && name.ends_with(']') && name.len() > b + 2
        }
        _ => is_valid_var_name(name),
    }
}

fn invalid_name(name: &str) -> String {
    format!("bash: read: `{name}': not a valid identifier\n")
}

/// Bytes of `input` one `read` with these `args` consumes from a shared
/// stdin (pipe feeding a loop): the record plus its delimiter, following
/// `-r`, `-d`, `-n` and `\<newline>` continuation like [`read_record`].
pub(crate) fn consumed_len(input: &[u8], args: &[String]) -> usize {
    let mut raw = false;
    let mut delim = '\n';
    let mut limit = None::<usize>;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let Some(flags) = arg.strip_prefix('-').filter(|f| !f.is_empty()) else {
            break;
        };
        let mut chars = flags.chars();
        while let Some(flag) = chars.next() {
            let mut value = || {
                let rest: String = chars.clone().collect();
                if rest.is_empty() {
                    iter.next().cloned().unwrap_or_default()
                } else {
                    rest
                }
            };
            match flag {
                'r' => raw = true,
                'd' => {
                    delim = value().chars().next().unwrap_or('\0');
                    break;
                }
                'n' | 'N' => {
                    limit = value().parse().ok();
                    if flag == 'N' {
                        delim = '\0';
                    }
                    break;
                }
                'p' | 't' | 'u' => {
                    value();
                    break;
                }
                _ => {}
            }
        }
    }
    // Byte scan: keeps non-UTF-8 input intact. A multi-byte delimiter
    // matches on its first byte.
    let mut delim_buf = [0u8; 4];
    let delim = delim.encode_utf8(&mut delim_buf).as_bytes()[0];
    let mut count = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        let b = input[i];
        // UTF-8 continuation bytes belong to the previous character.
        let starts_char = b & 0xC0 != 0x80;
        if starts_char && limit.is_some_and(|n| count >= n) {
            return i;
        }
        if b == delim {
            return i + 1;
        }
        if b == b'\\' && !raw {
            match input.get(i + 1) {
                Some(b'\n') => {
                    i += 2;
                    continue;
                }
                Some(_) => i += 1,
                None => return input.len(),
            }
        }
        if starts_char {
            count += 1;
        }
        i += 1;
    }
    input.len()
}

/// Escaped IFS characters are carried through field splitting as
/// private-use placeholders, so `read a b <<< 'x\ y z'` keeps `x y` whole.
const PROTECT_BASE: u32 = 0xF0000;

fn protect(ch: char) -> char {
    let c = ch as u32;
    if c < 0x10000 {
        char::from_u32(PROTECT_BASE + c).unwrap_or(ch)
    } else {
        ch
    }
}

fn unprotect(s: &str) -> String {
    s.chars()
        .map(|ch| {
            let c = ch as u32;
            if (PROTECT_BASE..PROTECT_BASE + 0x10000).contains(&c) {
                char::from_u32(c - PROTECT_BASE).unwrap_or(ch)
            } else {
                ch
            }
        })
        .collect()
}

/// Read one record from `input`: up to `delim` (not included) or `limit`
/// characters. Without `raw`, `\<newline>` joins lines and `\c` is a
/// literal `c` (protected from IFS splitting). Returns the record and
/// whether it ended at the delimiter or limit rather than at end of input.
fn read_record(
    input: &str,
    delim: char,
    limit: Option<usize>,
    raw: bool,
    ifs: &str,
) -> (String, bool) {
    let mut out = String::new();
    let mut count = 0usize;
    let mut chars = input.chars();
    loop {
        if limit.is_some_and(|n| count >= n) {
            return (out, true);
        }
        let Some(c) = chars.next() else {
            return (out, false);
        };
        if c == delim {
            return (out, true);
        }
        if c == '\\' && !raw {
            match chars.next() {
                Some('\n') => continue,
                Some(next) => {
                    out.push(if ifs.contains(next) {
                        protect(next)
                    } else {
                        next
                    });
                }
                None => return (out, false),
            }
        } else {
            out.push(c);
        }
        count += 1;
    }
}

#[cfg(feature = "terminal")]
enum TerminalRead {
    Line(String),
    Eof,
    TimedOut,
}

/// One line typed on the session terminal. The line comes back with a
/// trailing `\n` so the stdin parsing below treats it like piped input.
#[cfg(feature = "terminal")]
async fn read_from_terminal(
    tty: &crate::terminal::Tty,
    prompt: Option<&str>,
    silent: bool,
    nchars: Option<usize>,
    timeout: Option<f64>,
) -> Result<TerminalRead> {
    use crate::terminal::{InputOptions, LineRead, read_input};
    if let Some(prompt) = prompt {
        tty.write_cooked(prompt.as_bytes());
    }
    let opts = InputOptions { silent, nchars };
    let read = read_input(tty, opts);
    let outcome = match timeout {
        Some(secs) => {
            // THREAT[TM-DOS-120]: the wait is input time, excluded from the
            // execution deadline; -t only bounds how long `read` itself waits.
            let wait = std::time::Duration::from_secs_f64(secs.clamp(0.0, 86_400.0));
            match crate::time_compat::timeout(wait, read).await {
                Ok(r) => r,
                Err(_) => return Ok(TerminalRead::TimedOut),
            }
        }
        None => read.await,
    };
    Ok(match outcome {
        LineRead::Line(mut line) => {
            if silent {
                // bash -s prints no newline; move to the next row so the
                // following output does not overwrite the prompt line.
                tty.write(b"\r\n");
            }
            line.push('\n');
            TerminalRead::Line(line)
        }
        LineRead::Eof => TerminalRead::Eof,
        LineRead::Interrupt => return Err(crate::error::Error::Cancelled),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn setup() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();
        fs.mkdir(&cwd, true).await.unwrap();
        (fs, cwd, variables)
    }

    /// Extract SetVariable side effects into a map for easy assertion.
    fn extract_vars(result: &ExecResult) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for effect in &result.side_effects {
            if let BuiltinSideEffect::SetVariable { name, value } = effect {
                map.insert(name.clone(), value.clone());
            }
        }
        map
    }

    // ==================== no stdin ====================

    #[tokio::test]
    async fn read_no_stdin_returns_error() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    // ==================== basic read into REPLY ====================

    #[tokio::test]
    async fn read_into_reply() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("hello world\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("REPLY").unwrap(), "hello world");
    }

    #[tokio::test]
    async fn read_into_reply_preserves_trailing_ifs_whitespace() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("secret  \n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("REPLY").unwrap(), "secret  ");
    }

    // ==================== read into named variable ====================

    #[tokio::test]
    async fn read_into_named_var() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["MY_VAR".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("test_value\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("MY_VAR").unwrap(), "test_value");
    }

    // ==================== read into multiple variables ====================

    #[tokio::test]
    async fn read_multiple_vars() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("one two three four\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "one");
        assert_eq!(vars.get("B").unwrap(), "two");
        // Last var gets remaining words
        assert_eq!(vars.get("C").unwrap(), "three four");
    }

    #[tokio::test]
    async fn read_more_vars_than_words() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("one\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "one");
        assert_eq!(vars.get("B").unwrap(), "");
        assert_eq!(vars.get("C").unwrap(), "");
    }

    // ==================== -r flag (raw mode) ====================

    #[tokio::test]
    async fn read_raw_mode_preserves_backslash() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-r".to_string(), "LINE".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("hello\\world\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("LINE").unwrap(), "hello\\world");
    }

    #[tokio::test]
    async fn read_without_raw_handles_line_continuation() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["LINE".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("hello\\\nworld\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        // Without -r, backslash-newline is line continuation
        assert_eq!(vars.get("LINE").unwrap(), "helloworld");
    }

    // ==================== -n flag (read N chars) ====================

    #[tokio::test]
    async fn read_n_chars() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-n".to_string(), "3".to_string(), "CHUNK".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("abcdefgh"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("CHUNK").unwrap(), "abc");
    }

    #[tokio::test]
    async fn read_n_more_than_input() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-n".to_string(), "100".to_string(), "CHUNK".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("hi\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("CHUNK").unwrap(), "hi");
    }

    // ==================== -d flag (delimiter) ====================

    #[tokio::test]
    async fn read_custom_delimiter() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-d".to_string(), ",".to_string(), "FIELD".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("first,second,third"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("FIELD").unwrap(), "first");
    }

    // ==================== -a flag (array mode) ====================

    #[tokio::test]
    async fn read_array_mode() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-a".to_string(), "ARR".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("one two three\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.side_effects.len(), 1);
        match &result.side_effects[0] {
            BuiltinSideEffect::SetArray { name, elements } => {
                assert_eq!(name, "ARR");
                assert_eq!(elements, &["one", "two", "three"]);
            }
            _ => panic!("Expected SetArray side effect"),
        }
    }

    #[tokio::test]
    async fn read_array_mode_default_name() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-a".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("a b\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.side_effects.len(), 1);
        match &result.side_effects[0] {
            BuiltinSideEffect::SetArray { name, .. } => assert_eq!(name, "REPLY"),
            _ => panic!("Expected SetArray side effect"),
        }
    }

    // ==================== combined flags ====================

    #[tokio::test]
    async fn read_combined_r_flag() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        // -r combined in single arg
        let args = vec!["-r".to_string(), "V".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("path\\to\\file\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("V").unwrap(), "path\\to\\file");
    }

    // ==================== multiline input ====================

    #[tokio::test]
    async fn read_only_first_line() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-r".to_string(), "LINE".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("first\nsecond\nthird"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("LINE").unwrap(), "first");
    }

    // ==================== custom IFS ====================

    #[tokio::test]
    async fn read_custom_ifs() {
        let (fs, mut cwd, mut variables) = setup().await;
        let mut env = HashMap::new();
        env.insert("IFS".to_string(), ":".to_string());
        let args = vec!["A".to_string(), "B".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("foo:bar:baz\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "foo");
        assert_eq!(vars.get("B").unwrap(), "bar:baz");
    }

    #[tokio::test]
    async fn read_custom_ifs_last_var_preserves_original_delimiters() {
        let (fs, mut cwd, mut variables) = setup().await;
        let mut env = HashMap::new();
        env.insert("IFS".to_string(), ",:".to_string());
        let args = vec!["A".to_string(), "B".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("1,2:3\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "1");
        assert_eq!(vars.get("B").unwrap(), "2:3");
    }

    #[tokio::test]
    async fn read_last_var_trims_trailing_ifs_whitespace() {
        let (fs, mut cwd, mut variables) = setup().await;
        let mut env = HashMap::new();
        env.insert("IFS".to_string(), " ".to_string());
        let args = vec!["A".to_string(), "B".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("a   b  c  \n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "a");
        assert_eq!(vars.get("B").unwrap(), "b  c");
    }

    #[tokio::test]
    async fn read_last_var_trims_only_trailing_ifs_whitespace() {
        let (fs, mut cwd, mut variables) = setup().await;
        let mut env = HashMap::new();
        env.insert("IFS".to_string(), ",:".to_string());
        let args = vec!["A".to_string(), "B".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("1,2:3  \n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "1");
        assert_eq!(vars.get("B").unwrap(), "2:3  ");
    }

    #[tokio::test]
    async fn read_custom_ifs_preserves_empty_fields() {
        let (fs, mut cwd, mut variables) = setup().await;
        let mut env = HashMap::new();
        env.insert("IFS".to_string(), ":".to_string());
        let args = vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
            "D".to_string(),
        ];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("one::three:\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "one");
        assert_eq!(vars.get("B").unwrap(), "");
        assert_eq!(vars.get("C").unwrap(), "three");
        assert_eq!(vars.get("D").unwrap(), "");
    }

    #[tokio::test]
    async fn read_mixed_ifs_whitespace_and_non_ws() {
        let (fs, mut cwd, mut variables) = setup().await;
        let mut env = HashMap::new();
        env.insert("IFS".to_string(), ": ".to_string());
        let args = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("one two:three\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "one");
        assert_eq!(vars.get("B").unwrap(), "two");
        assert_eq!(vars.get("C").unwrap(), "three");
    }

    #[tokio::test]
    async fn read_mixed_ifs_whitespace_before_non_ws_delimiter() {
        let (fs, mut cwd, mut variables) = setup().await;
        let mut env = HashMap::new();
        env.insert("IFS".to_string(), ": ".to_string());
        let args = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("one : two\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "one");
        assert_eq!(vars.get("B").unwrap(), "two");
        assert_eq!(vars.get("C").unwrap(), "");
    }

    #[tokio::test]
    async fn read_empty_ifs_no_splitting() {
        let (fs, mut cwd, mut variables) = setup().await;
        let mut env = HashMap::new();
        env.insert("IFS".to_string(), String::new());
        let args = vec!["LINE".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("no splitting here\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("LINE").unwrap(), "no splitting here");
    }

    #[tokio::test]
    async fn read_ifs_from_shell_variables() {
        // IFS set as a shell variable (not env) — the common case (IFS=",")
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        variables.insert("IFS".to_string(), ",".to_string());
        let args = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("one,two,three\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let vars = extract_vars(&result);
        assert_eq!(vars.get("A").unwrap(), "one");
        assert_eq!(vars.get("B").unwrap(), "two");
        assert_eq!(vars.get("C").unwrap(), "three");
    }

    #[tokio::test]
    async fn read_ifs_from_shell_variables_array() {
        // IFS=: with read -ra should split into array
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        variables.insert("IFS".to_string(), ":".to_string());
        let args = vec!["-ra".to_string(), "parts".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            Some("a:b:c\n"),
        );
        let result = Read.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        match &result.side_effects[0] {
            BuiltinSideEffect::SetArray { name, elements } => {
                assert_eq!(name, "parts");
                assert_eq!(elements, &["a", "b", "c"]);
            }
            _ => panic!("Expected SetArray side effect"),
        }
    }
}
