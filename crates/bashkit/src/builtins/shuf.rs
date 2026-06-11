//! shuf builtin.
//!
//! Argument surface is generated from uutils/coreutils' `uu_app()` via the
//! `bashkit-coreutils-port` codegen tool — see `generated/shuf_args.rs`
//! and `crates/bashkit-coreutils-port/`. Behaviour is implemented locally
//! against the bashkit VFS.
//!
//! Resource decision: shuf builds an ExecResult/String in-process, so it must
//! reject outputs that exceed ExecutionLimits before allocation and must never
//! materialize numeric ranges only to apply `-n` afterward.

use async_trait::async_trait;
use std::collections::HashMap;
use std::ffi::OsString;
use std::ops::RangeInclusive;
use std::path::Path;

use super::generated::shuf_args::shuf_command;
use super::{Builtin, Context, read_text_file};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::limits::ExecutionLimits;

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

        let input = if echo {
            ShufInput::Items(positionals)
        } else if let Some(range) = input_range {
            ShufInput::Range(range)
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
            ShufInput::Items(split_separated(&raw, separator))
        };

        // THREAT[TM-DOS-090]: Bind shuf's in-process output construction to
        // ExecutionLimits before range/repeat loops can allocate.
        let output_limit = shuf_output_limit(&ctx, output_path.is_some());
        let mut rng = SmallRng::seed_from_now();
        let out = match if repeat {
            render_repeat(input, head_count, separator, output_limit, &mut rng)
        } else {
            render_non_repeat(input, head_count, separator, output_limit, &mut rng)
        } {
            Ok(out) => out,
            Err(stderr) => return Ok(ExecResult::err(stderr, 1)),
        };

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

enum ShufInput {
    Items(Vec<String>),
    Range(RangeInclusive<u64>),
}

fn shuf_output_limit(ctx: &Context<'_>, output_to_file: bool) -> usize {
    let exec_limit = ctx
        .execution_extension::<ExecutionLimits>()
        .map(|limits| limits.max_stdout_bytes)
        .unwrap_or_else(|| ExecutionLimits::default().max_stdout_bytes);

    if !output_to_file {
        return exec_limit;
    }

    let fs_limits = ctx.fs.limits();
    exec_limit
        .min(u64_to_usize_saturating(fs_limits.max_file_size))
        .min(u64_to_usize_saturating(fs_limits.max_total_bytes))
}

fn render_repeat(
    input: ShufInput,
    head_count: Option<u64>,
    separator: char,
    output_limit: usize,
    rng: &mut SmallRng,
) -> std::result::Result<String, String> {
    // -r samples *with* replacement: each pick is independent.
    // GNU shuf -r without -n loops forever; bashkit requires -n because
    // an embedded VFS shell needs a finite output contract.
    let count = match head_count {
        Some(n) => n,
        None => {
            return Err("shuf: --repeat requires --head-count to be finite\n".to_string());
        }
    };

    match input {
        ShufInput::Items(items) => {
            if items.is_empty() {
                return Err("shuf: no lines to repeat from\n".to_string());
            }
            let max_line_len = items
                .iter()
                .map(String::len)
                .max()
                .unwrap_or(0)
                .saturating_add(separator.len_utf8());
            ensure_output_fits(count as u128, max_line_len as u128, output_limit)?;

            let mut out =
                String::with_capacity(repeat_capacity(count, max_line_len, output_limit)?);
            for _ in 0..count {
                out.push_str(&items[rng.next_usize_lt(items.len())]);
                out.push(separator);
            }
            Ok(out)
        }
        ShufInput::Range(range) => {
            let Some((start, end, len)) = range_parts(&range) else {
                return Err("shuf: no lines to repeat from\n".to_string());
            };
            let max_line_len = max_range_value_len(start, end).saturating_add(separator.len_utf8());
            ensure_output_fits(count as u128, max_line_len as u128, output_limit)?;

            let mut out =
                String::with_capacity(repeat_capacity(count, max_line_len, output_limit)?);
            for _ in 0..count {
                let offset = rng.next_u128_lt(len);
                out.push_str(&(start as u128 + offset).to_string());
                out.push(separator);
            }
            Ok(out)
        }
    }
}

fn render_non_repeat(
    input: ShufInput,
    head_count: Option<u64>,
    separator: char,
    output_limit: usize,
    rng: &mut SmallRng,
) -> std::result::Result<String, String> {
    match input {
        ShufInput::Items(mut items) => {
            for i in (1..items.len()).rev() {
                let j = rng.next_usize_lt(i + 1);
                items.swap(i, j);
            }
            if let Some(n) = head_count {
                items.truncate(u64_to_usize_saturating(n));
            }
            render_items(items, separator, output_limit)
        }
        ShufInput::Range(range) => {
            let Some((start, end, len)) = range_parts(&range) else {
                return Ok(String::new());
            };
            let output_count = head_count.map(u128::from).unwrap_or(len).min(len);
            let max_line_len = max_range_value_len(start, end).saturating_add(separator.len_utf8());
            ensure_output_fits(output_count, max_line_len as u128, output_limit)?;

            let count =
                usize::try_from(output_count).map_err(|_| output_too_large(output_limit))?;
            let values = sample_range_without_replacement(start, len, count, rng);
            render_items(values, separator, output_limit)
        }
    }
}

fn render_items(
    items: Vec<String>,
    separator: char,
    output_limit: usize,
) -> std::result::Result<String, String> {
    let output_len =
        items_output_len(&items, separator).ok_or_else(|| output_too_large(output_limit))?;
    if output_len > output_limit {
        return Err(output_too_large(output_limit));
    }

    let mut out = String::with_capacity(output_len);
    for line in &items {
        out.push_str(line);
        out.push(separator);
    }
    Ok(out)
}

fn sample_range_without_replacement(
    start: u64,
    len: u128,
    count: usize,
    rng: &mut SmallRng,
) -> Vec<String> {
    let mut swaps: HashMap<u128, u128> = HashMap::with_capacity(count.saturating_mul(2));
    let mut out = Vec::with_capacity(count);
    for i in 0..count as u128 {
        let j = i + rng.next_u128_lt(len - i);
        let selected = *swaps.get(&j).unwrap_or(&j);
        let replacement = *swaps.get(&i).unwrap_or(&i);
        swaps.insert(j, replacement);
        out.push((start as u128 + selected).to_string());
    }
    out
}

fn range_parts(range: &RangeInclusive<u64>) -> Option<(u64, u64, u128)> {
    let start = *range.start();
    let end = *range.end();
    if start > end {
        return None;
    }
    Some((start, end, u128::from(end - start) + 1))
}

fn items_output_len(items: &[String], separator: char) -> Option<usize> {
    let sep_len = separator.len_utf8();
    items.iter().try_fold(0usize, |sum, item| {
        sum.checked_add(item.len())?.checked_add(sep_len)
    })
}

fn ensure_output_fits(
    count: u128,
    max_line_len: u128,
    output_limit: usize,
) -> std::result::Result<(), String> {
    let Some(max_output_len) = count.checked_mul(max_line_len) else {
        return Err(output_too_large(output_limit));
    };
    if max_output_len > output_limit as u128 {
        return Err(output_too_large(output_limit));
    }
    Ok(())
}

fn repeat_capacity(
    count: u64,
    max_line_len: usize,
    output_limit: usize,
) -> std::result::Result<usize, String> {
    let capacity = (count as u128)
        .checked_mul(max_line_len as u128)
        .ok_or_else(|| output_too_large(output_limit))?;
    usize::try_from(capacity).map_err(|_| output_too_large(output_limit))
}

fn max_range_value_len(start: u64, end: u64) -> usize {
    start.to_string().len().max(end.to_string().len())
}

fn u64_to_usize_saturating(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

fn output_too_large(output_limit: usize) -> String {
    format!("shuf: output too large (limit {output_limit} bytes)\n")
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
        let nanos = crate::time::SystemTime::now()
            .duration_since(crate::time::UNIX_EPOCH)
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

    fn next_u128_lt(&mut self, bound: u128) -> u128 {
        debug_assert!(bound > 0);
        loop {
            let value = (u128::from(self.next_u64()) << 64) | u128::from(self.next_u64());
            let zone = u128::MAX - (u128::MAX % bound);
            if value < zone {
                return value % bound;
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
