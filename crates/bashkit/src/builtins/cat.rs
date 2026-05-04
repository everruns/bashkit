//! cat builtin.
//!
//! Argument surface is generated from uutils/coreutils' `uu_app()` via the
//! `bashkit-coreutils-port` codegen tool — see `generated/cat_args.rs` and
//! `crates/bashkit-coreutils-port/`. Behaviour is implemented locally
//! against the bashkit VFS.

use async_trait::async_trait;
use std::ffi::OsString;
use std::path::Path;

use super::generated::cat_args::cat_command;
use super::{Builtin, Context, read_text_file};
use crate::error::Result;
use crate::interpreter::ExecResult;

pub struct Cat;

#[async_trait]
impl Builtin for Cat {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // clap expects argv[0] = program name; bashkit's ctx.args excludes it.
        let argv: Vec<OsString> = std::iter::once(OsString::from("cat"))
            .chain(ctx.args.iter().map(OsString::from))
            .collect();

        // GNU coreutils' help layout opens with the usage line; clap's
        // default template leads with the `about`. uutils handles this via
        // uucore's `localized_help_template`, which we drop during codegen
        // because it pulls in Fluent. Re-apply a GNU-equivalent template.
        let cmd = cat_command().help_template("Usage: {usage}\n{about}\n\n{all-args}\n");
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

        let mut raw = String::new();
        for file in &files {
            if file == "-" {
                if let Some(stdin) = ctx.stdin {
                    raw.push_str(stdin);
                }
            } else {
                let path = if Path::new(file).is_absolute() {
                    file.clone()
                } else {
                    ctx.cwd.join(file).to_string_lossy().into_owned()
                };
                match read_text_file(&*ctx.fs, Path::new(&path), "cat").await {
                    Ok(t) => raw.push_str(&t),
                    Err(e) => return Ok(e),
                }
            }
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
    raw: &str,
    show_ends: bool,
    show_tabs: bool,
    show_nonprinting: bool,
    number_all: bool,
    number_nonblank: bool,
    squeeze: bool,
) -> String {
    use std::fmt::Write;

    let mut out = String::with_capacity(raw.len());
    let mut counter: u64 = 0;
    let mut prev_blank = false;

    let mut iter = raw.split_inclusive('\n').peekable();
    if iter.peek().is_none() {
        return out;
    }

    for chunk in iter {
        let (body, sep): (&str, &str) = match chunk.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (chunk, ""),
        };
        let is_blank = body.is_empty();

        if squeeze && is_blank && prev_blank {
            continue;
        }
        prev_blank = is_blank;

        if number_all || (number_nonblank && !is_blank) {
            counter += 1;
            let _ = write!(out, "{counter:>6}\t");
        }

        if show_nonprinting || show_tabs {
            for b in body.bytes() {
                emit_byte(&mut out, b, show_tabs, show_nonprinting);
            }
        } else {
            out.push_str(body);
        }

        if show_ends && !sep.is_empty() {
            out.push('$');
        }
        out.push_str(sep);
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
fn emit_byte(out: &mut String, b: u8, show_tabs: bool, show_nonprinting: bool) {
    match b {
        b'\t' if show_tabs => {
            out.push('^');
            out.push('I');
        }
        b'\t' => out.push('\t'),
        b'\n' => out.push('\n'),
        0..=31 if show_nonprinting => {
            out.push('^');
            out.push((b + 64) as char);
        }
        0x7f if show_nonprinting => {
            out.push('^');
            out.push('?');
        }
        128..=255 if show_nonprinting => {
            out.push_str("M-");
            let low = b & 0x7f;
            if (0..32).contains(&low) {
                out.push('^');
                out.push((low + 64) as char);
            } else if low == 0x7f {
                out.push('^');
                out.push('?');
            } else {
                out.push(low as char);
            }
        }
        _ => out.push(b as char),
    }
}
