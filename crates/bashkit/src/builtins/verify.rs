//! Verify builtin - file hash verification
//!
//! Non-standard builtin for computing and verifying file checksums.

use async_trait::async_trait;
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// verify builtin - compute and verify file hashes
pub struct Verify;

fn hex_digest_bytes<D: Digest>(data: &[u8]) -> String {
    let result = D::digest(data);
    result.iter().map(|b| format!("{:02x}", b)).collect()
}

fn compute_hash(algo: &str, data: &[u8]) -> Option<String> {
    match algo {
        "sha256" => Some(hex_digest_bytes::<Sha256>(data)),
        "sha1" => Some(hex_digest_bytes::<Sha1>(data)),
        "md5" => Some(hex_digest_bytes::<Md5>(data)),
        _ => None,
    }
}

#[async_trait]
impl Builtin for Verify {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut algo = "sha256".to_string();
        let mut generate = false;
        let mut quiet = false;
        let mut positional: Vec<String> = Vec::new();

        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-a" => {
                    i += 1;
                    if i < ctx.args.len() {
                        algo = ctx.args[i].clone();
                    } else {
                        return Ok(ExecResult::err(
                            "verify: -a requires an algorithm\n".to_string(),
                            1,
                        ));
                    }
                }
                "--generate" | "-g" => generate = true,
                "--quiet" | "-q" => quiet = true,
                arg if !arg.starts_with('-') => {
                    positional.push(arg.to_string());
                }
                other => {
                    return Ok(ExecResult::err(
                        format!("verify: unknown option '{other}'\n"),
                        1,
                    ));
                }
            }
            i += 1;
        }

        if positional.is_empty() {
            return Ok(ExecResult::err(
                "verify: usage: verify [OPTIONS] file [expected-hash]\n".to_string(),
                1,
            ));
        }

        let file = &positional[0];
        let path = resolve_path(ctx.cwd, file);

        let data = match ctx.fs.read_file(&path).await {
            Ok(bytes) => bytes,
            Err(e) => {
                return Ok(ExecResult::err(format!("verify: {file}: {e}\n"), 1));
            }
        };

        let hash = match compute_hash(&algo, &data) {
            Some(h) => h,
            None => {
                return Ok(ExecResult::err(
                    format!("verify: unsupported algorithm '{algo}'\n"),
                    1,
                ));
            }
        };

        if generate {
            return Ok(ExecResult::ok(format!("{hash}  {file}\n")));
        }

        if positional.len() >= 2 {
            // Verification mode
            let expected = &positional[1];
            if hash == *expected {
                if quiet {
                    Ok(ExecResult::with_code("", 0))
                } else {
                    Ok(ExecResult::ok(format!("{file}: OK\n")))
                }
            } else if quiet {
                Ok(ExecResult::with_code("", 1))
            } else {
                Ok(ExecResult::err(
                    format!("{file}: FAILED (expected {expected}, got {hash})\n"),
                    1,
                ))
            }
        } else {
            // Just print the hash
            Ok(ExecResult::ok(format!("{hash}  {file}\n")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

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
        Verify.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_generate_sha256() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(std::path::Path::new("/test.txt"), b"hello\n")
            .await
            .unwrap();

        let r = run_with_fs(&["-g", "test.txt"], fs).await;
        assert_eq!(r.exit_code, 0);
        // sha256("hello\n") = 5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03
        assert!(
            r.stdout
                .starts_with("5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03")
        );
        assert!(r.stdout.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_verify_correct_hash() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(std::path::Path::new("/test.txt"), b"hello\n")
            .await
            .unwrap();

        let hash = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
        let r = run_with_fs(&["test.txt", hash], fs).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("OK"));
    }

    #[tokio::test]
    async fn test_verify_wrong_hash() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(std::path::Path::new("/test.txt"), b"hello\n")
            .await
            .unwrap();

        let r = run_with_fs(&["test.txt", "badhash"], fs).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("FAILED"));
    }

    #[tokio::test]
    async fn test_md5_algorithm() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(std::path::Path::new("/test.txt"), b"hello\n")
            .await
            .unwrap();

        let r = run_with_fs(&["-a", "md5", "-g", "test.txt"], fs).await;
        assert_eq!(r.exit_code, 0);
        // md5("hello\n") = b1946ac92492d2347c6235b4d2611184
        assert!(r.stdout.starts_with("b1946ac92492d2347c6235b4d2611184"));
    }

    #[tokio::test]
    async fn test_quiet_mode() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(std::path::Path::new("/test.txt"), b"hello\n")
            .await
            .unwrap();

        let hash = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
        let r = run_with_fs(&["-q", "test.txt", hash], fs).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_file_not_found() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        let r = run_with_fs(&["/nonexistent"], fs).await;
        assert_eq!(r.exit_code, 1);
    }

    #[tokio::test]
    async fn test_unsupported_algorithm() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(std::path::Path::new("/test.txt"), b"hi")
            .await
            .unwrap();

        let r = run_with_fs(&["-a", "sha512", "test.txt"], fs).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("unsupported"));
    }

    #[tokio::test]
    async fn test_no_args() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        let r = run_with_fs(&[], fs).await;
        assert_eq!(r.exit_code, 1);
    }
}
