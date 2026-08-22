//! cat builtin.
//!
//! Argument surface is generated from uutils/coreutils' `uu_app()` via the
//! `bashkit-coreutils-port` codegen tool — see `generated/cat_args.rs` and
//! `crates/bashkit-coreutils-port/`. Behaviour is implemented locally
//! against the bashkit VFS.

use super::clap_cache::cached_command;
use async_trait::async_trait;
use std::ffi::OsString;
use std::path::Path;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

pub struct Cat;

// Cached `cat` arg surface: pre-built once, cloned per invocation.
// See `builtins::clap_cache` for why it is built, not just constructed.
cached_command!(cat_cmd, super::generated::cat_args::cat_command());

#[async_trait]
impl Builtin for Cat {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // clap expects argv[0] = program name; bashkit's ctx.args excludes it.
        let argv: Vec<OsString> = std::iter::once(OsString::from("cat"))
            .chain(ctx.args.iter().map(OsString::from))
            .collect();

        let matches = match cat_cmd().try_get_matches_from(argv) {
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

        // Composite flags: -A = -vET, -e = -vE, -t = -vT.
        let g = |k: &str| matches.get_flag(k);
        let show_all = g("show-all");
        let number_nonblank = g("number-nonblank");
        let nonprint_ends = g("e");
        let nonprint_tabs = g("t");
        let show_ends = g("show-ends") || show_all || nonprint_ends;
        let show_tabs = g("show-tabs") || show_all || nonprint_tabs;
        let show_nonprinting = g("show-nonprinting") || show_all || nonprint_ends || nonprint_tabs;
        let number_all = g("number") && !number_nonblank;
        let squeeze = g("squeeze-blank");

        // GNU `cat` reads stdin when FILE is "-" or absent. clap defaults
        // FILE to "-" so absence and "-" are unified.
        let files: Vec<String> = matches
            .get_many::<OsString>("file")
            .map(|vs| vs.map(|v| v.to_string_lossy().into_owned()).collect())
            .unwrap_or_default();

        let mut raw = Vec::new();
        for file in &files {
            if file == "-" {
                if let Some(stdin) = ctx.stdin {
                    raw.extend_from_slice(stdin.as_bytes());
                }
            } else {
                let path = if Path::new(file).is_absolute() {
                    file.clone()
                } else {
                    ctx.cwd.join(file).to_string_lossy().into_owned()
                };
                match ctx.fs.read_file(Path::new(&path)).await {
                    Ok(bytes) => raw.extend_from_slice(&bytes),
                    Err(e) => return Ok(ExecResult::err(format!("cat: {file}: {e}\n"), 1)),
                }
            }
        }

        if !(show_ends || show_tabs || show_nonprinting || number_all || number_nonblank || squeeze)
        {
            return Ok(ExecResult::ok_bytes(raw));
        }

        let output = render(
            &raw,
            show_ends,
            show_tabs,
            show_nonprinting,
            number_all,
            number_nonblank,
            squeeze,
        );
        Ok(ExecResult::ok(output))
    }
}

/// Apply cat's display transforms in a single pass.
///
/// One pass because numbering interacts with squeezing — squeezed blank
/// lines must not be numbered. Two passes mis-number `cat -ns`.
fn render(
    raw: &[u8],
    show_ends: bool,
    show_tabs: bool,
    show_nonprinting: bool,
    number_all: bool,
    number_nonblank: bool,
    squeeze: bool,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut counter: u64 = 0;
    let mut prev_blank = false;

    let mut iter = raw.split_inclusive(|byte| *byte == b'\n').peekable();
    if iter.peek().is_none() {
        return out;
    }

    for chunk in iter {
        let (body, has_newline): (&[u8], bool) = match chunk.strip_suffix(b"\n") {
            Some(body) => (body, true),
            None => (chunk, false),
        };
        let is_blank = body.is_empty();

        if squeeze && is_blank && prev_blank {
            continue;
        }
        prev_blank = is_blank;

        if number_all || (number_nonblank && !is_blank) {
            counter += 1;
            out.extend_from_slice(format!("{counter:>6}\t").as_bytes());
        }

        if show_nonprinting || show_tabs {
            for &b in body {
                emit_byte(&mut out, b, show_tabs, show_nonprinting);
            }
        } else {
            out.extend_from_slice(body);
        }

        if show_ends && has_newline {
            out.push(b'$');
        }
        if has_newline {
            out.push(b'\n');
        }
    }
    out
}

/// GNU cat -v style byte rendering.
///
/// - tab (0x09): literal '\t' unless `show_tabs` (then `^I`).
/// - bytes < 0x20 (other than tab/newline): `^X` (X = byte + 64) when
///   show_nonprinting; passed through otherwise.
/// - 0x7F (DEL): `^?`.
/// - 0x80..=0xFF (high bit set): `M-` prefix + low-7-bit rendered same way.
fn emit_byte(out: &mut Vec<u8>, b: u8, show_tabs: bool, show_nonprinting: bool) {
    match b {
        b'\t' if show_tabs => {
            out.extend_from_slice(b"^I");
        }
        b'\t' => out.push(b'\t'),
        b'\n' => out.push(b'\n'),
        0..=31 if show_nonprinting => {
            out.push(b'^');
            out.push(b + 64);
        }
        0x7f if show_nonprinting => {
            out.extend_from_slice(b"^?");
        }
        128..=255 if show_nonprinting => {
            out.extend_from_slice(b"M-");
            let low = b & 0x7f;
            if (0..32).contains(&low) {
                out.push(b'^');
                out.push(low + 64);
            } else if low == 0x7f {
                out.extend_from_slice(b"^?");
            } else {
                out.push(low);
            }
        }
        _ => out.push(b),
    }
}
