//! Text reversal builtins: tac (reverse line order) and rev (reverse characters per line).
//!
//! `tac`'s argument surface is generated from uutils via
//! `bashkit-coreutils-port` — see `generated/tac_args.rs`. `rev` keeps a
//! handwritten parser because uutils does not ship a `rev` (BSD-only).

use super::clap_cache::cached_command;
use async_trait::async_trait;
use std::ffi::OsString;
use std::path::Path;

use super::{Builtin, Context, read_text_file};
use crate::error::Result;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;

/// Read input from files or stdin, returning the raw text. Used by `rev`.
async fn read_input(ctx: &Context<'_>) -> std::result::Result<String, ExecResult> {
    let mut files: Vec<&str> = Vec::new();
    for arg in ctx.args {
        if !arg.starts_with('-') {
            files.push(arg);
        } else if arg.len() > 1 && arg != "--" {
            // rev takes no options → reject unknown option-shaped tokens (exits 1).
            return Err(super::invalid_option("rev", arg, 1));
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
                    vfs_join(ctx.cwd, file).to_string_lossy().to_string()
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

// Cached `tac` arg surface: pre-built once, cloned per invocation.
// See `builtins::clap_cache` for why it is built, not just constructed.
cached_command!(tac_cmd, super::generated::tac_args::tac_command());

#[async_trait]
impl Builtin for Tac {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let argv: Vec<OsString> = std::iter::once(OsString::from("tac"))
            .chain(ctx.args.iter().map(OsString::from))
            .collect();

        let matches = match tac_cmd().try_get_matches_from(argv) {
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

        let before = matches.get_flag("before");
        let regex = matches.get_flag("regex");
        // GNU: an empty separator means a NUL byte.
        let separator = match matches.get_one::<OsString>("separator") {
            Some(s) if s.is_empty() => "\0".to_string(),
            Some(s) => s.to_string_lossy().into_owned(),
            None => "\n".to_string(),
        };
        let sep = if regex {
            let translated = super::sed::translate_posix_regex(&separator, false);
            match super::search_common::build_regex(&translated) {
                Ok(re) => TacSep::Regex(re),
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("tac: invalid regular expression: '{separator}'\n"),
                        1,
                    ));
                }
            }
        } else {
            TacSep::Literal(separator)
        };

        let mut files: Vec<String> = matches
            .get_many::<OsString>("file")
            .map(|vs| vs.map(|v| v.to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        if files.is_empty() {
            files.push("-".to_string());
        }

        // GNU tac reverses each file on its own, then outputs them in order.
        let mut out = String::new();
        for file in &files {
            let raw = match read_tac_file(&ctx, file).await {
                Ok(r) => r,
                Err(e) => return Ok(e),
            };
            reverse_records(&raw, &sep, before, &mut out);
        }
        Ok(ExecResult::ok(out))
    }
}

enum TacSep {
    Literal(String),
    Regex(regex::Regex),
}

impl TacSep {
    /// Byte ranges of every non-empty separator match, left to right.
    fn matches(&self, raw: &str) -> Vec<(usize, usize)> {
        match self {
            TacSep::Literal(s) => raw
                .match_indices(s.as_str())
                .map(|(i, m)| (i, i + m.len()))
                .collect(),
            TacSep::Regex(re) => re
                .find_iter(raw)
                .filter(|m| !m.is_empty())
                .map(|m| (m.start(), m.end()))
                .collect(),
        }
    }
}

async fn read_tac_file(ctx: &Context<'_>, file: &str) -> std::result::Result<String, ExecResult> {
    if file == "-" {
        return Ok(ctx.stdin.map(ToString::to_string).unwrap_or_default());
    }
    let path = if Path::new(file).is_absolute() {
        file.to_string()
    } else {
        vfs_join(ctx.cwd, file).to_string_lossy().into_owned()
    };
    read_text_file(&*ctx.fs, Path::new(&path), "tac").await
}

/// Split `raw` into records that end with (or, with `before`, start with) a
/// separator, and append them in reverse order. An unterminated last record
/// is kept as-is, so it joins the previous record without a separator.
fn reverse_records(raw: &str, sep: &TacSep, before: bool, out: &mut String) {
    let mut cuts = vec![0];
    for (start, end) in sep.matches(raw) {
        cuts.push(if before { start } else { end });
    }
    cuts.push(raw.len());
    cuts.dedup();
    for w in cuts.windows(2).rev() {
        out.push_str(&raw[w[0]..w[1]]);
    }
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
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_rev(args: &[&str], stdin: Option<&str>) -> ExecResult {
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

        Rev.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_rev_basic() {
        let result = run_rev(&[], Some("hello\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "olleh\n");
    }

    #[tokio::test]
    async fn test_rev_invalid_option() {
        let result = run_rev(&["-Q"], Some("hello\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    fn tac(raw: &str, sep: TacSep, before: bool) -> String {
        let mut out = String::new();
        reverse_records(raw, &sep, before, &mut out);
        out
    }

    #[test]
    fn test_tac_records_default_and_unterminated() {
        let nl = || TacSep::Literal("\n".to_string());
        assert_eq!(tac("a\nb\n", nl(), false), "b\na\n");
        assert_eq!(tac("a\nb", nl(), false), "ba\n");
        assert_eq!(tac("", nl(), false), "");
    }

    #[test]
    fn test_tac_records_separator_and_before() {
        let x = || TacSep::Literal("X".to_string());
        assert_eq!(tac("aXbXcX", x(), false), "cXbXaX");
        assert_eq!(tac("XaXb", x(), true), "XbXa");
    }

    #[test]
    fn test_tac_records_regex() {
        let re = regex::Regex::new("[0-9]").unwrap();
        assert_eq!(tac("a1b22c", TacSep::Regex(re), false), "c2b2a1");
    }
}
