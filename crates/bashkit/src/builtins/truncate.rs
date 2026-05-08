//! truncate builtin.
//!
//! Argument surface is generated from uutils/coreutils' `uu_app()` via the
//! `bashkit-coreutils-port` codegen tool — see `generated/truncate_args.rs`
//! and `crates/bashkit-coreutils-port/`. Behaviour is implemented locally
//! against the bashkit VFS (read/resize/write — the trait has no dedicated
//! truncate primitive yet).
//!
//! Security decision: validate target lengths against the active filesystem's
//! configured limits before resizing buffers. `write_file` enforces those
//! limits too late to guard this built-in's in-memory zero-fill path.

use async_trait::async_trait;
use std::ffi::OsString;

use super::generated::truncate_args::truncate_command;
use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

pub struct Truncate;

#[async_trait]
impl Builtin for Truncate {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let argv: Vec<OsString> = std::iter::once(OsString::from("truncate"))
            .chain(ctx.args.iter().map(OsString::from))
            .collect();

        let cmd = truncate_command().help_template("Usage: {usage}\n{about}\n\n{all-args}\n");
        let matches = match cmd.try_get_matches_from(argv) {
            Ok(m) => m,
            Err(e) => {
                let kind = e.kind();
                let rendered = e.render().to_string();
                if matches!(
                    kind,
                    clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
                ) {
                    return Ok(ExecResult::ok(rendered));
                }
                return Ok(ExecResult::err(rendered, 2));
            }
        };

        if matches.get_flag("io-blocks") {
            return Ok(ExecResult::err(
                "truncate: --io-blocks not yet implemented in bashkit\n".to_string(),
                1,
            ));
        }

        let no_create = matches.get_flag("no-create");

        let target_spec: Option<TargetSize> =
            if let Some(rfile) = matches.get_one::<String>("reference") {
                let path = resolve_path(ctx.cwd, rfile);
                let meta = match ctx.fs.stat(&path).await {
                    Ok(m) => m,
                    Err(e) => {
                        return Ok(ExecResult::err(
                            format!(
                                "truncate: cannot stat reference '{}': {}\n",
                                rfile,
                                error_message(&e)
                            ),
                            1,
                        ));
                    }
                };
                Some(TargetSize::Absolute(meta.size))
            } else if let Some(spec) = matches.get_one::<String>("size") {
                match parse_size(spec) {
                    Ok(t) => Some(t),
                    Err(e) => return Ok(ExecResult::err(format!("truncate: {e}\n"), 1)),
                }
            } else {
                None
            };

        let Some(target_spec) = target_spec else {
            return Ok(ExecResult::err(
                "truncate: you must specify either --size or --reference\n".to_string(),
                1,
            ));
        };

        let files: Vec<String> = matches
            .get_many::<OsString>("files")
            .map(|vs| vs.map(|v| v.to_string_lossy().into_owned()).collect())
            .unwrap_or_default();

        for file in &files {
            let path = resolve_path(ctx.cwd, file);
            let exists = ctx.fs.exists(&path).await.unwrap_or(false);

            if !exists && no_create {
                continue;
            }

            let current = if exists {
                match ctx.fs.read_file(&path).await {
                    Ok(b) => b,
                    Err(e) => {
                        return Ok(ExecResult::err(
                            format!(
                                "truncate: cannot open '{}' for reading: {}\n",
                                file,
                                error_message(&e)
                            ),
                            1,
                        ));
                    }
                }
            } else {
                Vec::new()
            };

            let new_len = match target_spec.resolve(current.len() as u64) {
                Ok(n) => n,
                Err(e) => return Ok(ExecResult::err(format!("truncate: {e}\n"), 1)),
            };

            let vfs_limit = target_size_limit(&ctx);
            if new_len > vfs_limit {
                return Ok(ExecResult::err(
                    format!(
                        "truncate: target size {new_len} exceeds VFS limit ({vfs_limit} bytes)\n"
                    ),
                    1,
                ));
            }

            let mut next = current;
            // THREAT[TM-DOS-005, TM-DOS-040]: fail before allocation if the
            // requested virtual file length cannot be represented locally.
            let new_len_usize = match usize::try_from(new_len) {
                Ok(n) => n,
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("truncate: target size {new_len} exceeds addressable memory\n"),
                        1,
                    ));
                }
            };
            if next.len() > new_len_usize {
                next.truncate(new_len_usize);
            } else {
                next.resize(new_len_usize, 0);
            }

            if let Err(e) = ctx.fs.write_file(&path, &next).await {
                return Ok(ExecResult::err(
                    format!("truncate: cannot write '{}': {}\n", file, error_message(&e)),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

fn error_message(e: &crate::error::Error) -> String {
    e.to_string()
}

fn target_size_limit(ctx: &Context<'_>) -> u64 {
    let limits = ctx.fs.limits();
    limits.max_file_size.min(limits.max_total_bytes)
}

#[derive(Debug, Clone, Copy)]
enum TargetSize {
    Absolute(u64),
    ExtendBy(u64),
    ReduceBy(u64),
    AtMost(u64),
    AtLeast(u64),
    RoundDownTo(u64),
    RoundUpTo(u64),
}

impl TargetSize {
    fn resolve(self, current: u64) -> std::result::Result<u64, String> {
        Ok(match self {
            TargetSize::Absolute(n) => n,
            TargetSize::ExtendBy(n) => current.checked_add(n).ok_or("size overflow")?,
            TargetSize::ReduceBy(n) => current.saturating_sub(n),
            TargetSize::AtMost(n) => current.min(n),
            TargetSize::AtLeast(n) => current.max(n),
            TargetSize::RoundDownTo(n) => {
                if n == 0 {
                    return Err("round-down multiple cannot be zero".into());
                }
                (current / n) * n
            }
            TargetSize::RoundUpTo(n) => {
                if n == 0 {
                    return Err("round-up multiple cannot be zero".into());
                }
                let rem = current % n;
                if rem == 0 {
                    current
                } else {
                    current.checked_add(n - rem).ok_or("size overflow")?
                }
            }
        })
    }
}

fn parse_size(raw: &str) -> std::result::Result<TargetSize, String> {
    let (op, rest) = match raw.chars().next() {
        Some('+') => (Some(b'+'), &raw[1..]),
        Some('-') => (Some(b'-'), &raw[1..]),
        Some('<') => (Some(b'<'), &raw[1..]),
        Some('>') => (Some(b'>'), &raw[1..]),
        Some('/') => (Some(b'/'), &raw[1..]),
        Some('%') => (Some(b'%'), &raw[1..]),
        _ => (None, raw),
    };

    let n = parse_size_number(rest).ok_or_else(|| format!("invalid number in size '{raw}'"))?;

    Ok(match op {
        None => TargetSize::Absolute(n),
        Some(b'+') => TargetSize::ExtendBy(n),
        Some(b'-') => TargetSize::ReduceBy(n),
        Some(b'<') => TargetSize::AtMost(n),
        Some(b'>') => TargetSize::AtLeast(n),
        Some(b'/') => TargetSize::RoundDownTo(n),
        Some(b'%') => TargetSize::RoundUpTo(n),
        _ => unreachable!(),
    })
}

/// Parse `<digits>[<unit>]` where unit follows GNU coreutils' truncate(1):
/// `K`/`M`/`G`/`T`/`P`/`E`/`Z`/`Y` = 1024-based, `KB`/`MB`/... = 1000-based.
fn parse_size_number(raw: &str) -> Option<u64> {
    let split = raw
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)
        .unwrap_or(raw.len());
    let (digits, unit) = raw.split_at(split);
    if digits.is_empty() {
        return None;
    }
    let n: u64 = digits.parse().ok()?;
    let mul = match unit {
        "" | "B" => 1,
        "K" => 1024,
        "KB" => 1000,
        "M" => 1024 * 1024,
        "MB" => 1000 * 1000,
        "G" => 1024u64.pow(3),
        "GB" => 1000u64.pow(3),
        "T" => 1024u64.pow(4),
        "TB" => 1000u64.pow(4),
        "P" => 1024u64.pow(5),
        "PB" => 1000u64.pow(5),
        "E" => 1024u64.pow(6),
        "EB" => 1000u64.pow(6),
        _ => return None,
    };
    n.checked_mul(mul)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, FsLimits, InMemoryFs};

    async fn run_truncate_with_fs(args: &[&str], fs: Arc<InMemoryFs>) -> ExecResult {
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
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Truncate.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn rejects_target_above_vfs_limit_before_write() {
        let fs = Arc::new(InMemoryFs::with_limits(
            FsLimits::new().max_file_size(10).max_total_bytes(10),
        ));
        let result = run_truncate_with_fs(&["-s", "11", "/tmp/too-large"], fs.clone()).await;

        assert_eq!(result.exit_code, 1);
        assert!(
            result.stderr.contains("exceeds VFS limit"),
            "stderr was: {}",
            result.stderr
        );
        assert!(
            !fs.exists(std::path::Path::new("/tmp/too-large"))
                .await
                .unwrap(),
            "oversized truncate must not create a file"
        );
    }

    #[test]
    fn parse_size_plain_number() {
        assert!(matches!(
            parse_size("100").unwrap(),
            TargetSize::Absolute(100)
        ));
    }

    #[test]
    fn parse_size_with_units() {
        assert!(matches!(
            parse_size("1K").unwrap(),
            TargetSize::Absolute(1024)
        ));
        assert!(matches!(
            parse_size("2KB").unwrap(),
            TargetSize::Absolute(2000)
        ));
        assert!(matches!(
            parse_size("1M").unwrap(),
            TargetSize::Absolute(1_048_576)
        ));
    }

    #[test]
    fn parse_size_relative_ops() {
        assert!(matches!(
            parse_size("+100").unwrap(),
            TargetSize::ExtendBy(100)
        ));
        assert!(matches!(
            parse_size("-50").unwrap(),
            TargetSize::ReduceBy(50)
        ));
        assert!(matches!(
            parse_size("<200").unwrap(),
            TargetSize::AtMost(200)
        ));
        assert!(matches!(
            parse_size(">300").unwrap(),
            TargetSize::AtLeast(300)
        ));
        assert!(matches!(
            parse_size("/16").unwrap(),
            TargetSize::RoundDownTo(16)
        ));
        assert!(matches!(
            parse_size("%16").unwrap(),
            TargetSize::RoundUpTo(16)
        ));
    }

    #[test]
    fn parse_size_rejects_garbage() {
        assert!(parse_size("abc").is_err());
        assert!(parse_size("100Q").is_err());
        assert!(parse_size("").is_err());
    }

    #[test]
    fn target_resolve_absolute() {
        assert_eq!(TargetSize::Absolute(100).resolve(50).unwrap(), 100);
    }

    #[test]
    fn target_resolve_extend_and_reduce() {
        assert_eq!(TargetSize::ExtendBy(10).resolve(50).unwrap(), 60);
        assert_eq!(TargetSize::ReduceBy(20).resolve(50).unwrap(), 30);
        // Reduce below zero clamps at 0 (matches GNU truncate).
        assert_eq!(TargetSize::ReduceBy(100).resolve(50).unwrap(), 0);
    }

    #[test]
    fn target_resolve_at_most_at_least() {
        assert_eq!(TargetSize::AtMost(40).resolve(50).unwrap(), 40);
        assert_eq!(TargetSize::AtMost(60).resolve(50).unwrap(), 50);
        assert_eq!(TargetSize::AtLeast(40).resolve(50).unwrap(), 50);
        assert_eq!(TargetSize::AtLeast(60).resolve(50).unwrap(), 60);
    }

    #[test]
    fn target_resolve_round_to_multiple() {
        assert_eq!(TargetSize::RoundDownTo(16).resolve(50).unwrap(), 48);
        assert_eq!(TargetSize::RoundUpTo(16).resolve(50).unwrap(), 64);
        assert_eq!(TargetSize::RoundDownTo(16).resolve(48).unwrap(), 48);
        assert_eq!(TargetSize::RoundUpTo(16).resolve(48).unwrap(), 48);
        assert!(TargetSize::RoundDownTo(0).resolve(10).is_err());
        assert!(TargetSize::RoundUpTo(0).resolve(10).is_err());
    }
}
