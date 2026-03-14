//! Glob command builtin - pattern matching for strings and files
//!
//! Non-standard builtin for glob pattern matching against strings or VFS files.

use async_trait::async_trait;

use super::{Builtin, Context, glob_match, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// glob builtin - match strings or files against glob patterns
pub struct GlobCmd;

#[async_trait]
impl Builtin for GlobCmd {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut invert = false;
        let mut quiet = false;
        let mut count = false;
        let mut files_mode = false;
        let mut positional: Vec<String> = Vec::new();

        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-v" => invert = true,
                "-q" => quiet = true,
                "-c" => count = true,
                "--files" => files_mode = true,
                arg if !arg.starts_with('-') => {
                    positional.push(arg.to_string());
                }
                other => {
                    return Ok(ExecResult::err(
                        format!("glob: unknown option '{other}'\n"),
                        1,
                    ));
                }
            }
            i += 1;
        }

        if positional.is_empty() {
            return Ok(ExecResult::err(
                "glob: usage: glob [OPTIONS] pattern [string...]\n".to_string(),
                1,
            ));
        }

        let pattern = &positional[0];

        if files_mode {
            return self.match_files(&ctx, pattern, invert, quiet, count).await;
        }

        // Get strings to match: from args or stdin
        let strings: Vec<String> = if positional.len() > 1 {
            positional[1..].to_vec()
        } else if let Some(input) = ctx.stdin {
            input
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect()
        } else {
            return Ok(ExecResult::err(
                "glob: no strings to match\n".to_string(),
                1,
            ));
        };

        let mut matched = Vec::new();
        for s in &strings {
            let m = glob_match(s, pattern);
            let include = if invert { !m } else { m };
            if include {
                matched.push(s.as_str());
            }
        }

        if count {
            return Ok(ExecResult::ok(format!("{}\n", matched.len())));
        }

        if matched.is_empty() {
            return Ok(ExecResult::with_code("", 1));
        }

        if quiet {
            return Ok(ExecResult::with_code("", 0));
        }

        let mut output = String::new();
        for m in &matched {
            output.push_str(m);
            output.push('\n');
        }
        Ok(ExecResult::ok(output))
    }
}

impl GlobCmd {
    async fn match_files(
        &self,
        ctx: &Context<'_>,
        pattern: &str,
        invert: bool,
        quiet: bool,
        count: bool,
    ) -> Result<ExecResult> {
        let dir = resolve_path(ctx.cwd, ".");
        let entries = match ctx.fs.read_dir(&dir).await {
            Ok(e) => e,
            Err(e) => {
                return Ok(ExecResult::err(
                    format!("glob: {}: {e}\n", dir.display()),
                    1,
                ));
            }
        };

        let mut matched = Vec::new();
        for entry in &entries {
            let m = glob_match(&entry.name, pattern);
            let include = if invert { !m } else { m };
            if include {
                matched.push(entry.name.as_str());
            }
        }

        if count {
            return Ok(ExecResult::ok(format!("{}\n", matched.len())));
        }

        if matched.is_empty() {
            return Ok(ExecResult::with_code("", 1));
        }

        if quiet {
            return Ok(ExecResult::with_code("", 0));
        }

        let mut output = String::new();
        for m in &matched {
            output.push_str(m);
            output.push('\n');
        }
        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run(args: &[&str], stdin: Option<&str>) -> ExecResult {
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
        };
        GlobCmd.execute(ctx).await.unwrap()
    }

    async fn run_with_fs(args: &[&str], fs: Arc<dyn crate::fs::FileSystem>) -> ExecResult {
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
        };
        GlobCmd.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_basic_match() {
        let r = run(&["*.txt", "hello.txt", "hello.rs"], None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("hello.txt"));
        assert!(!r.stdout.contains("hello.rs"));
    }

    #[tokio::test]
    async fn test_stdin_match() {
        let r = run(&["*.txt"], Some("a.txt\nb.rs\nc.txt\n")).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("a.txt"));
        assert!(r.stdout.contains("c.txt"));
        assert!(!r.stdout.contains("b.rs"));
    }

    #[tokio::test]
    async fn test_invert() {
        let r = run(&["-v", "*.txt", "a.txt", "b.rs"], None).await;
        assert_eq!(r.exit_code, 0);
        assert!(!r.stdout.contains("a.txt"));
        assert!(r.stdout.contains("b.rs"));
    }

    #[tokio::test]
    async fn test_count() {
        let r = run(&["-c", "*.txt", "a.txt", "b.txt", "c.rs"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "2");
    }

    #[tokio::test]
    async fn test_quiet() {
        let r = run(&["-q", "*.txt", "a.txt"], None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.is_empty());

        let r = run(&["-q", "*.txt", "a.rs"], None).await;
        assert_eq!(r.exit_code, 1);
    }

    #[tokio::test]
    async fn test_no_match() {
        let r = run(&["*.txt", "hello.rs"], None).await;
        assert_eq!(r.exit_code, 1);
    }

    #[tokio::test]
    async fn test_files_mode() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(std::path::Path::new("/hello.txt"), b"")
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/world.rs"), b"")
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/test.txt"), b"")
            .await
            .unwrap();

        let r = run_with_fs(&["--files", "*.txt"], fs).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("hello.txt"));
        assert!(r.stdout.contains("test.txt"));
        assert!(!r.stdout.contains("world.rs"));
    }

    #[tokio::test]
    async fn test_no_args() {
        let r = run(&[], None).await;
        assert_eq!(r.exit_code, 1);
    }
}
