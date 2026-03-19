//! split builtin command - split a file into pieces

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The split builtin command.
///
/// Usage: split [-l lines] [-b bytes] [-n chunks] [-d] [FILE [PREFIX]]
///
/// Options:
///   -l N     Split into pieces of N lines each (default 1000)
///   -b N     Split by byte size
///   -n N     Split into N equal pieces
///   -d       Use numeric suffixes (00, 01, ...) instead of alphabetic (aa, ab, ...)
pub struct Split;

#[async_trait]
impl Builtin for Split {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut lines_per_file: Option<usize> = None;
        let mut bytes_per_file: Option<usize> = None;
        let mut num_chunks: Option<usize> = None;
        let mut numeric_suffix = false;
        let mut positional: Vec<&str> = Vec::new();

        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-l" => {
                    i += 1;
                    lines_per_file =
                        Some(ctx.args.get(i).and_then(|s| s.parse().ok()).unwrap_or(1000));
                }
                "-b" => {
                    i += 1;
                    bytes_per_file = ctx.args.get(i).and_then(|s| parse_size(s));
                }
                "-n" => {
                    i += 1;
                    num_chunks = ctx.args.get(i).and_then(|s| s.parse().ok());
                }
                "-d" | "--numeric-suffixes" => numeric_suffix = true,
                _ => positional.push(&ctx.args[i]),
            }
            i += 1;
        }

        // Default to splitting by lines
        if lines_per_file.is_none() && bytes_per_file.is_none() && num_chunks.is_none() {
            lines_per_file = Some(1000);
        }

        let file = positional.first().copied().unwrap_or("-");
        let prefix = positional.get(1).copied().unwrap_or("x");

        let input = if file == "-" {
            ctx.stdin.unwrap_or("").to_string()
        } else {
            let path = resolve_path(ctx.cwd, file);
            match ctx.fs.read_file(&path).await {
                Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!(
                            "split: cannot open '{}' for reading: No such file or directory\n",
                            file
                        ),
                        1,
                    ));
                }
            }
        };

        let mut file_index = 0;

        if let Some(n) = num_chunks {
            // Split into N equal pieces
            if n == 0 {
                return Ok(ExecResult::err(
                    "split: invalid number of chunks: 0\n".to_string(),
                    1,
                ));
            }
            let chunk_size = input.len().div_ceil(n);
            let bytes = input.as_bytes();
            let mut pos = 0;
            while pos < bytes.len() {
                let end = (pos + chunk_size).min(bytes.len());
                let suffix = make_suffix(file_index, numeric_suffix);
                let out_path = resolve_path(ctx.cwd, &format!("{}{}", prefix, suffix));
                ctx.fs.write_file(&out_path, &bytes[pos..end]).await?;
                file_index += 1;
                pos = end;
            }
        } else if let Some(size) = bytes_per_file {
            // Split by byte size
            let bytes = input.as_bytes();
            let mut pos = 0;
            while pos < bytes.len() {
                let end = (pos + size).min(bytes.len());
                let suffix = make_suffix(file_index, numeric_suffix);
                let out_path = resolve_path(ctx.cwd, &format!("{}{}", prefix, suffix));
                ctx.fs.write_file(&out_path, &bytes[pos..end]).await?;
                file_index += 1;
                pos = end;
            }
        } else {
            // Split by lines
            let n = lines_per_file.unwrap_or(1000);
            let lines: Vec<&str> = input.lines().collect();
            let mut pos = 0;
            while pos < lines.len() {
                let end = (pos + n).min(lines.len());
                let suffix = make_suffix(file_index, numeric_suffix);
                let out_path = resolve_path(ctx.cwd, &format!("{}{}", prefix, suffix));
                let chunk = lines[pos..end].join("\n");
                let chunk_with_newline = if end < lines.len() || input.ends_with('\n') {
                    format!("{}\n", chunk)
                } else {
                    chunk
                };
                ctx.fs
                    .write_file(&out_path, chunk_with_newline.as_bytes())
                    .await?;
                file_index += 1;
                pos = end;
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

fn make_suffix(index: usize, numeric: bool) -> String {
    if numeric {
        format!("{:02}", index)
    } else {
        // aa, ab, ac, ..., az, ba, bb, ...
        let first = (b'a' + (index / 26) as u8) as char;
        let second = (b'a' + (index % 26) as u8) as char;
        format!("{}{}", first, second)
    }
}

fn parse_size(s: &str) -> Option<usize> {
    let s = s.trim();
    if let Some(stripped) = s.strip_suffix('k').or_else(|| s.strip_suffix('K')) {
        stripped.parse::<usize>().ok().map(|n| n * 1024)
    } else if let Some(stripped) = s.strip_suffix('m').or_else(|| s.strip_suffix('M')) {
        stripped.parse::<usize>().ok().map(|n| n * 1024 * 1024)
    } else {
        s.parse::<usize>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFs};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    async fn run_split(args: &[&str], stdin: Option<&str>, fs: Arc<dyn FileSystem>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
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
        Split.execute(ctx).await.expect("split failed")
    }

    #[tokio::test]
    async fn test_split_by_lines() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let input = "line1\nline2\nline3\nline4\nline5\n";
        fs.write_file(Path::new("/input"), input.as_bytes())
            .await
            .unwrap();
        let result = run_split(&["-l", "2", "/input"], None, fs.clone()).await;
        assert_eq!(result.exit_code, 0);
        // Should create xaa (2 lines), xab (2 lines), xac (1 line)
        assert!(fs.exists(Path::new("/xaa")).await.unwrap());
        assert!(fs.exists(Path::new("/xab")).await.unwrap());
        assert!(fs.exists(Path::new("/xac")).await.unwrap());
    }

    #[tokio::test]
    async fn test_split_numeric_suffix() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let input = "a\nb\nc\n";
        let result = run_split(&["-l", "1", "-d"], Some(input), fs.clone()).await;
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(Path::new("/x00")).await.unwrap());
        assert!(fs.exists(Path::new("/x01")).await.unwrap());
        assert!(fs.exists(Path::new("/x02")).await.unwrap());
    }

    #[tokio::test]
    async fn test_split_by_chunks() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let input = "abcdef";
        let result = run_split(&["-n", "3"], Some(input), fs.clone()).await;
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(Path::new("/xaa")).await.unwrap());
        assert!(fs.exists(Path::new("/xab")).await.unwrap());
        assert!(fs.exists(Path::new("/xac")).await.unwrap());
    }

    #[tokio::test]
    async fn test_split_custom_prefix() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let input = "data\n";
        let result = run_split(&["-l", "1", "-", "out_"], Some(input), fs.clone()).await;
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(Path::new("/out_aa")).await.unwrap());
    }

    #[tokio::test]
    async fn test_split_missing_file() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_split(&["/nonexistent"], None, fs).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("cannot open"));
    }

    #[tokio::test]
    async fn test_split_zero_chunks() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_split(&["-n", "0"], Some("data"), fs).await;
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_make_suffix_alpha() {
        assert_eq!(make_suffix(0, false), "aa");
        assert_eq!(make_suffix(1, false), "ab");
        assert_eq!(make_suffix(26, false), "ba");
    }

    #[tokio::test]
    async fn test_make_suffix_numeric() {
        assert_eq!(make_suffix(0, true), "00");
        assert_eq!(make_suffix(5, true), "05");
        assert_eq!(make_suffix(42, true), "42");
    }
}
