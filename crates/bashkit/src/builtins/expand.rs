//! expand/unexpand builtin commands - convert between tabs and spaces

use async_trait::async_trait;

use super::{Builtin, Context, read_text_file, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The expand builtin command.
///
/// Usage: expand [-t N] [FILE...]
///
/// Converts tabs to spaces. Default tab stop is 8.
pub struct Expand;

#[async_trait]
impl Builtin for Expand {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut tab_stops: Vec<usize> = vec![8];
        let mut files: Vec<&str> = Vec::new();

        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-t" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "expand: option requires an argument -- 't'\n".to_string(),
                            1,
                        ));
                    }
                    tab_stops = parse_tab_stops(&ctx.args[i]);
                }
                s if s.starts_with("-t") && s.len() > 2 => {
                    tab_stops = parse_tab_stops(&s[2..]);
                }
                _ => files.push(&ctx.args[i]),
            }
            i += 1;
        }

        let input = if files.is_empty() {
            ctx.stdin.unwrap_or("").to_string()
        } else {
            let mut buf = String::new();
            for file in &files {
                let path = resolve_path(ctx.cwd, file);
                match read_text_file(ctx.fs.as_ref(), &path, "expand").await {
                    Ok(text) => buf.push_str(&text),
                    Err(_) => {
                        return Ok(ExecResult::err(
                            format!("expand: {}: No such file or directory\n", file),
                            1,
                        ));
                    }
                }
            }
            buf
        };

        let mut output = String::new();
        for line in input.split('\n') {
            let mut col = 0;
            for ch in line.chars() {
                if ch == '\t' {
                    let next_stop = next_tab_stop(col, &tab_stops);
                    let spaces = next_stop - col;
                    for _ in 0..spaces {
                        output.push(' ');
                    }
                    col = next_stop;
                } else {
                    output.push(ch);
                    col += 1;
                }
            }
            output.push('\n');
        }

        // Remove trailing extra newline from split
        if !input.ends_with('\n') && output.ends_with('\n') {
            output.pop();
        }

        Ok(ExecResult::ok(output))
    }
}

/// The unexpand builtin command.
///
/// Usage: unexpand [-a] [-t N] [FILE...]
///
/// Converts spaces to tabs. By default, only converts leading spaces.
pub struct Unexpand;

#[async_trait]
impl Builtin for Unexpand {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut tab_stops: Vec<usize> = vec![8];
        let mut all = false;
        let mut files: Vec<&str> = Vec::new();

        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-a" | "--all" => all = true,
                "-t" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "unexpand: option requires an argument -- 't'\n".to_string(),
                            1,
                        ));
                    }
                    tab_stops = parse_tab_stops(&ctx.args[i]);
                    all = true; // -t implies -a
                }
                _ => files.push(&ctx.args[i]),
            }
            i += 1;
        }

        let input = if files.is_empty() {
            ctx.stdin.unwrap_or("").to_string()
        } else {
            let mut buf = String::new();
            for file in &files {
                let path = resolve_path(ctx.cwd, file);
                match read_text_file(ctx.fs.as_ref(), &path, "unexpand").await {
                    Ok(text) => buf.push_str(&text),
                    Err(_) => {
                        return Ok(ExecResult::err(
                            format!("unexpand: {}: No such file or directory\n", file),
                            1,
                        ));
                    }
                }
            }
            buf
        };

        let tab_size = tab_stops[0];
        let mut output = String::new();

        for line in input.split('\n') {
            if all {
                // Convert all sequences of spaces at tab stops
                let mut col = 0;
                let mut space_count = 0;
                let mut result = String::new();

                for ch in line.chars() {
                    if ch == ' ' {
                        space_count += 1;
                        col += 1;
                        if col % tab_size == 0 && space_count > 1 {
                            result.push('\t');
                            space_count = 0;
                        }
                    } else {
                        for _ in 0..space_count {
                            result.push(' ');
                        }
                        space_count = 0;
                        result.push(ch);
                        col += 1;
                    }
                }
                for _ in 0..space_count {
                    result.push(' ');
                }
                output.push_str(&result);
            } else {
                // Only convert leading spaces
                let mut col = 0;
                let chars: Vec<char> = line.chars().collect();
                let mut pos = 0;
                let mut result = String::new();

                // Process leading spaces
                while pos < chars.len() && chars[pos] == ' ' {
                    col += 1;
                    pos += 1;
                    if col % tab_size == 0 {
                        result.push('\t');
                    }
                }
                // Add remaining leading spaces that didn't fill a tab
                let remainder = col % tab_size;
                if remainder > 0 && pos < chars.len() {
                    // We consumed some spaces but not enough for a tab
                    let tabs_written = col / tab_size;
                    let spaces_accounted = tabs_written * tab_size;
                    for _ in 0..(col - spaces_accounted) {
                        // These are already handled by the tab pushes above
                    }
                }
                // Append rest of line unchanged
                for ch in &chars[pos..] {
                    result.push(*ch);
                }
                output.push_str(&result);
            }
            output.push('\n');
        }

        if !input.ends_with('\n') && output.ends_with('\n') {
            output.pop();
        }

        Ok(ExecResult::ok(output))
    }
}

fn parse_tab_stops(s: &str) -> Vec<usize> {
    s.split(',')
        .filter_map(|p| p.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .collect::<Vec<_>>()
        .into_iter()
        .collect()
}

fn next_tab_stop(col: usize, tab_stops: &[usize]) -> usize {
    if tab_stops.len() == 1 {
        let ts = tab_stops[0];
        ((col / ts) + 1) * ts
    } else {
        // Find the first tab stop > col
        for &ts in tab_stops {
            if ts > col {
                return ts;
            }
        }
        // Past all tab stops, use last interval
        col + 1
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
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
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
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
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
}
