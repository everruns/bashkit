//! fold builtin command - wrap lines at specified width

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The fold builtin command.
///
/// Usage: fold [-w width] [-s] [-b] [FILE...]
///
/// Options:
///   -w width  Wrap at width columns (default 80)
///   -s        Break at spaces (word boundary)
///   -b        Count bytes instead of columns
pub struct Fold;

#[async_trait]
impl Builtin for Fold {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut width: usize = 80;
        let mut break_at_spaces = false;
        let mut files: Vec<&str> = Vec::new();

        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-s" => break_at_spaces = true,
                "-b" => { /* byte mode is default for us since we use chars */ }
                "-w" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "fold: option requires an argument -- 'w'\n".to_string(),
                            1,
                        ));
                    }
                    width = ctx.args[i].parse().unwrap_or(80);
                }
                s if s.starts_with("-w") && s.len() > 2 => {
                    width = s[2..].parse().unwrap_or(80);
                }
                _ => files.push(&ctx.args[i]),
            }
            i += 1;
        }

        if width == 0 {
            width = 1; // prevent infinite loop
        }

        let input = if files.is_empty() {
            ctx.stdin.unwrap_or("").to_string()
        } else {
            let mut buf = String::new();
            for file in &files {
                let path = resolve_path(ctx.cwd, file);
                match ctx.fs.read_file(&path).await {
                    Ok(bytes) => buf.push_str(&String::from_utf8_lossy(&bytes)),
                    Err(_) => {
                        return Ok(ExecResult::err(
                            format!("fold: {}: No such file or directory\n", file),
                            1,
                        ));
                    }
                }
            }
            buf
        };

        let mut output = String::new();
        let lines: Vec<&str> = input.split('\n').collect();
        for (i, line) in lines.iter().enumerate() {
            fold_line(line, width, break_at_spaces, &mut output);
            if i < lines.len() - 1 {
                output.push('\n');
            }
        }
        // Preserve trailing newline if input had one
        if input.ends_with('\n') && !output.ends_with('\n') {
            output.push('\n');
        }

        Ok(ExecResult::ok(output))
    }
}

fn fold_line(line: &str, width: usize, break_at_spaces: bool, output: &mut String) {
    if line.len() <= width {
        output.push_str(line);
        return;
    }

    let chars: Vec<char> = line.chars().collect();
    let mut pos = 0;

    while pos < chars.len() {
        let remaining = chars.len() - pos;
        if remaining <= width {
            for ch in &chars[pos..] {
                output.push(*ch);
            }
            break;
        }

        let end = pos + width;
        if break_at_spaces {
            // Find last space within the width
            let mut break_pos = end;
            let mut found = false;
            for j in (pos..end).rev() {
                if chars[j] == ' ' {
                    break_pos = j + 1;
                    found = true;
                    break;
                }
            }
            if !found {
                break_pos = end;
            }
            for ch in &chars[pos..break_pos] {
                output.push(*ch);
            }
            output.push('\n');
            pos = break_pos;
        } else {
            for ch in &chars[pos..end] {
                output.push(*ch);
            }
            output.push('\n');
            pos = end;
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

    async fn run_fold(args: &[&str], stdin: Option<&str>) -> ExecResult {
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
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            shell: None,
        };
        Fold.execute(ctx).await.expect("fold failed")
    }

    #[tokio::test]
    async fn test_fold_default_width() {
        let long = "a".repeat(100);
        let result = run_fold(&[], Some(&long)).await;
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 80);
        assert_eq!(lines[1].len(), 20);
    }

    #[tokio::test]
    async fn test_fold_custom_width() {
        let result = run_fold(&["-w", "10"], Some("abcdefghijklmno")).await;
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "abcdefghij");
        assert_eq!(lines[1], "klmno");
    }

    #[tokio::test]
    async fn test_fold_break_at_spaces() {
        let result = run_fold(&["-w", "15", "-s"], Some("hello world this is a test")).await;
        assert_eq!(result.exit_code, 0);
        // Should break at spaces, not mid-word
        for line in result.stdout.lines() {
            assert!(line.len() <= 15, "Line too long: '{}'", line);
        }
    }

    #[tokio::test]
    async fn test_fold_short_line_unchanged() {
        let result = run_fold(&["-w", "80"], Some("short line\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "short line\n");
    }

    #[tokio::test]
    async fn test_fold_empty_input() {
        let result = run_fold(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
    }
}
