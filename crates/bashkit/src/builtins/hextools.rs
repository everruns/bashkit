//! od, xxd, and hexdump builtins - byte-level inspection tools
//!
//! Decision: od caps row width before layout arithmetic, pads only one field
//! on the stack, and admits output growth through the shared request budget.

use super::clap_cache::cached_command;
use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;
use crate::limits::{BudgetedString, ExecutionBudget};

/// The od builtin - dump files in octal and other formats.
///
/// Argument surface is generated from uutils/coreutils' `uu_app()` via
/// the `bashkit-coreutils-port` codegen tool — see
/// `generated/od_args.rs`. Behaviour is implemented locally against
/// the bashkit VFS: `-A RADIX`, repeated `-t TYPE` (a, c, d, o, u, x with
/// sizes and the `z` trailer), the integer/char shorthands, `-N`, `-j`,
/// `-w`, `-v` and `--endian`. Floating-point types are rejected.
pub struct Od;

/// The xxd builtin - make a hexdump or do the reverse.
///
/// Usage: xxd [-l LEN] [-s OFFSET] [-c COLS] [-g GROUP] [-p] [-r] [FILE...]
///
/// Options:
///   -l LEN     Stop after LEN bytes
///   -s OFFSET  Start at OFFSET bytes
///   -c COLS    Bytes per line (default: 16)
///   -g GROUP   Bytes per group (default: 2)
///   -p         Plain hex dump (no offsets, no ASCII)
///   -r         Reverse: convert hexdump back to binary (not implemented)
pub struct Xxd;

/// The hexdump builtin - display file contents in hex.
///
/// Usage: hexdump [-C] [-n LENGTH] [-s OFFSET] [FILE...]
///
/// Options:
///   -C         Canonical hex+ASCII display
///   -n LENGTH  Interpret only LENGTH bytes
///   -s OFFSET  Skip OFFSET bytes from beginning
pub struct Hexdump;

// --- Od implementation ---
//
// Decision: a port of GNU od's block layout. Every `-t` spec (and shorthand
// such as `-x` = `-t x2`) prints its own line per block in command-line
// order; fields share GNU's width-per-block padding so specs stay aligned; a
// short final block is zero-filled; `z` appends a `>...<` trailer; repeated
// full blocks collapse to `*` unless `-v`. Floating-point types (`f`, `-e`,
// `-f`, `-F`) are rejected with an explicit error (not yet ported).

// THREAT[TM-DOS-103]: Bound caller-selected dimensions even without a budget.
const OD_MAX_WIDTH: usize = 65_536;

struct OdOptions {
    addr_radix: AddrRadix,
    specs: Vec<OdSpec>,
    count: Option<usize>,
    skip: usize,
    width: Option<usize>,
    big_endian: bool,
    show_duplicates: bool,
}

#[derive(Clone, Copy)]
enum AddrRadix {
    Octal,
    Decimal,
    Hex,
    None,
}

#[derive(Clone, Copy, PartialEq)]
enum OdKind {
    Named,
    Char,
    Signed,
    Unsigned,
    Octal,
    Hex,
}

#[derive(Clone, Copy)]
struct OdSpec {
    kind: OdKind,
    size: usize,
    trailer: bool,
}

impl OdSpec {
    /// GNU field width: the widest value of this type, without the separator.
    fn field_width(&self) -> usize {
        let idx = match self.size {
            1 => 0,
            2 => 1,
            4 => 2,
            _ => 3,
        };
        match self.kind {
            OdKind::Named | OdKind::Char => 3,
            OdKind::Signed => [4, 6, 11, 20][idx],
            OdKind::Unsigned => [3, 5, 10, 20][idx],
            OdKind::Octal => [3, 6, 11, 22][idx],
            OdKind::Hex => [2, 4, 8, 16][idx],
        }
    }
}

/// Parse one `-t` TYPE string, which may hold several specs (`-t ox1z`).
fn parse_od_type(s: &str) -> std::result::Result<Vec<OdSpec>, String> {
    let b = s.as_bytes();
    let mut specs = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        i += 1;
        let (kind, size) = match c {
            b'a' => (OdKind::Named, 1),
            b'c' => (OdKind::Char, 1),
            b'd' | b'o' | b'u' | b'x' => {
                let kind = match c {
                    b'd' => OdKind::Signed,
                    b'o' => OdKind::Octal,
                    b'u' => OdKind::Unsigned,
                    _ => OdKind::Hex,
                };
                let size = match b.get(i) {
                    Some(b'C') => {
                        i += 1;
                        1
                    }
                    Some(b'S') => {
                        i += 1;
                        2
                    }
                    Some(b'I') => {
                        i += 1;
                        4
                    }
                    Some(b'L') => {
                        i += 1;
                        8
                    }
                    Some(d) if d.is_ascii_digit() => {
                        let n = b[i..].iter().take_while(|d| d.is_ascii_digit()).count();
                        let size: usize = s[i..i + n].parse().unwrap_or(0);
                        i += n;
                        size
                    }
                    _ => 4,
                };
                if !matches!(size, 1 | 2 | 4 | 8) {
                    return Err(format!(
                        "od: invalid type string '{s}';\nthis system doesn't provide a {size}-byte integral type"
                    ));
                }
                (kind, size)
            }
            b'f' => {
                return Err(format!(
                    "od: floating-point type string '{s}' is not supported in bashkit"
                ));
            }
            _ => {
                return Err(format!(
                    "od: invalid character '{}' in type string '{s}'",
                    c as char
                ));
            }
        };
        let trailer = b.get(i) == Some(&b'z');
        if trailer {
            i += 1;
        }
        specs.push(OdSpec {
            kind,
            size,
            trailer,
        });
    }
    if specs.is_empty() {
        return Err(format!("od: invalid type string '{s}'"));
    }
    Ok(specs)
}

/// GNU od number operand: `0x` hex, leading-`0` octal, or decimal, with an
/// optional `b`/`k`/`m` multiplier.
fn parse_od_num(s: &str, what: &str) -> std::result::Result<usize, String> {
    let err = || format!("od: invalid {what} argument '{s}'");
    let (digits, mult) = match s.as_bytes().last() {
        Some(b'b') if !s.starts_with("0x") && !s.starts_with("0X") => (&s[..s.len() - 1], 512),
        Some(b'k' | b'K') => (&s[..s.len() - 1], 1024),
        Some(b'm' | b'M') => (&s[..s.len() - 1], 1024 * 1024),
        _ => (s, 1),
    };
    let value = if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        usize::from_str_radix(hex, 16).map_err(|_| err())?
    } else if digits.len() > 1 && digits.starts_with('0') {
        usize::from_str_radix(&digits[1..], 8).map_err(|_| err())?
    } else {
        digits.parse::<usize>().map_err(|_| err())?
    };
    value.checked_mul(mult).ok_or_else(err)
}

/// Translate clap-parsed `od` matches into the local rendering struct.
fn od_options_from_matches(
    matches: &clap::ArgMatches,
) -> std::result::Result<(OdOptions, Vec<String>), String> {
    let mut opts = OdOptions {
        addr_radix: AddrRadix::Octal,
        specs: Vec::new(),
        count: None,
        skip: 0,
        width: None,
        big_endian: false,
        show_duplicates: matches.get_flag("output-duplicates"),
    };

    if let Some(radix) = matches.get_one::<String>("address-radix") {
        opts.addr_radix = match radix.as_str() {
            "d" => AddrRadix::Decimal,
            "o" => AddrRadix::Octal,
            "x" => AddrRadix::Hex,
            "n" => AddrRadix::None,
            other => return Err(format!("od: invalid address radix: '{}'", other)),
        };
    }

    // Specs apply in command-line order, so collect (index, TYPE) pairs from
    // both `-t` and the single-letter shorthands, then sort.
    let mut ordered: Vec<(usize, String)> = Vec::new();
    if let (Some(idx), Some(vals)) = (
        matches.indices_of("format"),
        matches.get_many::<String>("format"),
    ) {
        ordered.extend(idx.zip(vals.cloned()));
    }
    const SHORTHANDS: &[(&str, &str)] = &[
        ("a", "a"),
        ("b", "o1"),
        ("c", "c"),
        ("d", "u2"),
        ("D", "u4"),
        ("o", "o2"),
        ("I", "dL"),
        ("L", "dL"),
        ("i", "dI"),
        ("l", "dL"),
        ("x", "x2"),
        ("h", "x2"),
        ("O", "o4"),
        ("s", "d2"),
        ("X", "x4"),
        ("H", "x4"),
        ("e", "fD"),
        ("f", "fF"),
        ("F", "fD"),
    ];
    for (id, ty) in SHORTHANDS {
        if matches.try_get_one::<bool>(id).ok().flatten().copied() == Some(true)
            && let Some(idx) = matches.index_of(id)
        {
            ordered.push((idx, (*ty).to_string()));
        }
    }
    ordered.sort_by_key(|(idx, _)| *idx);
    for (_, ty) in &ordered {
        opts.specs.extend(parse_od_type(ty)?);
    }
    if opts.specs.is_empty() {
        opts.specs.push(OdSpec {
            kind: OdKind::Octal,
            size: 2,
            trailer: false,
        });
    }

    if let Some(s) = matches.get_one::<String>("read-bytes") {
        opts.count = Some(parse_od_num(s, "-N")?);
    }
    if let Some(s) = matches.get_one::<String>("skip-bytes") {
        opts.skip = parse_od_num(s, "-j")?;
    }
    if let Some(s) = matches.get_one::<String>("width") {
        let width = parse_od_num(s, "-w")?;
        if width > OD_MAX_WIDTH {
            return Err(format!("od: width exceeds maximum of {OD_MAX_WIDTH} bytes"));
        }
        opts.width = Some(width);
    }
    if let Some(e) = matches.get_one::<String>("endian") {
        opts.big_endian = e == "big";
    }

    let files: Vec<String> = matches
        .get_many::<String>("FILENAME")
        .map(|vs| vs.cloned().collect())
        .unwrap_or_default();

    Ok((opts, files))
}

fn format_od_addr(offset: usize, radix: AddrRadix) -> String {
    match radix {
        AddrRadix::Octal => format!("{:07o}", offset),
        AddrRadix::Decimal => format!("{:07}", offset),
        AddrRadix::Hex => format!("{:06x}", offset),
        AddrRadix::None => String::new(),
    }
}

const OD_NAMES: [&str; 33] = [
    "nul", "soh", "stx", "etx", "eot", "enq", "ack", "bel", "bs", "ht", "nl", "vt", "ff", "cr",
    "so", "si", "dle", "dc1", "dc2", "dc3", "dc4", "nak", "syn", "etb", "can", "em", "sub", "esc",
    "fs", "gs", "rs", "us", "sp",
];

fn format_od_field(spec: &OdSpec, bytes: &[u8], big_endian: bool) -> String {
    match spec.kind {
        OdKind::Named => {
            let b = bytes[0] & 0x7f;
            match b {
                0..=32 => OD_NAMES[b as usize].to_string(),
                127 => "del".to_string(),
                _ => (b as char).to_string(),
            }
        }
        OdKind::Char => match bytes[0] {
            0 => "\\0".to_string(),
            7 => "\\a".to_string(),
            8 => "\\b".to_string(),
            9 => "\\t".to_string(),
            10 => "\\n".to_string(),
            11 => "\\v".to_string(),
            12 => "\\f".to_string(),
            13 => "\\r".to_string(),
            b @ 0x20..=0x7e => (b as char).to_string(),
            b => format!("{:03o}", b),
        },
        _ => {
            let mut v: u64 = 0;
            if big_endian {
                for &b in bytes {
                    v = (v << 8) | u64::from(b);
                }
            } else {
                for &b in bytes.iter().rev() {
                    v = (v << 8) | u64::from(b);
                }
            }
            let w = spec.field_width();
            match spec.kind {
                OdKind::Octal => format!("{:0w$o}", v),
                OdKind::Hex => format!("{:0w$x}", v),
                OdKind::Unsigned => v.to_string(),
                _ => {
                    let shift = 64 - 8 * spec.size as u32;
                    (((v << shift) as i64) >> shift).to_string()
                }
            }
        }
    }
}

// Avoid allocating padding or formatted rows outside the budgeted owner.
fn od_spaces(output: &mut BudgetedString, mut count: usize) -> Result<()> {
    const SPACES: &str = "                                                                ";
    while count > 0 {
        let n = count.min(SPACES.len());
        output.try_push_str(&SPACES[..n])?;
        count -= n;
    }
    Ok(())
}

fn od_dump(data: &[u8], opts: &OdOptions, budget: Option<&ExecutionBudget>) -> Result<String> {
    let lcm = opts.specs.iter().fold(1usize, |acc, s| {
        let (mut a, mut b) = (acc, s.size);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        acc / a * s.size
    });
    let bpb = match opts.width {
        Some(w) if w > 0 && w % lcm == 0 => w,
        Some(_) => lcm,
        None => lcm * (16 / lcm).max(1),
    };
    let width_per_block = opts
        .specs
        .iter()
        .map(|s| (s.field_width() + 1) * (bpb / s.size))
        .max()
        .unwrap_or(0);
    let addr_pad = format_od_addr(0, opts.addr_radix).len();

    let data = data.get(opts.skip..).unwrap_or(&[]);
    let data = match opts.count {
        Some(n) => &data[..data.len().min(n)],
        None => data,
    };

    // THREAT[TM-DOS-103]: lease capacity before fallible allocation.
    let mut output = BudgetedString::new(budget)?;
    let mut prev: Option<&[u8]> = None;
    let mut in_dup_run = false;
    for (chunk_idx, chunk) in data.chunks(bpb).enumerate() {
        if let Some(budget) = budget {
            budget.consume_work(chunk.len() as u64)?;
        }
        let offset = opts.skip + chunk_idx * bpb;
        if !opts.show_duplicates && chunk.len() == bpb && prev == Some(chunk) {
            if !in_dup_run {
                output.try_push_str("*\n")?;
                in_dup_run = true;
            }
            continue;
        }
        in_dup_run = false;
        prev = Some(chunk);

        for (si, spec) in opts.specs.iter().enumerate() {
            if si == 0 {
                output.try_push_str(&format_od_addr(offset, opts.addr_radix))?;
            } else {
                od_spaces(&mut output, addr_pad)?;
            }
            let fields = bpb / spec.size;
            let blank = (bpb - chunk.len()) / spec.size;
            let width = spec.field_width();
            let pad = width_per_block - width * fields;
            let mut pad_remaining = pad;
            for i in (blank + 1..=fields).rev() {
                let idx = fields - i;
                // Width is capped; u64 also keeps the product safe on 32-bit hosts.
                let next_pad = (pad as u64 * (i - 1) as u64 / fields as u64) as usize;
                let adjusted = pad_remaining - next_pad + width;
                if let Some(budget) = budget {
                    budget.consume_work(1)?;
                }
                // Only an incomplete numeric field needs zero padding (max 8 bytes).
                let mut field = [0u8; 8];
                let start = idx * spec.size;
                let available = spec.size.min(chunk.len() - start);
                field[..available].copy_from_slice(&chunk[start..start + available]);
                let text = format_od_field(spec, &field[..spec.size], opts.big_endian);
                od_spaces(&mut output, adjusted.saturating_sub(text.len()))?;
                output.try_push_str(&text)?;
                pad_remaining = next_pad;
            }
            if spec.trailer {
                let trailer_pad =
                    blank * width + (pad as u64 * blank as u64 / fields as u64) as usize;
                od_spaces(&mut output, trailer_pad)?;
                output.try_push_str("  >")?;
                for &b in chunk {
                    output.try_push(if (0x20..0x7f).contains(&b) {
                        b as char
                    } else {
                        '.'
                    })?;
                }
                output.try_push('<')?;
            }
            output.try_push('\n')?;
        }
    }

    // GNU always ends with the final address (nothing for `-An`).
    let addr = format_od_addr(opts.skip + data.len(), opts.addr_radix);
    if !addr.is_empty() {
        output.try_push_str(&addr)?;
        output.try_push('\n')?;
    }

    Ok(output.into_inner())
}

// Cached `od` arg surface: pre-built once, cloned per invocation.
// See `builtins::clap_cache` for why it is built, not just constructed.
cached_command!(od_cmd, super::generated::od_args::od_command());

#[async_trait]
impl Builtin for Od {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        use std::ffi::OsString;

        let argv: Vec<OsString> = std::iter::once(OsString::from("od"))
            .chain(ctx.args.iter().map(OsString::from))
            .collect();

        let matches = match od_cmd().try_get_matches_from(argv) {
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

        let (opts, files) = match od_options_from_matches(&matches) {
            Ok(v) => v,
            Err(e) => return Ok(ExecResult::err(format!("{}\n", e), 1)),
        };

        let data = match collect_input("od", ctx.stdin, &files, ctx.cwd, &ctx.fs).await {
            Ok(data) => data,
            Err(failure) => return Ok(failure),
        };
        let budget = ctx
            .execution_budget()
            .map(|budget| {
                budget
                    .try_with(Clone::clone)
                    .map_err(|_| crate::Error::Cancelled)
            })
            .transpose()?;
        let output = od_dump(&data, &opts, budget.as_ref())?;

        Ok(ExecResult::ok(output))
    }
}

// --- Xxd implementation ---

struct XxdOptions {
    length: Option<usize>,
    offset: usize,
    cols: usize,
    group: usize,
    plain: bool,
    reverse: bool,
}

fn parse_xxd_args(args: &[String]) -> std::result::Result<(XxdOptions, Vec<String>), String> {
    let mut opts = XxdOptions {
        length: None,
        offset: 0,
        cols: 16,
        group: 2,
        plain: false,
        reverse: false,
    };
    let mut files = Vec::new();
    let mut p = super::arg_parser::ArgParser::new(args);

    while !p.is_done() {
        if let Some(val) = p.flag_value("-l", "xxd")? {
            opts.length = Some(
                val.parse()
                    .map_err(|_| format!("xxd: invalid length: '{}'", val))?,
            );
        } else if let Some(val) = p.flag_value("-s", "xxd")? {
            opts.offset = val
                .parse()
                .map_err(|_| format!("xxd: invalid offset: '{}'", val))?;
        } else if let Some(val) = p.flag_value("-c", "xxd")? {
            opts.cols = val
                .parse()
                .map_err(|_| format!("xxd: invalid cols: '{}'", val))?;
            if opts.cols == 0 {
                opts.cols = 16;
            }
        } else if let Some(val) = p.flag_value("-g", "xxd")? {
            opts.group = val
                .parse()
                .map_err(|_| format!("xxd: invalid group: '{}'", val))?;
        } else if p.flag("-p") {
            opts.plain = true;
        } else if p.flag("-r") {
            opts.reverse = true;
        } else if p.is_flag() && p.current() != Some("--") {
            // Reject unknown options instead of treating them as filenames.
            return Err(invalid_option_msg("xxd", p.current().unwrap_or_default()));
        } else if let Some(arg) = p.positional() {
            files.push(arg.to_string());
        }
    }

    Ok((opts, files))
}

/// Build an unrecognized-option message (no trailing newline; callers append
/// it via `format!("{}\n", e)`). Mirrors `super::invalid_option`.
fn invalid_option_msg(cmd: &str, arg: &str) -> String {
    if let Some(long) = arg.strip_prefix("--") {
        format!("{cmd}: unrecognized option '--{long}'")
    } else {
        let ch = arg
            .strip_prefix('-')
            .and_then(|s| s.chars().next())
            .unwrap_or('-');
        format!("{cmd}: invalid option -- '{ch}'")
    }
}

fn xxd_dump(data: &[u8], opts: &XxdOptions) -> String {
    let mut output = String::new();

    let data = if opts.offset < data.len() {
        &data[opts.offset..]
    } else {
        &[]
    };

    let data = match opts.length {
        Some(n) => &data[..data.len().min(n)],
        None => data,
    };

    if opts.plain {
        for byte in data {
            output.push_str(&format!("{:02x}", byte));
        }
        if !data.is_empty() {
            output.push('\n');
        }
        return output;
    }

    for (chunk_idx, chunk) in data.chunks(opts.cols).enumerate() {
        let offset = opts.offset + chunk_idx * opts.cols;

        // Offset
        output.push_str(&format!("{:08x}: ", offset));

        // Hex bytes with grouping
        for (j, byte) in chunk.iter().enumerate() {
            if j > 0 && opts.group > 0 && j % opts.group == 0 {
                output.push(' ');
            }
            output.push_str(&format!("{:02x}", byte));
        }

        // Padding for short lines
        let missing = opts.cols - chunk.len();
        for k in 0..missing {
            if (chunk.len() + k) > 0 && opts.group > 0 && (chunk.len() + k) % opts.group == 0 {
                output.push(' ');
            }
            output.push_str("  ");
        }

        // ASCII representation
        output.push_str("  ");
        for byte in chunk {
            if *byte >= 0x20 && *byte < 0x7f {
                output.push(*byte as char);
            } else {
                output.push('.');
            }
        }

        output.push('\n');
    }

    output
}

/// Decode a string of hex digit characters into bytes.
/// Non-hex characters are silently ignored. Odd trailing nibble is dropped.
fn decode_hex(hex: &str) -> Vec<u8> {
    let clean: String = hex.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    clean
        .as_bytes()
        .chunks(2)
        .filter_map(|pair| {
            if pair.len() == 2 {
                u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()
            } else {
                None
            }
        })
        .collect()
}

/// Reverse hex dump: convert hex string back to binary bytes.
/// In plain mode (-r -p), treats input as a continuous hex stream.
/// In normal mode (-r), parses xxd-style output (skips address and ASCII columns).
fn xxd_reverse(data: &[u8], plain: bool) -> Vec<u8> {
    let text = String::from_utf8_lossy(data);

    if plain {
        return decode_hex(&text);
    }

    // Normal xxd output: "ADDR: HH HH ...  ASCII"
    let mut result = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Strip address prefix (before colon)
        let hex_part = match line.find(':') {
            Some(idx) => &line[idx + 1..],
            None => line,
        };
        // Strip ASCII column (after double space)
        let hex_part = match hex_part.find("  ") {
            Some(idx) => &hex_part[..idx],
            None => hex_part,
        };
        result.extend(decode_hex(hex_part));
    }
    result
}

#[async_trait]
impl Builtin for Xxd {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: xxd [OPTIONS] [FILE]\nMake a hexdump or do the reverse.\n\n  -l LEN\tstop after LEN bytes\n  -s OFFSET\tstart at OFFSET bytes\n  -c COLS\tbytes per line (default: 16)\n  -g GROUP\tbytes per group (default: 2)\n  -p\tplain hex dump (no offsets, no ASCII)\n  -r\treverse: convert hexdump back to binary\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("xxd (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let (opts, files) = match parse_xxd_args(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(ExecResult::err(format!("{}\n", e), 1)),
        };

        let data = match collect_input("xxd", ctx.stdin, &files, ctx.cwd, &ctx.fs).await {
            Ok(data) => data,
            Err(failure) => return Ok(failure),
        };

        if opts.reverse {
            let bytes = xxd_reverse(&data, opts.plain);
            // Output raw bytes as lossy UTF-8
            let output = String::from_utf8_lossy(&bytes).to_string();
            Ok(ExecResult::ok(output))
        } else {
            let output = xxd_dump(&data, &opts);
            Ok(ExecResult::ok(output))
        }
    }
}

// --- Hexdump implementation ---

struct HexdumpOptions {
    canonical: bool,
    length: Option<usize>,
    offset: usize,
}

fn parse_hexdump_args(
    args: &[String],
) -> std::result::Result<(HexdumpOptions, Vec<String>), String> {
    let mut opts = HexdumpOptions {
        canonical: false,
        length: None,
        offset: 0,
    };
    let mut files = Vec::new();
    let mut p = super::arg_parser::ArgParser::new(args);

    while !p.is_done() {
        if p.flag("-C") {
            opts.canonical = true;
        } else if let Some(val) = p.flag_value("-n", "hexdump")? {
            opts.length = Some(
                val.parse()
                    .map_err(|_| format!("hexdump: invalid length: '{}'", val))?,
            );
        } else if let Some(val) = p.flag_value("-s", "hexdump")? {
            opts.offset = val
                .parse()
                .map_err(|_| format!("hexdump: invalid offset: '{}'", val))?;
        } else if p.is_flag() && p.current() != Some("--") {
            // Reject unknown options instead of treating them as filenames.
            return Err(invalid_option_msg(
                "hexdump",
                p.current().unwrap_or_default(),
            ));
        } else if let Some(arg) = p.positional() {
            files.push(arg.to_string());
        }
    }

    Ok((opts, files))
}

fn hexdump_dump(data: &[u8], opts: &HexdumpOptions) -> String {
    let mut output = String::new();

    let data = if opts.offset < data.len() {
        &data[opts.offset..]
    } else {
        &[]
    };

    let data = match opts.length {
        Some(n) => &data[..data.len().min(n)],
        None => data,
    };

    if opts.canonical {
        // -C mode: hex+ASCII like `hexdump -C`
        for (chunk_idx, chunk) in data.chunks(16).enumerate() {
            let offset = opts.offset + chunk_idx * 16;
            output.push_str(&format!("{:08x}  ", offset));

            // First 8 bytes
            for j in 0..8 {
                if j < chunk.len() {
                    output.push_str(&format!("{:02x} ", chunk[j]));
                } else {
                    output.push_str("   ");
                }
            }
            output.push(' ');

            // Next 8 bytes
            for j in 8..16 {
                if j < chunk.len() {
                    output.push_str(&format!("{:02x} ", chunk[j]));
                } else {
                    output.push_str("   ");
                }
            }

            // ASCII
            output.push_str(" |");
            for byte in chunk {
                if *byte >= 0x20 && *byte < 0x7f {
                    output.push(*byte as char);
                } else {
                    output.push('.');
                }
            }
            output.push_str("|\n");
        }

        // Final offset
        if !data.is_empty() {
            let final_offset = opts.offset + data.len();
            output.push_str(&format!("{:08x}\n", final_offset));
        }
    } else {
        // Default mode: 16-bit hex words
        for (chunk_idx, chunk) in data.chunks(16).enumerate() {
            let offset = opts.offset + chunk_idx * 16;
            output.push_str(&format!("{:07x}", offset));

            for pair in chunk.chunks(2) {
                if pair.len() == 2 {
                    // Little-endian 16-bit word
                    let word = (pair[1] as u16) << 8 | pair[0] as u16;
                    output.push_str(&format!(" {:04x}", word));
                } else {
                    output.push_str(&format!(" {:04x}", pair[0] as u16));
                }
            }
            output.push('\n');
        }

        // Final offset
        if !data.is_empty() {
            let final_offset = opts.offset + data.len();
            output.push_str(&format!("{:07x}\n", final_offset));
        }
    }

    output
}

#[async_trait]
impl Builtin for Hexdump {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: hexdump [OPTION]... [FILE]...\nDisplay file contents in hexadecimal.\n\n  -C\tcanonical hex+ASCII display\n  -n LENGTH\tinterpret only LENGTH bytes\n  -s OFFSET\tskip OFFSET bytes from beginning\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("hexdump (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let (opts, files) = match parse_hexdump_args(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(ExecResult::err(format!("{}\n", e), 1)),
        };

        let data = match collect_input("hexdump", ctx.stdin, &files, ctx.cwd, &ctx.fs).await {
            Ok(data) => data,
            Err(failure) => return Ok(failure),
        };
        let output = hexdump_dump(&data, &opts);

        Ok(ExecResult::ok(output))
    }
}

// --- Shared helpers ---

/// Read the operand files for `od` / `xxd` / `hexdump`.
///
/// An unreadable operand is an ordinary command failure, not an interpreter
/// error: real `od` prints `od: FILE: No such file or directory`, exits 1 and
/// lets the rest of the script run. Wrapping the VFS error in
/// `Error::Internal` instead aborted the whole script with
/// `internal error: FILE: io error: file not found` — two Rust enum shapes on
/// a path any typo reaches, which is the TM-INF-022 leak nightly `glob_fuzz`
/// run 228 caught. Hence `Err(ExecResult)`: the failure is the command's own
/// output, so the caller returns it as `Ok`.
async fn collect_input(
    name: &str,
    stdin: Option<&crate::StreamData>,
    files: &[String],
    cwd: &std::path::Path,
    fs: &std::sync::Arc<dyn crate::fs::FileSystem>,
) -> std::result::Result<Vec<u8>, ExecResult> {
    let mut data = Vec::new();

    if files.is_empty() {
        if let Some(stdin) = stdin {
            data.extend_from_slice(stdin.as_bytes());
        }
    } else {
        for file in files {
            if file == "-" {
                if let Some(stdin) = stdin {
                    data.extend_from_slice(stdin.as_bytes());
                }
            } else {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    vfs_join(cwd, file)
                };

                let content = fs.read_file(&path).await.map_err(|e| {
                    ExecResult::err(
                        format!("{name}: {file}: {}\n", crate::error::io_error_reason(&e)),
                        1,
                    )
                })?;
                data.extend_from_slice(&content);
            }
        }
    }

    Ok(data)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn run_od(args: &[&str], stdin: Option<&str>) -> ExecResult {
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
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Od.execute(ctx).await.unwrap()
    }

    async fn run_xxd(args: &[&str], stdin: Option<&str>) -> ExecResult {
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
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Xxd.execute(ctx).await.unwrap()
    }

    async fn run_hexdump(args: &[&str], stdin: Option<&str>) -> ExecResult {
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
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Hexdump.execute(ctx).await.unwrap()
    }

    async fn run_od_with_fs(args: &[&str], files: &[(&str, &[u8])]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        for (path, content) in files {
            fs.write_file(std::path::Path::new(path), content)
                .await
                .unwrap();
        }
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

        Od.execute(ctx).await.unwrap()
    }

    // --- Od tests ---

    #[tokio::test]
    async fn test_od_basic() {
        let result = run_od(&[], Some("AB")).await;
        assert_eq!(result.exit_code, 0);
        // Default is `-t o2`: "AB" little-endian = 0x4241 = octal 041101.
        assert!(result.stdout.contains("041101"));
    }

    #[tokio::test]
    async fn test_od_hex() {
        let result = run_od(&["-t", "x"], Some("AB")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("41")); // 'A' = 0x41
        assert!(result.stdout.contains("42")); // 'B' = 0x42
    }

    #[tokio::test]
    async fn test_od_decimal() {
        let result = run_od(&["-t", "d"], Some("A")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains(" 65")); // 'A' = 65
    }

    #[tokio::test]
    async fn test_od_char() {
        let result = run_od(&["-t", "c"], Some("A\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("A"));
        assert!(result.stdout.contains("\\n"));
    }

    #[tokio::test]
    async fn test_od_hex_addr() {
        let result = run_od(&["-A", "x"], Some("test")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.starts_with("000000 "));
    }

    #[tokio::test]
    async fn test_od_no_addr() {
        let result = run_od(&["-A", "n", "-t", "x"], Some("AB")).await;
        assert_eq!(result.exit_code, 0);
        assert!(!result.stdout.starts_with("0"));
        assert!(result.stdout.contains("41"));
    }

    #[tokio::test]
    async fn test_od_count() {
        let result = run_od(&["-N", "2", "-t", "x"], Some("ABCD")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("41"));
        assert!(result.stdout.contains("42"));
        assert!(!result.stdout.contains("43"));
    }

    #[tokio::test]
    async fn test_od_skip() {
        let result = run_od(&["-j", "2", "-t", "x"], Some("ABCD")).await;
        assert_eq!(result.exit_code, 0);
        assert!(!result.stdout.contains(" 41"));
        assert!(result.stdout.contains("43"));
    }

    #[tokio::test]
    async fn test_od_empty_input() {
        let result = run_od(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "0000000\n");
    }

    #[tokio::test]
    async fn test_od_from_file() {
        let result =
            run_od_with_fs(&["-t", "x", "/test.bin"], &[("/test.bin", &[0x41, 0x42])]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("41"));
    }

    #[tokio::test]
    async fn test_od_default_is_octal_shorts() {
        let result = run_od(&[], Some("AB")).await;
        assert_eq!(result.stdout, "0000000 041101\n0000002\n");
    }

    #[tokio::test]
    async fn test_od_multiple_types_align() {
        let result = run_od(&["-An", "-tx1", "-c"], Some("A\n")).await;
        assert_eq!(result.stdout, "  41  0a\n   A  \\n\n");
    }

    #[tokio::test]
    async fn test_od_hexl_trailer() {
        let result = run_od(&["-t", "x2z"], Some("abc")).await;
        assert_eq!(
            result.stdout,
            format!("0000000 6261 0063{}  >abc<\n0000003\n", " ".repeat(30))
        );
    }

    #[tokio::test]
    async fn test_od_duplicates_collapse() {
        let data = "a".repeat(48);
        let result = run_od(&["-An", "-tx1", "-w4"], Some(&data[..12])).await;
        assert_eq!(result.stdout, " 61 61 61 61\n*\n");
        let result = run_od(&["-An", "-v", "-tx1", "-w4"], Some(&data[..8])).await;
        assert_eq!(result.stdout, " 61 61 61 61\n 61 61 61 61\n");
    }

    #[tokio::test]
    async fn test_od_named_and_signed() {
        let result = run_od(&["-An", "-ta", "-td1"], Some("\x7f ")).await;
        assert_eq!(result.stdout, "  del   sp\n  127   32\n");
    }

    #[tokio::test]
    async fn test_od_float_rejected() {
        let result = run_od(&["-tf"], Some("abcd")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("not supported"));
    }

    // --- Xxd tests ---

    #[tokio::test]
    async fn test_xxd_basic() {
        let result = run_xxd(&[], Some("Hello")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("00000000:"));
        assert!(result.stdout.contains("4865 6c6c 6f"));
        assert!(result.stdout.contains("Hello"));
    }

    #[tokio::test]
    async fn test_xxd_plain() {
        let result = run_xxd(&["-p"], Some("Hi")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "4869\n");
    }

    #[tokio::test]
    async fn test_xxd_length() {
        let result = run_xxd(&["-l", "3", "-p"], Some("Hello World")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "48656c\n");
    }

    #[tokio::test]
    async fn test_xxd_offset() {
        let result = run_xxd(&["-s", "2", "-p"], Some("Hello")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "6c6c6f\n");
    }

    #[tokio::test]
    async fn test_xxd_cols() {
        let result = run_xxd(&["-c", "4"], Some("ABCDEFGH")).await;
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("00000000:"));
        assert!(lines[1].contains("00000004:"));
    }

    #[tokio::test]
    async fn test_xxd_empty() {
        let result = run_xxd(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_xxd_group() {
        let result = run_xxd(&["-g", "1"], Some("AB")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("41 42"));
    }

    #[tokio::test]
    async fn test_xxd_non_printable() {
        let result = run_xxd(&["-p"], Some("\x00\x01\x02")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "000102\n");
    }

    #[tokio::test]
    async fn test_xxd_reverse_plain() {
        let result = run_xxd(&["-r", "-p"], Some("48656c6c6f")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Hello");
    }

    #[tokio::test]
    async fn test_xxd_reverse_plain_whitespace() {
        let result = run_xxd(&["-r", "-p"], Some("4865 6c6c\n6f")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Hello");
    }

    #[tokio::test]
    async fn test_xxd_unknown_option() {
        let result = run_xxd(&["-Q"], Some("Hi")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_xxd_reverse_normal() {
        // Normal xxd output format
        let result = run_xxd(
            &["-r"],
            Some("00000000: 4865 6c6c 6f                             Hello"),
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Hello");
    }

    // --- Hexdump tests ---

    #[tokio::test]
    async fn test_hexdump_default() {
        let result = run_hexdump(&[], Some("AB")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("4241")); // Little-endian
    }

    #[tokio::test]
    async fn test_hexdump_canonical() {
        let result = run_hexdump(&["-C"], Some("Hello")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("48 65 6c 6c 6f"));
        assert!(result.stdout.contains("|Hello|"));
    }

    #[tokio::test]
    async fn test_hexdump_canonical_non_printable() {
        let result = run_hexdump(&["-C"], Some("\x00\x01\x02")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("00 01 02"));
        assert!(result.stdout.contains("|...|"));
    }

    #[tokio::test]
    async fn test_hexdump_length() {
        let result = run_hexdump(&["-C", "-n", "3"], Some("Hello World")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("48 65 6c"));
        assert!(!result.stdout.contains("6f")); // 'o' should not be there
    }

    #[tokio::test]
    async fn test_hexdump_offset() {
        let result = run_hexdump(&["-C", "-s", "2"], Some("Hello")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("6c 6c 6f"));
    }

    #[tokio::test]
    async fn test_hexdump_empty() {
        let result = run_hexdump(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_hexdump_unknown_option() {
        let result = run_hexdump(&["-Q"], Some("Hi")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_hexdump_canonical_final_offset() {
        let result = run_hexdump(&["-C"], Some("AB")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("00000002")); // final offset
    }

    /// TM-INF-022: a missing operand must not put a Rust enum shape on
    /// stderr. Guards the whole `UNIVERSAL_BANNED` list, not just the two
    /// shapes `Error::Internal(Error::Io)` used to produce, so a future
    /// rewording cannot reintroduce a different leak.
    #[tokio::test]
    async fn no_leak_missing_operand() {
        let result = run_od_with_fs(&["/nope"], &[]).await;
        crate::testing::assert_no_leak(&result, "od", &[]);
        assert_eq!(result.stderr, "od: /nope: No such file or directory\n");
        assert_eq!(result.exit_code, 1);
    }
}
