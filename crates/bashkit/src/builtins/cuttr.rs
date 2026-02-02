//! Cut and tr builtins - extract fields and translate characters

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The cut builtin - remove sections from each line.
///
/// Usage: cut -d DELIM -f FIELDS [FILE...]
///
/// Options:
///   -d DELIM   Use DELIM instead of TAB for field delimiter
///   -f FIELDS  Select only these fields (1-indexed, comma-separated or ranges)
pub struct Cut;

#[async_trait]
impl Builtin for Cut {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut delimiter = '\t';
        let mut fields_spec = String::new();
        let mut files = Vec::new();

        // Parse arguments
        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            if arg == "-d" {
                i += 1;
                if i < ctx.args.len() {
                    delimiter = ctx.args[i].chars().next().unwrap_or('\t');
                }
            } else if let Some(d) = arg.strip_prefix("-d") {
                delimiter = d.chars().next().unwrap_or('\t');
            } else if arg == "-f" {
                i += 1;
                if i < ctx.args.len() {
                    fields_spec = ctx.args[i].clone();
                }
            } else if let Some(f) = arg.strip_prefix("-f") {
                fields_spec = f.to_string();
            } else if !arg.starts_with('-') {
                files.push(arg.clone());
            }
            i += 1;
        }

        if fields_spec.is_empty() {
            return Ok(ExecResult::err(
                "cut: you must specify a list of fields\n".to_string(),
                1,
            ));
        }

        // Parse field specification
        let fields = parse_field_spec(&fields_spec);

        let mut output = String::new();

        if files.is_empty() || files.iter().all(|f| f.as_str() == "-") {
            // Read from stdin
            if let Some(stdin) = ctx.stdin {
                for line in stdin.lines() {
                    output.push_str(&cut_line(line, delimiter, &fields));
                    output.push('\n');
                }
            }
        } else {
            // Read from files
            for file in &files {
                if file.as_str() == "-" {
                    if let Some(stdin) = ctx.stdin {
                        for line in stdin.lines() {
                            output.push_str(&cut_line(line, delimiter, &fields));
                            output.push('\n');
                        }
                    }
                    continue;
                }

                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    ctx.cwd.join(file)
                };

                match ctx.fs.read_file(&path).await {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content);
                        for line in text.lines() {
                            output.push_str(&cut_line(line, delimiter, &fields));
                            output.push('\n');
                        }
                    }
                    Err(e) => {
                        return Ok(ExecResult::err(format!("cut: {}: {}\n", file, e), 1));
                    }
                }
            }
        }

        Ok(ExecResult::ok(output))
    }
}

/// Parse a field specification like "1", "1,3", "1-3", "1,3-5"
fn parse_field_spec(spec: &str) -> Vec<usize> {
    let mut fields = Vec::new();

    for part in spec.split(',') {
        if let Some((start, end)) = part.split_once('-') {
            let start: usize = start.parse().unwrap_or(1);
            let end: usize = end.parse().unwrap_or(start);
            for f in start..=end {
                if f > 0 {
                    fields.push(f);
                }
            }
        } else if let Ok(f) = part.parse::<usize>() {
            if f > 0 {
                fields.push(f);
            }
        }
    }

    fields.sort();
    fields.dedup();
    fields
}

/// Cut fields from a line
fn cut_line(line: &str, delimiter: char, fields: &[usize]) -> String {
    let parts: Vec<&str> = line.split(delimiter).collect();
    let selected: Vec<&str> = fields
        .iter()
        .filter_map(|&f| parts.get(f - 1).copied())
        .collect();
    selected.join(&delimiter.to_string())
}

/// The tr builtin - translate or delete characters.
///
/// Usage: tr [-d] SET1 [SET2]
///
/// Options:
///   -d   Delete characters in SET1
///
/// SET1 and SET2 can contain character ranges like a-z, A-Z, 0-9
pub struct Tr;

#[async_trait]
impl Builtin for Tr {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let delete = ctx.args.iter().any(|a| a == "-d");
        // Only treat as flag if it's a known flag like "-d", not a lone "-" which is a valid char set
        let non_flag_args: Vec<_> = ctx
            .args
            .iter()
            .filter(|a| *a != "-d" && (a.len() == 1 || !a.starts_with('-')))
            .collect();

        if non_flag_args.is_empty() {
            return Ok(ExecResult::err("tr: missing operand\n".to_string(), 1));
        }

        let set1 = expand_char_set(non_flag_args[0]);

        let result = if delete {
            // Delete mode
            let stdin = ctx.stdin.unwrap_or("");
            stdin
                .chars()
                .filter(|c| !set1.contains(c))
                .collect::<String>()
        } else {
            // Translate mode
            if non_flag_args.len() < 2 {
                return Ok(ExecResult::err(
                    "tr: missing operand after SET1\n".to_string(),
                    1,
                ));
            }

            let set2 = expand_char_set(non_flag_args[1]);
            let stdin = ctx.stdin.unwrap_or("");

            stdin
                .chars()
                .map(|c| {
                    if let Some(pos) = set1.iter().position(|&x| x == c) {
                        // Get corresponding char from set2, or last char if set2 is shorter
                        *set2.get(pos).or(set2.last()).unwrap_or(&c)
                    } else {
                        c
                    }
                })
                .collect::<String>()
        };

        Ok(ExecResult::ok(result))
    }
}

/// Expand a character set specification like "a-z" into a list of characters
fn expand_char_set(spec: &str) -> Vec<char> {
    let mut chars = Vec::new();
    let mut iter = spec.chars().peekable();

    while let Some(c) = iter.next() {
        if iter.peek() == Some(&'-') {
            iter.next(); // consume '-'
            if let Some(&end) = iter.peek() {
                iter.next(); // consume end char
                             // Expand range
                let start = c as u32;
                let end = end as u32;
                for code in start..=end {
                    if let Some(ch) = char::from_u32(code) {
                        chars.push(ch);
                    }
                }
            } else {
                // Trailing dash, treat literally
                chars.push(c);
                chars.push('-');
            }
        } else {
            chars.push(c);
        }
    }

    chars
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_cut(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin,
        };

        Cut.execute(ctx).await.unwrap()
    }

    async fn run_tr(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin,
        };

        Tr.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_cut_single_field() {
        let result = run_cut(&["-d", ",", "-f", "2"], Some("a,b,c\n1,2,3\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "b\n2\n");
    }

    #[tokio::test]
    async fn test_cut_multiple_fields() {
        let result = run_cut(&["-d", ",", "-f", "1,3"], Some("a,b,c\n1,2,3\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a,c\n1,3\n");
    }

    #[tokio::test]
    async fn test_cut_field_range() {
        let result = run_cut(&["-d", ",", "-f", "1-2"], Some("a,b,c,d\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a,b\n");
    }

    #[tokio::test]
    async fn test_tr_lowercase_to_uppercase() {
        let result = run_tr(&["a-z", "A-Z"], Some("hello world")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "HELLO WORLD");
    }

    #[tokio::test]
    async fn test_tr_delete() {
        let result = run_tr(&["-d", "aeiou"], Some("hello world")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hll wrld");
    }

    #[tokio::test]
    async fn test_tr_single_char() {
        let result = run_tr(&[":", "-"], Some("a:b:c")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a-b-c");
    }

    #[test]
    fn test_expand_char_set() {
        assert_eq!(expand_char_set("abc"), vec!['a', 'b', 'c']);
        assert_eq!(expand_char_set("a-c"), vec!['a', 'b', 'c']);
        assert_eq!(expand_char_set("0-2"), vec!['0', '1', '2']);
    }

    #[test]
    fn test_parse_field_spec() {
        assert_eq!(parse_field_spec("1"), vec![1]);
        assert_eq!(parse_field_spec("1,3"), vec![1, 3]);
        assert_eq!(parse_field_spec("1-3"), vec![1, 2, 3]);
        assert_eq!(parse_field_spec("1,3-5"), vec![1, 3, 4, 5]);
    }
}
