//! Checksum builtins - md5sum, sha1sum, sha256sum

use async_trait::async_trait;
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// md5sum builtin - compute MD5 message digest
pub struct Md5sum;

/// sha1sum builtin - compute SHA-1 message digest
pub struct Sha1sum;

/// sha256sum builtin - compute SHA-256 message digest
pub struct Sha256sum;

#[async_trait]
impl Builtin for Md5sum {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        checksum_execute::<Md5>(&ctx, "md5sum").await
    }
}

#[async_trait]
impl Builtin for Sha1sum {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        checksum_execute::<Sha1>(&ctx, "sha1sum").await
    }
}

#[async_trait]
impl Builtin for Sha256sum {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        checksum_execute::<Sha256>(&ctx, "sha256sum").await
    }
}

async fn checksum_execute<D: Digest>(ctx: &Context<'_>, cmd: &str) -> Result<ExecResult> {
    let files: Vec<&String> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

    let mut output = String::new();

    if files.is_empty() {
        // Read from stdin
        let input = ctx.stdin.unwrap_or("");
        let hash = hex_digest::<D>(input.as_bytes());
        output.push_str(&hash);
        output.push_str("  -\n");
    } else {
        for file in &files {
            let path = if file.starts_with('/') {
                std::path::PathBuf::from(file)
            } else {
                ctx.cwd.join(file)
            };

            match ctx.fs.read_file(&path).await {
                Ok(content) => {
                    let hash = hex_digest::<D>(&content);
                    output.push_str(&hash);
                    output.push_str("  ");
                    output.push_str(file);
                    output.push('\n');
                }
                Err(e) => {
                    return Ok(ExecResult::err(format!("{}: {}: {}\n", cmd, file, e), 1));
                }
            }
        }
    }

    Ok(ExecResult::ok(output))
}

fn hex_digest<D: Digest>(data: &[u8]) -> String {
    let result = D::digest(data);
    result.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_checksum<B: Builtin>(
        builtin: &B,
        args: &[&str],
        stdin: Option<&str>,
    ) -> ExecResult {
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        builtin.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_md5sum_stdin() {
        let result = run_checksum(&Md5sum, &[], Some("hello\n")).await;
        assert_eq!(result.exit_code, 0);
        // md5("hello\n") = b1946ac92492d2347c6235b4d2611184
        assert!(result
            .stdout
            .starts_with("b1946ac92492d2347c6235b4d2611184"));
        assert!(result.stdout.contains("  -"));
    }

    #[tokio::test]
    async fn test_sha256sum_stdin() {
        let result = run_checksum(&Sha256sum, &[], Some("hello\n")).await;
        assert_eq!(result.exit_code, 0);
        // sha256("hello\n") = 5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03
        assert!(result
            .stdout
            .starts_with("5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"));
    }

    #[tokio::test]
    async fn test_sha1sum_stdin() {
        let result = run_checksum(&Sha1sum, &[], Some("hello\n")).await;
        assert_eq!(result.exit_code, 0);
        // sha1("hello\n") = f572d396fae9206628714fb2ce00f72e94f2258f
        assert!(result
            .stdout
            .starts_with("f572d396fae9206628714fb2ce00f72e94f2258f"));
    }

    #[tokio::test]
    async fn test_md5sum_empty() {
        let result = run_checksum(&Md5sum, &[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        // md5("") = d41d8cd98f00b204e9800998ecf8427e
        assert!(result
            .stdout
            .starts_with("d41d8cd98f00b204e9800998ecf8427e"));
    }
}
