//! Dotenv builtin - load .env files into shell variables
//!
//! Non-standard builtin for parsing .env format files and setting shell variables.

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::{ExecResult, is_internal_variable};

/// dotenv builtin - load environment from .env files
pub struct Dotenv;

/// Parse a .env file content into key-value pairs.
fn parse_dotenv(content: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Split on first '='
        let Some(eq_pos) = trimmed.find('=') else {
            continue;
        };

        let key = trimmed[..eq_pos].trim().to_string();
        let raw_value = trimmed[eq_pos + 1..].trim();

        // Handle quoted values
        let value = if (raw_value.starts_with('"') && raw_value.ends_with('"'))
            || (raw_value.starts_with('\'') && raw_value.ends_with('\''))
        {
            if raw_value.len() >= 2 {
                raw_value[1..raw_value.len() - 1].to_string()
            } else {
                String::new()
            }
        } else {
            // Strip inline comments for unquoted values
            raw_value.split('#').next().unwrap_or("").trim().to_string()
        };

        if !key.is_empty() {
            pairs.push((key, value));
        }
    }

    pairs
}

#[async_trait]
impl Builtin for Dotenv {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut files: Vec<String> = Vec::new();
        let mut export = false;
        let mut override_existing = false;
        let mut print_only = false;
        let mut prefix = String::new();

        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-f" => {
                    i += 1;
                    if i < ctx.args.len() {
                        files.push(ctx.args[i].clone());
                    } else {
                        return Ok(ExecResult::err(
                            "dotenv: -f requires a filename\n".to_string(),
                            1,
                        ));
                    }
                }
                "-e" | "--export" => export = true,
                "-o" | "--override" => override_existing = true,
                "-p" | "--print" => print_only = true,
                "--prefix" => {
                    i += 1;
                    if i < ctx.args.len() {
                        prefix = ctx.args[i].clone();
                    } else {
                        return Ok(ExecResult::err(
                            "dotenv: --prefix requires an argument\n".to_string(),
                            1,
                        ));
                    }
                }
                arg if !arg.starts_with('-') => {
                    files.push(arg.to_string());
                }
                other => {
                    return Ok(ExecResult::err(
                        format!("dotenv: unknown option '{other}'\n"),
                        1,
                    ));
                }
            }
            i += 1;
        }

        // Default file
        if files.is_empty() {
            files.push(".env".to_string());
        }

        let mut output = String::new();

        for file in &files {
            let path = resolve_path(ctx.cwd, file);
            let content = match ctx.fs.read_file(&path).await {
                Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
                Err(e) => {
                    return Ok(ExecResult::err(format!("dotenv: {file}: {e}\n"), 1));
                }
            };

            let pairs = parse_dotenv(&content);

            for (key, value) in pairs {
                let full_key = if prefix.is_empty() {
                    key
                } else {
                    format!("{prefix}{key}")
                };

                if print_only {
                    if export {
                        output.push_str(&format!("export {full_key}={value}\n"));
                    } else {
                        output.push_str(&format!("{full_key}={value}\n"));
                    }
                    continue;
                }

                // THREAT[TM-INJ-018]: Block internal variable prefix injection via dotenv
                if is_internal_variable(&full_key) {
                    continue;
                }
                // Only set if not already set, unless --override
                if override_existing || !ctx.variables.contains_key(&full_key) {
                    ctx.variables.insert(full_key, value);
                }
            }
        }

        // export flag without print sets variables in env (already in variables)
        // The export semantics are handled by the caller; we just set variables.
        let _ = export;

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

    async fn run_with_fs(
        args: &[&str],
        fs: Arc<dyn crate::fs::FileSystem>,
        variables: &mut HashMap<String, String>,
    ) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let ctx = Context {
            args: &args,
            env: &env,
            variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            shell: None,
        };
        Dotenv.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_basic_dotenv() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(std::path::Path::new("/.env"), b"FOO=bar\nBAZ=qux\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        let r = run_with_fs(&[], fs, &mut vars).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(vars.get("FOO").unwrap(), "bar");
        assert_eq!(vars.get("BAZ").unwrap(), "qux");
    }

    #[tokio::test]
    async fn test_quoted_values() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(
            std::path::Path::new("/.env"),
            b"A=\"hello world\"\nB='single'\n",
        )
        .await
        .unwrap();

        let mut vars = HashMap::new();
        let r = run_with_fs(&[], fs, &mut vars).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(vars.get("A").unwrap(), "hello world");
        assert_eq!(vars.get("B").unwrap(), "single");
    }

    #[tokio::test]
    async fn test_comments_and_blanks() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(
            std::path::Path::new("/.env"),
            b"# comment\n\nKEY=val\n  # another comment\n",
        )
        .await
        .unwrap();

        let mut vars = HashMap::new();
        let r = run_with_fs(&[], fs, &mut vars).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars.get("KEY").unwrap(), "val");
    }

    #[tokio::test]
    async fn test_no_override_by_default() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(std::path::Path::new("/.env"), b"X=new\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        vars.insert("X".to_string(), "old".to_string());

        let r = run_with_fs(&[], fs, &mut vars).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(vars.get("X").unwrap(), "old");
    }

    #[tokio::test]
    async fn test_override_flag() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(std::path::Path::new("/.env"), b"X=new\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        vars.insert("X".to_string(), "old".to_string());

        let r = run_with_fs(&["--override"], fs, &mut vars).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(vars.get("X").unwrap(), "new");
    }

    #[tokio::test]
    async fn test_print_mode() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(std::path::Path::new("/.env"), b"A=1\nB=2\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        let r = run_with_fs(&["--print"], fs, &mut vars).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("A=1"));
        assert!(r.stdout.contains("B=2"));
        // print mode should not set variables
        assert!(vars.is_empty());
    }

    #[tokio::test]
    async fn test_prefix() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(std::path::Path::new("/.env"), b"KEY=val\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        let r = run_with_fs(&["--prefix", "APP_"], fs, &mut vars).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(vars.get("APP_KEY").unwrap(), "val");
    }

    #[tokio::test]
    async fn test_file_not_found() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        let mut vars = HashMap::new();
        let r = run_with_fs(&["-f", "/nonexistent"], fs, &mut vars).await;
        assert_eq!(r.exit_code, 1);
    }
}
