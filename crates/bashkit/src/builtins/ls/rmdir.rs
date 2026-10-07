//! rmdir builtin - remove empty directories.

use async_trait::async_trait;

use crate::builtins::arg_parser::OptArg;
use crate::builtins::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The rmdir builtin - remove empty directories.
///
/// Usage: rmdir [-pv] [--ignore-fail-on-non-empty] DIRECTORY...
///
/// Options:
///   -p   Remove DIRECTORY and then each of its operand prefixes (a/b/c, a/b, a)
///   -v   Print a line for every directory processed
pub struct Rmdir;

#[async_trait]
impl Builtin for Rmdir {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = crate::builtins::check_help_version(
            ctx.args,
            "Usage: rmdir [OPTION]... DIRECTORY...\nRemove empty directories.\n\n  -p, --parents\tremove DIRECTORY and its ancestors\n  -v, --verbose\toutput a diagnostic for every directory processed\n      --ignore-fail-on-non-empty\tignore failures due to non-empty directories\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n",
            Some("rmdir (bashkit) 0.1"),
        ) {
            return Ok(r);
        }

        let (parsed, dirs) = match crate::builtins::arg_parser::gnu_getopt(
            "rmdir",
            ctx.args,
            "pv",
            &[
                ("parents", OptArg::No, 'p'),
                ("verbose", OptArg::No, 'v'),
                ("ignore-fail-on-non-empty", OptArg::No, 'i'),
            ],
            true,
            1,
        ) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let parents = parsed.iter().any(|o| o.key == 'p');
        let verbose = parsed.iter().any(|o| o.key == 'v');
        let ignore_non_empty = parsed.iter().any(|o| o.key == 'i');
        if dirs.is_empty() {
            return Ok(ExecResult::err("rmdir: missing operand\n".to_string(), 1));
        }

        let mut stdout = String::new();
        let mut stderr = String::new();
        for dir in &dirs {
            // GNU -p walks the operand's own prefixes: a/b/c, a/b, a.
            let mut target = dir.as_str();
            loop {
                if verbose {
                    stdout.push_str(&format!("rmdir: removing directory, '{target}'\n"));
                }
                match remove_empty_dir(&ctx, target).await? {
                    Ok(()) => {}
                    Err(reason) => {
                        if !(ignore_non_empty && reason == "Directory not empty") {
                            stderr.push_str(&format!(
                                "rmdir: failed to remove '{target}': {reason}\n"
                            ));
                        }
                        break;
                    }
                }
                if !parents {
                    break;
                }
                let trimmed = target.trim_end_matches('/');
                match trimmed.rfind('/') {
                    Some(i) => {
                        let parent = trimmed[..i].trim_end_matches('/');
                        if parent.is_empty() {
                            break;
                        }
                        target = parent;
                    }
                    None => break,
                }
            }
        }

        Ok(ExecResult {
            stdout: stdout.into(),
            exit_code: i32::from(!stderr.is_empty()),
            stderr: stderr.into(),
            ..Default::default()
        })
    }
}

/// Remove one empty directory; the inner error is GNU's reason text.
async fn remove_empty_dir(
    ctx: &Context<'_>,
    dir: &str,
) -> Result<std::result::Result<(), &'static str>> {
    let path = resolve_path(ctx.cwd, dir);
    let Ok(metadata) = ctx.fs.lstat(&path).await else {
        return Ok(Err("No such file or directory"));
    };
    if !metadata.file_type.is_dir() {
        return Ok(Err("Not a directory"));
    }
    if !ctx.fs.read_dir(&path).await?.is_empty() {
        return Ok(Err("Directory not empty"));
    }
    if ctx.fs.remove(&path, false).await.is_err() {
        return Ok(Err("Device or resource busy"));
    }
    Ok(Ok(()))
}
