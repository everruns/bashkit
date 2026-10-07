//! Secret and identifier generators: `uuidgen`, `openssl rand`.
//!
//! Decisions:
//! - Bytes come from the OS CSPRNG via `getrandom` (same source as
//!   `/dev/urandom`), never from the `$RANDOM` LCG: these are used for
//!   tokens, keys and salts.
//! - `openssl` implements only the `rand` subcommand; anything else fails
//!   with openssl's own "Invalid command" message so scripts can fall back.
//! - THREAT[TM-DOS-123]: `openssl rand N` is capped at
//!   `OPENSSL_RAND_MAX_BYTES` so one call cannot allocate unbounded memory.
//! - `uuidgen` emits random (v4) UUIDs; time-based `-t` and name-based
//!   `-m`/`-s` are not supported (no host clock/MAC leakage, L-RAND-001).

use async_trait::async_trait;
use base64::Engine;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Largest `openssl rand` request (1 MiB).
pub(crate) const OPENSSL_RAND_MAX_BYTES: usize = 1024 * 1024;

fn random_bytes(n: usize) -> std::result::Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    getrandom::fill(&mut buf).map_err(|_| "random source unavailable".to_string())?;
    Ok(buf)
}

/// A random version 4 UUID in lowercase hyphenated form.
pub(crate) fn uuid_v4() -> std::result::Result<String, String> {
    let mut b = random_bytes(16)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    ))
}

/// The `uuidgen` builtin: `uuidgen [-r|--random]`.
pub struct Uuidgen;

#[async_trait]
impl Builtin for Uuidgen {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        for arg in ctx.args {
            match arg.as_str() {
                "-r" | "--random" => {}
                "-h" | "--help" => {
                    return Ok(ExecResult::ok(
                        "Usage: uuidgen [-r|--random]\nCreate a new random (version 4) UUID.\n",
                    ));
                }
                "-t" | "--time" | "-m" | "--md5" | "-s" | "--sha1" => {
                    return Ok(ExecResult::err(
                        format!("uuidgen: {arg}: not supported in bashkit (random UUIDs only)\n"),
                        1,
                    ));
                }
                other => {
                    return Ok(ExecResult::err(
                        format!(
                            "uuidgen: invalid option -- '{}'\n",
                            other.trim_start_matches('-')
                        ),
                        1,
                    ));
                }
            }
        }
        match uuid_v4() {
            Ok(u) => Ok(ExecResult::ok(format!("{u}\n"))),
            Err(e) => Ok(ExecResult::err(format!("uuidgen: {e}\n"), 1)),
        }
    }
}

/// The `openssl` builtin: only `openssl rand [-hex|-base64] [-out FILE] NUM`.
pub struct Openssl;

const RAND_USAGE: &str = "rand: Use -help for summary.\n";

#[async_trait]
impl Builtin for Openssl {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some((cmd, rest)) = ctx.args.split_first() else {
            return Ok(ExecResult::err(
                "openssl: interactive mode is not supported in bashkit\n",
                1,
            ));
        };
        if cmd != "rand" {
            return Ok(ExecResult::err(
                format!("Invalid command '{cmd}'; type \"help\" for a list.\n"),
                1,
            ));
        }
        let mut hex = false;
        let mut b64 = false;
        let mut out: Option<&str> = None;
        let mut num: Option<&str> = None;
        let mut i = 0;
        while i < rest.len() {
            match rest[i].as_str() {
                "-hex" => hex = true,
                "-base64" => b64 = true,
                "-out" => {
                    i += 1;
                    match rest.get(i) {
                        Some(f) => out = Some(f),
                        None => return Ok(ExecResult::err(RAND_USAGE, 1)),
                    }
                }
                "-help" => {
                    return Ok(ExecResult::ok(
                        "Usage: rand [options] num\n -out outfile  Output file\n -base64       Base64 encode output\n -hex          Hex encode output\n",
                    ));
                }
                a if num.is_none() && !a.starts_with('-') => num = Some(a),
                a => {
                    return Ok(ExecResult::err(
                        format!("rand: Unknown option or number: {a}\n{RAND_USAGE}"),
                        1,
                    ));
                }
            }
            i += 1;
        }
        let Some(num) = num else {
            return Ok(ExecResult::err(RAND_USAGE, 1));
        };
        let n: usize = match num.parse() {
            Ok(n) => n,
            Err(_) => {
                return Ok(ExecResult::err(
                    format!("rand: Can't parse \"{num}\" as a number\n{RAND_USAGE}"),
                    1,
                ));
            }
        };
        if n > OPENSSL_RAND_MAX_BYTES {
            return Ok(ExecResult::err(
                format!("rand: {n} bytes exceeds the {OPENSSL_RAND_MAX_BYTES} byte limit\n"),
                1,
            ));
        }
        let bytes = match random_bytes(n) {
            Ok(b) => b,
            Err(e) => return Ok(ExecResult::err(format!("rand: {e}\n"), 1)),
        };
        let data: Vec<u8> = if n == 0 {
            Vec::new()
        } else if hex {
            let mut s: String = bytes.iter().map(|x| format!("{x:02x}")).collect();
            s.push('\n');
            s.into_bytes()
        } else if b64 {
            let enc = base64::engine::general_purpose::STANDARD.encode(&bytes);
            // openssl wraps base64 at 64 columns.
            let mut s = String::with_capacity(enc.len() + enc.len() / 64 + 1);
            for chunk in enc.as_bytes().chunks(64) {
                s.push_str(&String::from_utf8_lossy(chunk));
                s.push('\n');
            }
            s.into_bytes()
        } else {
            bytes
        };
        match out {
            Some(f) => {
                let path = resolve_path(ctx.cwd, f);
                if let Err(e) = ctx.fs.write_file(&path, &data).await {
                    return Ok(ExecResult::err(format!("rand: {f}: {e}\n"), 1));
                }
                Ok(ExecResult::ok(String::new()))
            }
            None => Ok(ExecResult::ok(crate::StreamData::from(data))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_v4_shape() {
        let u = uuid_v4().unwrap();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
        assert!(matches!(&u[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(u, uuid_v4().unwrap());
    }

    #[tokio::test]
    async fn openssl_rand_limits() {
        let mut bash = crate::Bash::new();
        let r = bash.exec("openssl rand -hex 2000000").await.unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("limit"));
        let r = bash.exec("uuidgen -t").await.unwrap();
        assert_eq!(r.exit_code, 1);
    }
}
