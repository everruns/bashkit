//! Text reversal builtins: tac (reverse line order) and rev (reverse characters per line).
//!
//! `tac`'s argument surface is generated from uutils via
//! `bashkit-coreutils-port` — see `generated/tac_args.rs`. `rev` keeps a
//! handwritten parser because uutils does not ship a `rev` (BSD-only).

use async_trait::async_trait;
use std::ffi::OsString;
use std::path::Path;

use super::generated::tac_args::tac_command;
use super::{Builtin, Context, read_text_file};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Read input from files or stdin, returning the raw text. Used by `rev`.
async fn read_input(ctx: &Context<'_>) -> std::result::Result<String, ExecResult> {
    let mut files: Vec<&str> = Vec::new();
    for arg in ctx.args {
        if !arg.starts_with('-') {
            files.push(arg);
        }
    }

    let mut raw = String::new();
    if files.is_empty() {
        if let Some(stdin) = ctx.stdin {
            raw.push_str(stdin);
        }
    } else {
        for file in &files {
            if *file == "-" {
                if let Some(stdin) = ctx.stdin {
                    raw.push_str(stdin);
                }
            } else {
                let path = if Path::new(file).is_absolute() {
                    file.to_string()
                } else {
                    ctx.cwd.join(file).to_string_lossy().to_string()
                };
                let text = read_text_file(&*ctx.fs, Path::new(&path), "rev").await?;
                raw.push_str(&text);
            }
        }
    }
    Ok(raw)
}

/// The tac builtin — concatenate and print files in reverse (line order).
pub struct Tac;

#[async_trait]
impl Builtin for Tac {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let argv: Vec<OsString> = std::iter::once(OsString::from("tac"))
            .chain(ctx.args.iter().map(OsString::from))
            .collect();

        let cmd = tac_command().help_template("Usage: {usage}\n{about}\n\n{all-args}\n");
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

        // TODO(uutils-parity): implement -b/--before, -r/--regex,
        // -s/--separator. The generated parser accepts them so scripts that
        // pass the flags are not rejected outright; until the body lands we
        // error explicitly when any are actually requested rather than
        // silently no-op'ing.
        if matches.get_flag("before")
            || matches.get_flag("regex")
            || matches.contains_id("separator")
        {
            return Ok(ExecResult::err(
                "tac: -b/-r/-s not yet implemented in bashkit\n".to_string(),
                2,
            ));
        }

        let files: Vec<String> = matches
            .get_many::<OsString>("file")
            .map(|vs| vs.map(|v| v.to_string_lossy().into_owned()).collect())
            .unwrap_or_default();

        let raw = match read_tac_files(&ctx, &files).await {
            Ok(r) => r,
            Err(e) => return Ok(e),
        };

        Ok(ExecResult::ok(reverse_lines(&raw)))
    }
}

async fn read_tac_files(
    ctx: &Context<'_>,
    files: &[String],
) -> std::result::Result<String, ExecResult> {
    let mut raw = String::new();
    if files.is_empty() {
        if let Some(stdin) = ctx.stdin {
            raw.push_str(stdin);
        }
        return Ok(raw);
    }
    for file in files {
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
            let text = read_text_file(&*ctx.fs, Path::new(&path), "tac").await?;
            raw.push_str(&text);
        }
    }
    Ok(raw)
}

fn reverse_lines(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let has_trailing_newline = raw.ends_with('\n');
    let trimmed = if has_trailing_newline {
        &raw[..raw.len() - 1]
    } else {
        raw
    };
    let mut lines: Vec<&str> = trimmed.split('\n').collect();
    lines.reverse();
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// The rev builtin - reverse characters of each line.
pub struct Rev;

#[async_trait]
impl Builtin for Rev {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: rev [FILE]...\nReverse characters of each line.\n\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("rev (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let raw = match read_input(&ctx).await {
            Ok(r) => r,
            Err(e) => return Ok(e),
        };

        if raw.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let has_trailing_newline = raw.ends_with('\n');
        let trimmed = if has_trailing_newline {
            &raw[..raw.len() - 1]
        } else {
            &raw
        };

        let mut output = String::new();
        for (i, line) in trimmed.split('\n').enumerate() {
            if i > 0 {
                output.push('\n');
            }
            let reversed: String = line.chars().rev().collect();
            output.push_str(&reversed);
        }
        output.push('\n');

        Ok(ExecResult::ok(output))
    }
}
