//! install builtin - copy files and set attributes
//!
//! Owner/group options (`-o`, `-g`) are accepted and ignored: the VFS has a
//! single virtual user. `-s` (strip) is a no-op since there are no binaries.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// `install` builtin.
pub struct Install;

const HELP: &str = "Usage: install [OPTION]... SOURCE DEST\n  or:  install [OPTION]... SOURCE... DIRECTORY\n  or:  install [OPTION]... -t DIRECTORY SOURCE...\n  or:  install [OPTION]... -d DIRECTORY...\nCopy SOURCE to DEST or multiple SOURCEs to DIRECTORY, setting modes.\n\n  -d, --directory\ttreat all arguments as directory names; create them\n  -D\tcreate all leading components of DEST\n  -m, --mode=MODE\tset permission mode (as in chmod), default rwxr-xr-x\n  -t, --target-directory=DIR\tcopy all SOURCE arguments into DIR\n  -T, --no-target-directory\ttreat DEST as a normal file\n  -v, --verbose\tprint the name of each created file or directory\n  -o, -g OWNER\taccepted, ignored (single virtual user)\n  -p, -s, -c, -C\taccepted, no effect\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n";

fn parse_mode(mode: &str) -> Option<u32> {
    if !mode.is_empty() && mode.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        return u32::from_str_radix(mode, 8).ok().filter(|m| *m <= 0o7777);
    }
    super::fileops::apply_symbolic_mode(mode, 0)
}

#[async_trait]
impl Builtin for Install {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(ctx.args, HELP, Some("install (bashkit) 0.1")) {
            return Ok(r);
        }
        let (mut dir_mode, mut make_leading, mut verbose, mut no_target) =
            (false, false, false, false);
        let mut mode = 0o755u32;
        let mut target: Option<String> = None;
        let mut operands: Vec<String> = Vec::new();
        let mut args = ctx.args.iter();
        let fail = |m: String| Ok(ExecResult::err(format!("install: {m}\n"), 1));
        while let Some(arg) = args.next() {
            let a = arg.as_str();
            if !a.starts_with('-') || a == "-" {
                operands.push(arg.clone());
                continue;
            }
            if a == "--" {
                operands.extend(args.by_ref().cloned());
                break;
            }
            if let Some(long) = a.strip_prefix("--") {
                let (key, val) = match long.split_once('=') {
                    Some((k, v)) => (k, Some(v.to_string())),
                    None => (long, None),
                };
                let mut value = || val.clone().or_else(|| args.next().cloned());
                match key {
                    "directory" => dir_mode = true,
                    "verbose" => verbose = true,
                    "no-target-directory" => no_target = true,
                    "preserve-timestamps" | "strip" | "compare" => {}
                    "mode" => match value().as_deref().and_then(parse_mode) {
                        Some(m) => mode = m,
                        None => return fail("invalid mode".to_string()),
                    },
                    "target-directory" => target = value(),
                    "owner" | "group" => {
                        value();
                    }
                    _ => return fail(format!("unrecognized option '{a}'")),
                }
                continue;
            }
            let chars: Vec<char> = a[1..].chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let c = chars[i];
                let rest: String = chars[i + 1..].iter().collect();
                let mut take = || {
                    if rest.is_empty() {
                        args.next().cloned()
                    } else {
                        Some(rest.clone())
                    }
                };
                match c {
                    'd' => dir_mode = true,
                    'D' => make_leading = true,
                    'v' => verbose = true,
                    'T' => no_target = true,
                    'p' | 's' | 'c' | 'C' => {}
                    'm' => {
                        match take().as_deref().and_then(parse_mode) {
                            Some(m) => mode = m,
                            None => return fail("invalid mode".to_string()),
                        }
                        break;
                    }
                    't' => {
                        target = take();
                        break;
                    }
                    'o' | 'g' => {
                        take();
                        break;
                    }
                    _ => return fail(format!("invalid option -- '{c}'")),
                }
                i += 1;
            }
        }

        let mut out = String::new();
        if dir_mode {
            if operands.is_empty() {
                return fail("missing operand".to_string());
            }
            for d in &operands {
                let p = super::resolve_path(ctx.cwd, d);
                if let Err(e) = ctx.fs.mkdir(&p, true).await {
                    return fail(format!("cannot create directory '{d}': {e}"));
                }
                let _ = ctx.fs.chmod(&p, mode).await;
                if verbose {
                    out.push_str(&format!("install: creating directory '{d}'\n"));
                }
            }
            return Ok(ExecResult::ok(out));
        }

        let (sources, dest_dir, dest_file): (Vec<String>, Option<String>, Option<String>) =
            if let Some(t) = target {
                (operands, Some(t), None)
            } else {
                if operands.len() < 2 {
                    return fail(match operands.first() {
                        Some(f) => format!("missing destination file operand after '{f}'"),
                        None => "missing file operand".to_string(),
                    });
                }
                let dest = operands.pop().unwrap_or_default();
                let dest_path = super::resolve_path(ctx.cwd, &dest);
                let is_dir = !no_target
                    && ctx
                        .fs
                        .stat(&dest_path)
                        .await
                        .is_ok_and(|m| m.file_type.is_dir());
                if operands.len() > 1 && !is_dir && !make_leading {
                    return fail(format!("target '{dest}' is not a directory"));
                }
                if is_dir || operands.len() > 1 {
                    (operands, Some(dest), None)
                } else {
                    (operands, None, Some(dest))
                }
            };

        if let Some(dir) = &dest_dir {
            let p = super::resolve_path(ctx.cwd, dir);
            if make_leading {
                let _ = ctx.fs.mkdir(&p, true).await;
            }
        }
        for src in &sources {
            let sp = super::resolve_path(ctx.cwd, src);
            let data = match ctx.fs.stat(&sp).await {
                Ok(m) if m.file_type.is_dir() => {
                    return fail(format!("omitting directory '{src}'"));
                }
                Ok(_) => match ctx.fs.read_file(&sp).await {
                    Ok(d) => d,
                    Err(e) => return fail(format!("cannot stat '{src}': {e}")),
                },
                Err(_) => {
                    return fail(format!("cannot stat '{src}': No such file or directory"));
                }
            };
            let dest = match (&dest_dir, &dest_file) {
                (Some(dir), _) => {
                    let base = src.rsplit('/').next().unwrap_or(src);
                    format!("{}/{}", dir.trim_end_matches('/'), base)
                }
                (None, Some(f)) => f.clone(),
                (None, None) => unreachable!("install: destination resolved above"),
            };
            let dp = super::resolve_path(ctx.cwd, &dest);
            if make_leading && let Some(parent) = dp.parent() {
                let _ = ctx.fs.mkdir(parent, true).await;
            }
            if let Err(e) = ctx.fs.write_file(&dp, &data).await {
                return fail(format!("cannot create regular file '{dest}': {e}"));
            }
            let _ = ctx.fs.chmod(&dp, mode).await;
            if verbose {
                out.push_str(&format!("'{src}' -> '{dest}'\n"));
            }
        }
        Ok(ExecResult::ok(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes() {
        assert_eq!(parse_mode("644"), Some(0o644));
        assert_eq!(parse_mode("u+x"), Some(0o100));
        assert_eq!(parse_mode("99"), None);
    }
}
