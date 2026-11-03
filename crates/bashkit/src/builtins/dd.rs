//! dd builtin - copy and convert bytes in blocks
//!
//! Decision: dd works on whole buffers (VFS files are whole buffers anyway).
//! `/dev/zero`, `/dev/urandom` and `/dev/random` are generated here at the
//! size the operands ask for, since the VFS serves them as bounded or absent
//! files. Every invocation moves at most `DD_MAX_BYTES`
//! (THREAT[TM-DOS-003]); an unbounded device read without `count=` stops at
//! the cap and exits 1, like dd running out of space.
//! Sync padding is admitted by checked length and live-capacity budget before
//! allocation; the lease remains with the copy buffer through output handling.

use async_trait::async_trait;

use super::limits::DD_MAX_BYTES;
use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::limits::{BudgetedBytes, ExecutionBudget};

/// `dd` builtin.
pub struct Dd;

const HELP: &str = "Usage: dd [OPERAND]...\nCopy a file, converting and formatting according to the operands.\n\n  bs=BYTES\tread and write up to BYTES bytes at a time\n  ibs=BYTES\tread up to BYTES bytes at a time (default: 512)\n  obs=BYTES\twrite BYTES bytes at a time (default: 512)\n  count=N\tcopy only N input blocks\n  skip=N\tskip N ibs-sized input blocks\n  seek=N\tskip N obs-sized output blocks\n  if=FILE\tread from FILE instead of stdin\n  of=FILE\twrite to FILE instead of stdout\n  conv=CONVS\tlcase, ucase, notrunc, sync, excl, nocreat\n  iflag=FLAGS\tcount_bytes, skip_bytes\n  oflag=FLAGS\tseek_bytes, append\n  status=LEVEL\tnone, noxfer, progress\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n";

fn parse_size(v: &str) -> Option<usize> {
    let digits_end = v.find(|c: char| !c.is_ascii_digit()).unwrap_or(v.len());
    let n: usize = v[..digits_end].parse().ok()?;
    let mult: usize = match &v[digits_end..] {
        "" | "c" => 1,
        "w" => 2,
        "b" => 512,
        "kB" => 1000,
        "K" | "k" | "KiB" => 1 << 10,
        "MB" => 1000 * 1000,
        "M" | "MiB" => 1 << 20,
        "GB" => 1000 * 1000 * 1000,
        "G" | "GiB" => 1 << 30,
        _ => return None,
    };
    n.checked_mul(mult)
}

fn human(bytes: usize) -> String {
    let b = bytes as f64;
    let si = ["kB", "MB", "GB"];
    let iec = ["KiB", "MiB", "GiB"];
    let mut i = 0;
    let (mut s, mut e) = (b / 1000.0, b / 1024.0);
    while i < 2 && s >= 1000.0 {
        s /= 1000.0;
        e /= 1024.0;
        i += 1;
    }
    let fmt = |x: f64| {
        if x < 10.0 {
            format!("{x:.1}")
        } else {
            format!("{x:.0}")
        }
    };
    format!("{} {}, {} {}", fmt(s), si[i], fmt(e), iec[i])
}

#[derive(Default)]
struct Opts {
    input: Option<String>,
    output: Option<String>,
    ibs: usize,
    obs: usize,
    count: Option<usize>,
    skip: usize,
    seek: usize,
    ucase: bool,
    lcase: bool,
    notrunc: bool,
    sync: bool,
    excl: bool,
    nocreat: bool,
    count_bytes: bool,
    skip_bytes: bool,
    seek_bytes: bool,
    append: bool,
    status: String,
}

fn parse_opts(args: &[String]) -> std::result::Result<Opts, String> {
    let mut o = Opts {
        ibs: 512,
        obs: 512,
        ..Default::default()
    };
    for arg in args {
        let Some((key, val)) = arg.split_once('=') else {
            return Err(format!("dd: unrecognized operand '{arg}'"));
        };
        let size = || parse_size(val).ok_or_else(|| format!("dd: invalid number: '{val}'"));
        match key {
            "if" => o.input = Some(val.to_string()),
            "of" => o.output = Some(val.to_string()),
            "bs" => {
                let n = size()?;
                o.ibs = n;
                o.obs = n;
            }
            "ibs" => o.ibs = size()?,
            "obs" => o.obs = size()?,
            "count" => o.count = Some(size()?),
            "skip" | "iseek" => o.skip = size()?,
            "seek" | "oseek" => o.seek = size()?,
            "status" => match val {
                "none" | "noxfer" | "progress" | "default" => o.status = val.to_string(),
                _ => return Err(format!("dd: invalid status level: '{val}'")),
            },
            "conv" | "iflag" | "oflag" => {
                for flag in val.split(',').filter(|f| !f.is_empty()) {
                    match (key, flag) {
                        ("conv", "ucase") => o.ucase = true,
                        ("conv", "lcase") => o.lcase = true,
                        ("conv", "notrunc") => o.notrunc = true,
                        ("conv", "sync") => o.sync = true,
                        ("conv", "excl") => o.excl = true,
                        ("conv", "nocreat") => o.nocreat = true,
                        ("conv", "fsync" | "fdatasync" | "noerror") => {}
                        ("iflag", "count_bytes") => o.count_bytes = true,
                        ("iflag", "skip_bytes") => o.skip_bytes = true,
                        ("oflag", "seek_bytes") => o.seek_bytes = true,
                        ("oflag", "append") => o.append = true,
                        (
                            "iflag" | "oflag",
                            "fullblock" | "direct" | "dsync" | "sync" | "nonblock" | "noatime"
                            | "nocache" | "binary" | "text",
                        ) => {}
                        _ => return Err(format!("dd: invalid {key}: '{flag}'")),
                    }
                }
            }
            _ => return Err(format!("dd: unrecognized operand '{arg}'")),
        }
    }
    if o.ibs == 0 || o.obs == 0 {
        return Err("dd: invalid number: '0'".to_string());
    }
    if o.ucase && o.lcase {
        return Err("dd: cannot combine lcase and ucase".to_string());
    }
    Ok(o)
}

// THREAT[TM-DOS-003] THREAT[TM-DOS-103]: Padding must fit both limits before growth.
fn padded_len(len: usize, ibs: usize, limit: usize) -> Option<usize> {
    let remainder = len % ibs;
    let padding = if remainder == 0 { 0 } else { ibs - remainder };
    len.checked_add(padding).filter(|&size| size <= limit)
}

fn copy_data(
    input: &[u8],
    ibs: usize,
    sync: bool,
    budget: Option<&ExecutionBudget>,
) -> Result<BudgetedBytes> {
    let len = if sync {
        padded_len(input.len(), ibs, DD_MAX_BYTES).ok_or_else(|| {
            crate::LimitExceeded::Memory("dd padded data exceeds byte limit".into())
        })?
    } else {
        input.len()
    };
    let mut buffer = BudgetedBytes::try_with_capacity(budget, len)?;
    buffer.try_extend_from_slice(input)?;
    // Every append stays within the already admitted full padded capacity.
    const ZEROS: [u8; 1024] = [0; 1024];
    while buffer.len() < len {
        let padding = (len - buffer.len()).min(ZEROS.len());
        buffer.try_extend_from_slice(&ZEROS[..padding])?;
    }
    Ok(buffer)
}

fn device_bytes(path: &str, len: usize) -> Option<std::result::Result<Vec<u8>, String>> {
    match path {
        "/dev/zero" => Some(Ok(vec![0; len])),
        "/dev/urandom" | "/dev/random" => {
            let mut buf = vec![0; len];
            Some(
                getrandom::fill(&mut buf)
                    .map(|()| buf)
                    .map_err(|_| "dd: random device unavailable".to_string()),
            )
        }
        _ => None,
    }
}

#[async_trait]
impl Builtin for Dd {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(ctx.args, HELP, Some("dd (bashkit) 0.1")) {
            return Ok(r);
        }
        let o = match parse_opts(ctx.args) {
            Ok(o) => o,
            Err(e) => return Ok(ExecResult::err(format!("{e}\n"), 1)),
        };
        let skip_bytes = if o.skip_bytes {
            o.skip
        } else {
            o.skip.saturating_mul(o.ibs)
        };
        let want = o.count.map(|c| {
            if o.count_bytes {
                c
            } else {
                c.saturating_mul(o.ibs)
            }
        });

        // Read the input.
        let mut capped = false;
        let input: Vec<u8> = match o.input.as_deref() {
            None | Some("-") | Some("/dev/stdin") => ctx.stdin_bytes().unwrap_or_default().to_vec(),
            Some(path) => {
                let device_len = want.map_or(DD_MAX_BYTES, |w| w.min(DD_MAX_BYTES));
                match device_bytes(path, skip_bytes.min(DD_MAX_BYTES) + device_len) {
                    Some(Ok(bytes)) => {
                        capped = want.is_none_or(|w| w > DD_MAX_BYTES);
                        bytes
                    }
                    Some(Err(e)) => return Ok(ExecResult::err(format!("{e}\n"), 1)),
                    None => {
                        let p = super::resolve_path(ctx.cwd, path);
                        match ctx.fs.read_file(&p).await {
                            Ok(b) => b,
                            Err(_) => {
                                return Ok(ExecResult::err(
                                    format!(
                                        "dd: failed to open '{path}': No such file or directory\n"
                                    ),
                                    1,
                                ));
                            }
                        }
                    }
                }
            }
        };
        let start = skip_bytes.min(input.len());
        let available = input.len() - start;
        let requested = want.unwrap_or(available).min(available);
        capped |= requested > DD_MAX_BYTES;
        let len = requested.min(DD_MAX_BYTES);
        let budget = ctx
            .execution_budget()
            .map(|budget| {
                budget
                    .try_with(Clone::clone)
                    .map_err(|_| crate::Error::Cancelled)
            })
            .transpose()?;
        let (full_in, partial_in) = (len / o.ibs, usize::from(!len.is_multiple_of(o.ibs)));
        let mut data = copy_data(&input[start..start + len], o.ibs, o.sync, budget.as_ref())?;
        if o.ucase {
            data.make_ascii_uppercase();
        } else if o.lcase {
            data.make_ascii_lowercase();
        }
        let (full_out, partial_out) = (
            data.len() / o.obs,
            usize::from(!data.len().is_multiple_of(o.obs)),
        );
        let copied = data.len();

        let mut result = ExecResult::ok(String::new());
        let mut _output_storage = [None, None];
        match o.output.as_deref() {
            None | Some("-") | Some("/dev/stdout") => {
                let stdout = if let Some(budget) = &budget {
                    let (stdout, storage) = crate::StreamData::from_budgeted_bytes(data, budget)?;
                    _output_storage = storage;
                    stdout
                } else {
                    crate::StreamData::from(data.into_parts().0)
                };
                result = ExecResult::ok(stdout);
            }
            Some("/dev/null") => {}
            Some(path) => {
                let p = super::resolve_path(ctx.cwd, path);
                let existing = ctx.fs.read_file(&p).await.ok();
                if o.excl && existing.is_some() {
                    return Ok(ExecResult::err(
                        format!("dd: failed to open '{path}': File exists\n"),
                        1,
                    ));
                }
                if o.nocreat && existing.is_none() {
                    return Ok(ExecResult::err(
                        format!("dd: failed to open '{path}': No such file or directory\n"),
                        1,
                    ));
                }
                let mut file = existing.unwrap_or_default();
                let offset = if o.append {
                    file.len()
                } else if o.seek_bytes {
                    o.seek
                } else {
                    o.seek.saturating_mul(o.obs)
                };
                if offset.saturating_add(data.len()) > DD_MAX_BYTES {
                    return Ok(ExecResult::err(
                        format!("dd: error writing '{path}': File too large\n"),
                        1,
                    ));
                }
                if !o.notrunc && !o.append {
                    file.truncate(offset);
                }
                if file.len() < offset {
                    file.resize(offset, 0);
                }
                let end = offset + data.len();
                if file.len() < end {
                    file.resize(end, 0);
                }
                file[offset..end].copy_from_slice(&data);
                if let Err(e) = ctx.fs.write_file(&p, &file).await {
                    return Ok(ExecResult::err(
                        format!("dd: error writing '{path}': {e}\n"),
                        1,
                    ));
                }
            }
        }

        let mut err = String::new();
        if capped {
            err.push_str(&format!(
                "dd: stopped after {DD_MAX_BYTES} bytes (bashkit dd limit)\n"
            ));
        }
        if o.status != "none" {
            err.push_str(&format!(
                "{full_in}+{partial_in} records in\n{full_out}+{partial_out} records out\n"
            ));
            if o.status != "noxfer" {
                if copied >= 1000 {
                    err.push_str(&format!(
                        "{copied} bytes ({}) copied, 0 s, 0 B/s\n",
                        human(copied)
                    ));
                } else if copied == 1 {
                    err.push_str("1 byte copied, 0 s, 0 B/s\n");
                } else {
                    err.push_str(&format!("{copied} bytes copied, 0 s, 0 B/s\n"));
                }
            }
        }
        result.stderr = err.into();
        if capped {
            result.exit_code = 1;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_budget(bytes: u64) -> ExecutionBudget {
        ExecutionBudget::new(
            &crate::ExecutionLimits::new().max_live_intermediate_bytes(bytes),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
    }

    #[test]
    fn padding_size_admission_is_checked_without_allocation() {
        assert_eq!(padded_len(0, 4, 8), Some(0));
        assert_eq!(padded_len(0, usize::MAX, 8), Some(0));
        assert_eq!(padded_len(1, usize::MAX, 8), None);
        assert_eq!(padded_len(4, 4, 4), Some(4));
        assert_eq!(padded_len(5, 4, 8), Some(8));
        assert_eq!(padded_len(5, 4, 7), None);
        assert_eq!(padded_len(usize::MAX - 1, 4, usize::MAX), None);
    }

    #[test]
    fn padded_copy_requires_live_capacity_admission() {
        let budget = tiny_budget(4);
        assert!(matches!(
            copy_data(b"x", 8, true, Some(&budget)),
            Err(crate::Error::ResourceLimit(
                crate::LimitExceeded::ExecutionBudget(
                    crate::ExecutionBudgetExceeded::LiveBytes { .. }
                )
            ))
        ));
    }

    #[test]
    fn copy_capacity_lease_survives_until_owner_drops() {
        let budget = tiny_budget(8);
        let (data, lease) = copy_data(b"ab", 4, true, Some(&budget))
            .unwrap()
            .into_parts();
        assert_eq!(data, b"ab\0\0");
        assert!(lease.is_some());
        let remaining = budget.lease_bytes(8 - data.capacity()).unwrap();
        drop(remaining);
        drop(data);
        drop(lease);
        assert!(budget.lease_bytes(8).is_ok());

        let budget = tiny_budget(4);
        let (_data, _lease) = copy_data(b"ab", 4, true, Some(&budget))
            .unwrap()
            .into_parts();
        assert!(matches!(
            budget.lease_bytes(1),
            Err(crate::LimitExceeded::ExecutionBudget(
                crate::ExecutionBudgetExceeded::LiveBytes { .. }
            ))
        ));
    }

    #[test]
    fn ordinary_copy_and_complete_blocks_do_not_pad() {
        for (input, ibs, sync, expected) in [
            (&b""[..], 8, true, &b""[..]),
            (&b"abcd"[..], 4, true, &b"abcd"[..]),
            (&b"abc"[..], 8, false, &b"abc"[..]),
            (&b"abcde"[..], 4, true, &b"abcde\0\0\0"[..]),
        ] {
            let budget = tiny_budget(8);
            let data = copy_data(input, ibs, sync, Some(&budget)).unwrap();
            assert_eq!(&*data, expected);
        }
    }

    #[test]
    fn stdout_padding_admits_cached_text() {
        for limit in [7, 8] {
            let budget = tiny_budget(limit);
            let data = copy_data(b"ab", 4, true, Some(&budget)).unwrap();
            let result = crate::StreamData::from_budgeted_bytes(data, &budget);
            if limit == 7 {
                assert!(matches!(
                    result,
                    Err(crate::LimitExceeded::ExecutionBudget(
                        crate::ExecutionBudgetExceeded::LiveBytes { .. }
                    ))
                ));
            } else {
                let (stream, _storage) = result.unwrap();
                assert_eq!(stream.as_bytes(), b"ab\0\0");
                assert_eq!(stream.text_lossy(), "ab\0\0");
                assert!(budget.lease_bytes(1).is_err());
            }
        }
    }

    #[test]
    fn parses_sizes_like_gnu() {
        assert_eq!(parse_size("512"), Some(512));
        assert_eq!(parse_size("2b"), Some(1024));
        assert_eq!(parse_size("1K"), Some(1024));
        assert_eq!(parse_size("1kB"), Some(1000));
        assert_eq!(parse_size("1M"), Some(1 << 20));
        assert_eq!(parse_size("x"), None);
        assert_eq!(parse_size("1Q"), None);
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human(1_048_576), "1.0 MB, 1.0 MiB");
        assert_eq!(human(10_000), "10 kB, 9.8 KiB");
    }

    #[test]
    fn rejects_bad_operands() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(parse_opts(&args(&["bs=0"])).is_err());
        assert!(parse_opts(&args(&["conv=ucase,lcase"])).is_err());
        assert!(parse_opts(&args(&["foo"])).is_err());
        assert!(parse_opts(&args(&["conv=bogus"])).is_err());
    }
}
