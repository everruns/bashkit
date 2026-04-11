//! join builtin command - join lines of two sorted files on a common field

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The join builtin command.
///
/// Usage: join [-1 FIELD] [-2 FIELD] [-t CHAR] [-a FILENUM] [-e STRING] FILE1 FILE2
///
/// Join lines of two sorted files on a common field (default: first field).
pub struct Join;

struct JoinOptions {
    field1: usize,        // 1-based field number for file1
    field2: usize,        // 1-based field number for file2
    separator: char,      // field separator
    unpaired: Vec<usize>, // which file's unpairable lines to show (1 or 2)
    empty: String,        // replacement for missing fields
}

#[async_trait]
impl Builtin for Join {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: join [OPTION]... FILE1 FILE2\nJoin lines of two sorted files on a common field.\n\n  -1 FIELD\tjoin on this FIELD of file 1\n  -2 FIELD\tjoin on this FIELD of file 2\n  -a FILENUM\talso print unpairable lines from file FILENUM\n  -e STRING\treplace missing input fields with STRING\n  -t CHAR\tuse CHAR as input and output field separator\n  --help\t\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("join (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let mut opts = JoinOptions {
            field1: 1,
            field2: 1,
            separator: ' ',
            unpaired: Vec::new(),
            empty: String::new(),
        };

        let mut files: Vec<&str> = Vec::new();
        let mut p = super::arg_parser::ArgParser::new(ctx.args);

        while !p.is_done() {
            if let Some(val) = p.flag_value_opt("-1") {
                opts.field1 = val.parse().unwrap_or(1);
            } else if let Some(val) = p.flag_value_opt("-2") {
                opts.field2 = val.parse().unwrap_or(1);
            } else if let Some(val) = p.flag_value_opt("-t") {
                opts.separator = val.chars().next().unwrap_or(' ');
            } else if let Some(val) = p.flag_value_opt("-a") {
                if let Ok(n) = val.parse::<usize>() {
                    opts.unpaired.push(n);
                }
            } else if let Some(val) = p.flag_value_opt("-e") {
                opts.empty = val.to_string();
            } else if let Some(arg) = p.positional() {
                files.push(arg);
            }
        }

        if files.len() < 2 {
            return Ok(ExecResult::err("join: missing operand\n".to_string(), 1));
        }

        let content1 = read_input(ctx.fs.as_ref(), ctx.cwd, files[0], ctx.stdin).await?;
        let content2 = read_input(ctx.fs.as_ref(), ctx.cwd, files[1], None).await?;

        let lines1: Vec<&str> = content1.lines().collect();
        let lines2: Vec<&str> = content2.lines().collect();

        let sep = opts.separator;
        let mut output = String::new();
        let mut j = 0;

        for line1 in &lines1 {
            let fields1: Vec<&str> = line1.split(sep).collect();
            let key1 = fields1.get(opts.field1 - 1).copied().unwrap_or("");

            let mut matched = false;
            while j < lines2.len() {
                let fields2: Vec<&str> = lines2[j].split(sep).collect();
                let key2 = fields2.get(opts.field2 - 1).copied().unwrap_or("");

                match key1.cmp(key2) {
                    std::cmp::Ordering::Equal => {
                        matched = true;
                        // Output: key, remaining fields from file1, remaining fields from file2
                        output.push_str(key1);
                        for (k, f) in fields1.iter().enumerate() {
                            if k != opts.field1 - 1 {
                                output.push(sep);
                                output.push_str(f);
                            }
                        }
                        for (k, f) in fields2.iter().enumerate() {
                            if k != opts.field2 - 1 {
                                output.push(sep);
                                output.push_str(f);
                            }
                        }
                        output.push('\n');
                        j += 1;
                        break;
                    }
                    std::cmp::Ordering::Greater => {
                        if opts.unpaired.contains(&2) {
                            output.push_str(lines2[j]);
                            output.push('\n');
                        }
                        j += 1;
                    }
                    std::cmp::Ordering::Less => {
                        break;
                    }
                }
            }

            if !matched && opts.unpaired.contains(&1) {
                output.push_str(line1);
                output.push('\n');
            }
        }

        // Remaining unmatched lines from file2
        if opts.unpaired.contains(&2) {
            while j < lines2.len() {
                output.push_str(lines2[j]);
                output.push('\n');
                j += 1;
            }
        }

        Ok(ExecResult::ok(output))
    }
}

async fn read_input(
    fs: &dyn crate::fs::FileSystem,
    cwd: &std::path::Path,
    file: &str,
    stdin: Option<&str>,
) -> Result<String> {
    if file == "-" {
        Ok(stdin.unwrap_or("").to_string())
    } else {
        let path = resolve_path(cwd, file);
        let bytes = fs.read_file(&path).await?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFs};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    async fn run_join(args: &[&str], fs: Arc<dyn FileSystem>) -> ExecResult {
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
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };
        Join.execute(ctx).await.expect("join failed")
    }

    #[tokio::test]
    async fn test_join_basic() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.write_file(Path::new("/f1"), b"a 1\nb 2\nc 3")
            .await
            .unwrap();
        fs.write_file(Path::new("/f2"), b"a x\nb y\nc z")
            .await
            .unwrap();
        let result = run_join(&["/f1", "/f2"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("a 1 x"));
        assert!(result.stdout.contains("b 2 y"));
        assert!(result.stdout.contains("c 3 z"));
    }

    #[tokio::test]
    async fn test_join_custom_field() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.write_file(Path::new("/f1"), b"x a\ny b").await.unwrap();
        fs.write_file(Path::new("/f2"), b"a 1\nb 2").await.unwrap();
        let result = run_join(&["-1", "2", "/f1", "/f2"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("a x 1"));
    }

    #[tokio::test]
    async fn test_join_custom_separator() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.write_file(Path::new("/f1"), b"a:1\nb:2").await.unwrap();
        fs.write_file(Path::new("/f2"), b"a:x\nb:y").await.unwrap();
        let result = run_join(&["-t", ":", "/f1", "/f2"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("a:1:x"));
    }

    #[tokio::test]
    async fn test_join_missing_operand() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_join(&["/f1"], fs).await;
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_join_unpairable() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.write_file(Path::new("/f1"), b"a 1\nb 2\nc 3")
            .await
            .unwrap();
        fs.write_file(Path::new("/f2"), b"a x\nc z").await.unwrap();
        let result = run_join(&["-a", "1", "/f1", "/f2"], fs).await;
        assert_eq!(result.exit_code, 0);
        // "b 2" should appear as unpairable from file1
        assert!(result.stdout.contains("b 2"));
    }
}
