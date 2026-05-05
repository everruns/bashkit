//! shuf builtin.
//!
//! Argument surface is generated from uutils/coreutils' `uu_app()` via the
//! `bashkit-coreutils-port` codegen tool — see `generated/shuf_args.rs`
//! and `crates/bashkit-coreutils-port/`. Behaviour is implemented locally
//! against the bashkit VFS.

use async_trait::async_trait;
use std::ffi::OsString;
use std::ops::RangeInclusive;
use std::path::Path;

use super::generated::shuf_args::shuf_command;
use super::{Builtin, Context, read_text_file};
use crate::error::Result;
use crate::interpreter::ExecResult;

pub struct Shuf;

#[async_trait]
impl Builtin for Shuf {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let argv: Vec<OsString> = std::iter::once(OsString::from("shuf"))
            .chain(ctx.args.iter().map(OsString::from))
            .collect();

        let cmd = shuf_command().help_template("Usage: {usage}\n{about}\n\n{all-args}\n");
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

        for unsupported in ["random-seed", "random-source"] {
            if matches.contains_id(unsupported)
                && matches.value_source(unsupported)
                    != Some(clap::parser::ValueSource::DefaultValue)
            {
                return Ok(ExecResult::err(
                    format!("shuf: --{unsupported} not yet implemented in bashkit\n",),
                    1,
                ));
            }
        }

        let echo = matches.get_flag("echo");
        let repeat = matches.get_flag("repeat");
        let zero_terminated = matches.get_flag("zero-terminated");
        // The generated args declare value_parsers that pre-parse these
        // values for us — we read the typed result, not the raw String.
        let head_count: Option<u64> = matches
            .get_many::<u64>("head-count")
            .and_then(|mut vs| vs.next_back().copied());
        let output_path = matches.get_one::<std::path::PathBuf>("output").cloned();
        let input_range = matches
            .get_one::<RangeInclusive<u64>>("input-range")
            .cloned();
        let positionals: Vec<String> = matches
            .get_many::<OsString>("file-or-args")
            .map(|vs| vs.map(|v| v.to_string_lossy().into_owned()).collect())
            .unwrap_or_default();

        let separator = if zero_terminated { '\0' } else { '\n' };

        let items: Vec<String> = if echo {
            positionals
        } else if let Some(range) = input_range {
            range.map(|n| n.to_string()).collect()
        } else {
            // Reading lines: positional file path, "-", or absent (stdin).
            let raw = match positionals.first() {
                None => ctx.stdin.unwrap_or("").to_string(),
                Some(s) if s == "-" => ctx.stdin.unwrap_or("").to_string(),
                Some(file) => {
                    let path = if Path::new(file).is_absolute() {
                        file.clone()
                    } else {
                        ctx.cwd.join(file).to_string_lossy().into_owned()
                    };
                    match read_text_file(&*ctx.fs, Path::new(&path), "shuf").await {
                        Ok(t) => t,
                        Err(e) => return Ok(e),
                    }
                }
            };
            split_separated(&raw, separator)
        };

        let mut rng = SmallRng::seed_from_now();

        let output_lines: Vec<String> = if repeat {
            // -r samples *with* replacement: each pick is independent.
            // GNU shuf -r without -n loops forever; bashkit caps at 1
            // and requires -n, mirroring the safe behavior an embedded
            // VFS shell needs.
            let count = match head_count {
                Some(n) => n,
                None => {
                    return Ok(ExecResult::err(
                        "shuf: --repeat requires --head-count to be finite\n".to_string(),
                        1,
                    ));
                }
            };
            if items.is_empty() {
                return Ok(ExecResult::err(
                    "shuf: no lines to repeat from\n".to_string(),
                    1,
                ));
            }
            (0..count)
                .map(|_| items[rng.next_usize_lt(items.len())].clone())
                .collect()
        } else {
            // Without -r: Fisher-Yates shuffle, then truncate to -n.
            let mut v = items;
            for i in (1..v.len()).rev() {
                let j = rng.next_usize_lt(i + 1);
                v.swap(i, j);
            }
            if let Some(n) = head_count {
                v.truncate(n as usize);
            }
            v
        };

        let mut out = String::with_capacity(output_lines.iter().map(String::len).sum::<usize>());
        for line in &output_lines {
            out.push_str(line);
            out.push(separator);
        }

        if let Some(path) = output_path {
            let resolved = if path.is_absolute() {
                path.clone()
            } else {
                ctx.cwd.join(&path)
            };
            if let Err(e) = ctx.fs.write_file(&resolved, out.as_bytes()).await {
                return Ok(ExecResult::err(
                    format!("shuf: cannot write '{}': {e}\n", path.display()),
                    1,
                ));
            }
            return Ok(ExecResult::ok(String::new()));
        }

        Ok(ExecResult::ok(out))
    }
}

fn split_separated(raw: &str, sep: char) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = raw.split(sep).map(str::to_string).collect();
    // Trailing separator yields an empty trailing element; drop it to
    // match GNU shuf's "lines" model.
    if out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

// Note: range parsing is handled by `parse_range` inlined into the
// generated `shuf_args.rs` (codegen copies it from uutils' source).
// We don't need a second parser here; clap returns the parsed
// `RangeInclusive<u64>` directly.

/// xorshift64* RNG. Adequate for line shuffling — not cryptographic.
/// We choose this over a `rand` workspace dep (currently feature-
/// gated behind `bot-auth`) to keep `shuf` available with no feature
/// flags. Non-cryptographic shuffling does not need rand's quality
/// guarantees.
struct SmallRng {
    state: u64,
}

impl SmallRng {
    fn seed_from_now() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x123_4567_89AB_CDEF);
        // SystemTime::now can return a tiny duration in tight loops; XOR
        // with an address-derived value so two `SmallRng::seed_from_now()`
        // calls in the same nanosecond don't produce the same state.
        let addr_bits = (&nanos as *const u64) as u64;
        let mixed = nanos ^ addr_bits.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        Self {
            state: if mixed == 0 {
                0xDEAD_BEEF_CAFE_F00D
            } else {
                mixed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn next_usize_lt(&mut self, bound: usize) -> usize {
        debug_assert!(bound > 0);
        // Lemire-style unbiased reduction. Adequate for our use case;
        // the rejection branch is rare for small `bound`.
        let bound = bound as u64;
        loop {
            let x = self.next_u64();
            let m = (x as u128) * (bound as u128);
            let l = m as u64;
            if l >= bound.wrapping_neg() % bound {
                return (m >> 64) as usize;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_separated_drops_trailing_empty() {
        assert_eq!(
            split_separated("a\nb\nc\n", '\n'),
            vec!["a".to_string(), "b".into(), "c".into()]
        );
        assert_eq!(
            split_separated("a\nb\nc", '\n'),
            vec!["a".to_string(), "b".into(), "c".into()]
        );
    }

    #[test]
    fn small_rng_bounded_in_range() {
        let mut rng = SmallRng::seed_from_now();
        for _ in 0..1000 {
            let n = rng.next_usize_lt(7);
            assert!(n < 7);
        }
    }

    #[test]
    fn small_rng_bound_one_is_always_zero() {
        let mut rng = SmallRng::seed_from_now();
        for _ in 0..100 {
            assert_eq!(rng.next_usize_lt(1), 0);
        }
    }
}
