//! File operation builtins - mkdir, rm, cp, mv, touch, chmod
// Decision: touch delegates mtime changes to the filesystem layer so `touch`
// and `touch -t` stay consistent across in-memory, overlay, and realfs backends.
// THREAT[TM-INF-018]: timezone-naive `touch -t` stamps are UTC; host-local
// conversion would leak process timezone through a later `date -r`.

use super::clap_cache::cached_command;
use crate::time_compat::SystemTime;
use async_trait::async_trait;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use std::path::Path;

use super::limits::MKTEMP_MAX_ATTEMPTS;
use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;

/// The mkdir builtin - create directories.
///
/// Usage: mkdir [-p] DIRECTORY...
///
/// Options:
///   -p   Create parent directories as needed, no error if existing
pub struct Mkdir;

#[async_trait]
impl Builtin for Mkdir {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: mkdir [OPTION]... DIRECTORY...\nCreate the DIRECTORY(ies), if they do not already exist.\n\n  -p\t\tno error if existing, make parent directories as needed\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n",
            Some("mkdir (bashkit) 0.1"),
        ) {
            return Ok(r);
        }

        if ctx.args.is_empty() {
            return Ok(ExecResult::err("mkdir: missing operand\n".to_string(), 1));
        }

        // Reject unknown options like GNU mkdir. Only -p/--parents has an
        // effect in the VFS; other real-mkdir flags are accepted and ignored.
        // '--' ends option parsing.
        let mut recursive = false;
        let mut dirs: Vec<&String> = Vec::new();
        let mut opts_done = false;
        for arg in ctx.args {
            if opts_done {
                dirs.push(arg);
            } else if arg == "--" {
                opts_done = true;
            } else if let Some(long) = arg.strip_prefix("--") {
                match long.split('=').next().unwrap_or("") {
                    "parents" => recursive = true,
                    "mode" | "verbose" | "context" => {}
                    _ => return Ok(super::invalid_option("mkdir", arg, 1)),
                }
            } else if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        'p' => recursive = true,
                        'm' | 'v' | 'Z' => {}
                        _ => return Ok(super::invalid_option("mkdir", &format!("-{c}"), 1)),
                    }
                }
            } else {
                dirs.push(arg);
            }
        }

        if dirs.is_empty() {
            return Ok(ExecResult::err("mkdir: missing operand\n".to_string(), 1));
        }

        for dir in dirs {
            let path = resolve_path(ctx.cwd, dir);

            // Check if already exists
            if ctx.fs.exists(&path).await.unwrap_or(false) {
                // Check if it's a directory or something else (file/symlink)
                if let Ok(meta) = ctx.fs.stat(&path).await
                    && meta.file_type.is_dir()
                {
                    if !recursive {
                        return Ok(ExecResult::err(
                            format!("mkdir: cannot create directory '{}': File exists\n", dir),
                            1,
                        ));
                    }
                    // With -p, existing directory is not an error
                    continue;
                }
                // File or symlink exists - always an error
                return Ok(ExecResult::err(
                    format!("mkdir: cannot create directory '{}': File exists\n", dir),
                    1,
                ));
            }

            if let Err(e) = ctx.fs.mkdir(&path, recursive).await {
                return Ok(ExecResult::err(
                    format!("mkdir: cannot create directory '{}': {}\n", dir, e),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The rm builtin - remove files or directories.
///
/// Usage: rm [-rf] FILE...
///
/// Options:
///   -r, -R   Remove directories and their contents recursively
///   -f       Ignore nonexistent files, never prompt
pub struct Rm;

#[async_trait]
impl Builtin for Rm {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: rm [OPTION]... [FILE]...\nRemove (unlink) the FILE(s).\n\n  -f\t\tignore nonexistent files and arguments, never prompt\n  -r, -R\tremove directories and their contents recursively\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n",
            Some("rm (bashkit) 0.1"),
        ) {
            return Ok(r);
        }

        if ctx.args.is_empty() {
            return Ok(ExecResult::err("rm: missing operand\n".to_string(), 1));
        }

        // Reject unknown options like GNU rm; parse short bundles per-char.
        // '--' ends option parsing; a lone '-' is a file operand.
        let mut recursive = false;
        let mut force = false;
        let mut files: Vec<&String> = Vec::new();
        let mut opts_done = false;
        for arg in ctx.args {
            if opts_done {
                files.push(arg);
            } else if arg == "--" {
                opts_done = true;
            } else if let Some(long) = arg.strip_prefix("--") {
                match long.split('=').next().unwrap_or("") {
                    "recursive" => recursive = true,
                    "force" => force = true,
                    "dir" | "interactive" | "verbose" | "one-file-system" | "no-preserve-root"
                    | "preserve-root" => {}
                    _ => return Ok(super::invalid_option("rm", arg, 1)),
                }
            } else if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        'r' | 'R' => recursive = true,
                        'f' => force = true,
                        'i' | 'I' | 'd' | 'v' => {}
                        _ => return Ok(super::invalid_option("rm", &format!("-{c}"), 1)),
                    }
                }
            } else {
                files.push(arg);
            }
        }

        if files.is_empty() {
            return Ok(ExecResult::err("rm: missing operand\n".to_string(), 1));
        }

        for file in files {
            let path = resolve_path(ctx.cwd, file);

            // lstat: rm acts on a link itself (dangling links included).
            let metadata = ctx.fs.lstat(&path).await;
            if metadata.is_err() {
                if !force {
                    return Ok(ExecResult::err(
                        format!("rm: cannot remove '{}': No such file or directory\n", file),
                        1,
                    ));
                }
                continue;
            }

            // Check if it's a directory
            if let Ok(meta) = metadata
                && meta.file_type.is_dir()
                && !recursive
            {
                return Ok(ExecResult::err(
                    format!("rm: cannot remove '{}': Is a directory\n", file),
                    1,
                ));
            }

            if let Err(e) = ctx.fs.remove(&path, recursive).await
                && !force
            {
                return Ok(ExecResult::err(
                    format!("rm: cannot remove '{}': {}\n", file, e),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The cp builtin - copy files and directories.
///
/// Usage: cp [-r] SOURCE... DEST
///
/// Options:
///   -r, -R   Copy directories recursively
pub struct Cp;

#[async_trait]
impl Builtin for Cp {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: cp [OPTION]... SOURCE... DEST\nCopy SOURCE to DEST, or multiple SOURCE(s) to DIRECTORY.\n\n  -r, -R\tcopy directories recursively\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n",
            Some("cp (bashkit) 0.1"),
        ) {
            return Ok(r);
        }

        // Reject unknown options like GNU cp. `-r`/`-R`/`-a` (and the long
        // forms) copy directories recursively; the remaining flags are
        // accepted and ignored. '--' ends options.
        let mut recursive = false;
        let mut files: Vec<&String> = Vec::new();
        let mut opts_done = false;
        for arg in ctx.args {
            if opts_done {
                files.push(arg);
            } else if arg == "--" {
                opts_done = true;
            } else if let Some(long) = arg.strip_prefix("--") {
                let name = long.split('=').next().unwrap_or("");
                if matches!(name, "archive" | "recursive") {
                    recursive = true;
                }
                match name {
                    "archive"
                    | "attributes-only"
                    | "backup"
                    | "copy-contents"
                    | "dereference"
                    | "no-dereference"
                    | "force"
                    | "interactive"
                    | "link"
                    | "no-clobber"
                    | "one-file-system"
                    | "parents"
                    | "preserve"
                    | "no-preserve"
                    | "recursive"
                    | "reflink"
                    | "remove-destination"
                    | "sparse"
                    | "strip-trailing-slashes"
                    | "symbolic-link"
                    | "target-directory"
                    | "no-target-directory"
                    | "update"
                    | "verbose"
                    | "context" => {}
                    _ => return Ok(super::invalid_option("cp", arg, 1)),
                }
            } else if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        'a' | 'r' | 'R' => recursive = true,
                        'd' | 'f' | 'H' | 'i' | 'l' | 'L' | 'n' | 'P' | 'p' | 's' | 't' | 'T'
                        | 'u' | 'v' | 'x' | 'Z' => {}
                        _ => return Ok(super::invalid_option("cp", &format!("-{c}"), 1)),
                    }
                }
            } else {
                files.push(arg);
            }
        }

        if files.is_empty() {
            return Ok(ExecResult::err("cp: missing file operand\n".to_string(), 1));
        }
        if files.len() < 2 {
            return Ok(ExecResult::err(
                "cp: missing destination file operand\n".to_string(),
                1,
            ));
        }

        let dest = files
            .last()
            .expect("files.last() valid: guarded by files.len() < 2 check above");
        let sources = &files[..files.len() - 1];
        let dest_path = resolve_path(ctx.cwd, dest);

        // Check if destination is a directory
        let dest_is_dir = if let Ok(meta) = ctx.fs.stat(&dest_path).await {
            meta.file_type.is_dir()
        } else {
            false
        };

        if sources.len() > 1 && !dest_is_dir {
            return Ok(ExecResult::err(
                format!("cp: target '{}' is not a directory\n", dest),
                1,
            ));
        }

        let mut stderr = String::new();
        for source in sources {
            let src_path = resolve_path(ctx.cwd, source);

            let final_dest = if dest_is_dir {
                // Copy into directory
                let filename = Path::new(source)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| source.to_string());
                vfs_join(&dest_path, &filename)
            } else {
                dest_path.clone()
            };

            let src_is_dir = ctx
                .fs
                .stat(&src_path)
                .await
                .is_ok_and(|m| m.file_type.is_dir());
            if src_is_dir {
                if !recursive {
                    stderr.push_str(&format!(
                        "cp: -r not specified; omitting directory '{source}'\n"
                    ));
                    continue;
                }
                if final_dest.starts_with(&src_path) {
                    let shown = if dest_is_dir {
                        let name = Path::new(source.as_str())
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        format!("{}/{name}", dest.trim_end_matches('/'))
                    } else {
                        dest.to_string()
                    };
                    stderr.push_str(&format!(
                        "cp: cannot copy a directory, '{source}', into itself, '{shown}'\n"
                    ));
                    continue;
                }
                if let Err(msg) = copy_tree(&ctx, &src_path, &final_dest).await? {
                    stderr.push_str(&format!("cp: {msg}\n"));
                }
                continue;
            }

            if let Err(e) = ctx.fs.copy(&src_path, &final_dest).await {
                stderr.push_str(&format!("cp: cannot copy '{}': {}\n", source, e));
            }
        }

        if stderr.is_empty() {
            Ok(ExecResult::ok(String::new()))
        } else {
            Ok(ExecResult::err(stderr, 1))
        }
    }
}

/// Recursively copy `src` to `dst` (GNU `cp -R` without `-L`: symlinks are
/// recreated, not followed). The outer error is cancellation/budget; the
/// inner one is a user-facing message for the first failed entry.
fn copy_tree<'a>(
    ctx: &'a Context<'_>,
    src: &'a Path,
    dst: &'a Path,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<std::result::Result<(), String>>> + Send + 'a>,
> {
    Box::pin(async move {
        ctx.consume_budget_work(1)?;
        let meta = match ctx.fs.lstat(src).await {
            Ok(m) => m,
            Err(e) => return Ok(Err(format!("cannot stat '{}': {e}", src.display()))),
        };
        if meta.file_type.is_dir() {
            match ctx.fs.stat(dst).await {
                Ok(m) if m.file_type.is_dir() => {}
                Ok(_) => {
                    return Ok(Err(format!(
                        "cannot overwrite non-directory '{}' with directory '{}'",
                        dst.display(),
                        src.display()
                    )));
                }
                Err(_) => {
                    if let Err(e) = ctx.fs.mkdir(dst, false).await {
                        return Ok(Err(format!(
                            "cannot create directory '{}': {e}",
                            dst.display()
                        )));
                    }
                    let _ = ctx.fs.chmod(dst, meta.mode).await;
                }
            }
            let entries = match ctx.fs.read_dir(src).await {
                Ok(e) => e,
                Err(e) => return Ok(Err(format!("cannot access '{}': {e}", src.display()))),
            };
            for entry in entries {
                let from = vfs_join(src, &entry.name);
                let to = vfs_join(dst, &entry.name);
                if let Err(msg) = copy_tree(ctx, &from, &to).await? {
                    return Ok(Err(msg));
                }
            }
            return Ok(Ok(()));
        }
        if meta.file_type.is_symlink() {
            let target = match ctx.fs.read_link(src).await {
                Ok(t) => t,
                Err(e) => {
                    return Ok(Err(format!(
                        "cannot read symbolic link '{}': {e}",
                        src.display()
                    )));
                }
            };
            if ctx.fs.lstat(dst).await.is_ok() {
                let _ = ctx.fs.remove(dst, false).await;
            }
            return Ok(ctx
                .fs
                .symlink(&target, dst)
                .await
                .map_err(|e| format!("cannot create symbolic link '{}': {e}", dst.display())));
        }
        Ok(ctx
            .fs
            .copy(src, dst)
            .await
            .map_err(|e| format!("cannot copy '{}': {e}", src.display())))
    })
}

/// The mv builtin - move (rename) files.
///
/// Usage: mv SOURCE... DEST
pub struct Mv;

#[async_trait]
impl Builtin for Mv {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: mv [OPTION]... SOURCE... DEST\nRename SOURCE to DEST, or move SOURCE(s) to DIRECTORY.\n\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n",
            Some("mv (bashkit) 0.1"),
        ) {
            return Ok(r);
        }

        // Reject unknown options like GNU mv; accept-and-ignore the rest.
        // '--' ends option parsing.
        let mut files: Vec<&String> = Vec::new();
        let mut opts_done = false;
        for arg in ctx.args {
            if opts_done {
                files.push(arg);
            } else if arg == "--" {
                opts_done = true;
            } else if let Some(long) = arg.strip_prefix("--") {
                match long.split('=').next().unwrap_or("") {
                    "backup"
                    | "force"
                    | "interactive"
                    | "no-clobber"
                    | "no-target-directory"
                    | "strip-trailing-slashes"
                    | "suffix"
                    | "target-directory"
                    | "update"
                    | "verbose"
                    | "context" => {}
                    _ => return Ok(super::invalid_option("mv", arg, 1)),
                }
            } else if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        'b' | 'f' | 'i' | 'n' | 'T' | 't' | 'u' | 'v' | 'Z' => {}
                        _ => return Ok(super::invalid_option("mv", &format!("-{c}"), 1)),
                    }
                }
            } else {
                files.push(arg);
            }
        }

        if files.is_empty() {
            return Ok(ExecResult::err("mv: missing file operand\n".to_string(), 1));
        }
        if files.len() < 2 {
            return Ok(ExecResult::err(
                "mv: missing destination file operand\n".to_string(),
                1,
            ));
        }

        let dest = files
            .last()
            .expect("files.last() valid: guarded by files.len() < 2 check above");
        let sources = &files[..files.len() - 1];
        let dest_path = resolve_path(ctx.cwd, dest);

        // Check if destination is a directory
        let dest_is_dir = if let Ok(meta) = ctx.fs.stat(&dest_path).await {
            meta.file_type.is_dir()
        } else {
            false
        };

        if sources.len() > 1 && !dest_is_dir {
            return Ok(ExecResult::err(
                format!("mv: target '{}' is not a directory\n", dest),
                1,
            ));
        }

        for source in sources {
            let src_path = resolve_path(ctx.cwd, source);

            let final_dest = if dest_is_dir {
                // Move into directory
                let filename = Path::new(source)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| source.to_string());
                vfs_join(&dest_path, &filename)
            } else {
                dest_path.clone()
            };

            if let Err(e) = ctx.fs.rename(&src_path, &final_dest).await {
                return Ok(ExecResult::err(
                    format!("mv: cannot move '{}': {}\n", source, e),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The touch builtin - change file timestamps or create empty files.
///
/// Usage: touch FILE...
#[derive(Default)]
pub struct Touch {
    clock: super::Date,
}

impl Touch {
    /// Touch builtin parsing `-d` dates against `clock` (shared with `date`).
    pub fn with_clock(clock: super::Date) -> Self {
        Self { clock }
    }
}

fn parse_touch_timestamp(raw: &str) -> std::result::Result<SystemTime, String> {
    let (main, seconds) = match raw.split_once('.') {
        Some((main, seconds)) => {
            if seconds.len() != 2 || !seconds.chars().all(|ch| ch.is_ascii_digit()) {
                return Err(format!("touch: invalid date format '{}'\n", raw));
            }
            let seconds = seconds
                .parse::<u32>()
                .map_err(|_| format!("touch: invalid date format '{}'\n", raw))?;
            (main, seconds)
        }
        None => (raw, 0),
    };

    if !main.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(format!("touch: invalid date format '{}'\n", raw));
    }

    let year = match main.len() {
        8 => crate::time_compat::now_utc().year(),
        10 => {
            let yy = main[0..2]
                .parse::<i32>()
                .map_err(|_| format!("touch: invalid date format '{}'\n", raw))?;
            if yy >= 69 { 1900 + yy } else { 2000 + yy }
        }
        12 => main[0..4]
            .parse::<i32>()
            .map_err(|_| format!("touch: invalid date format '{}'\n", raw))?,
        _ => return Err(format!("touch: invalid date format '{}'\n", raw)),
    };

    let offset = main.len() - 8;
    let month = main[offset..offset + 2]
        .parse::<u32>()
        .map_err(|_| format!("touch: invalid date format '{}'\n", raw))?;
    let day = main[offset + 2..offset + 4]
        .parse::<u32>()
        .map_err(|_| format!("touch: invalid date format '{}'\n", raw))?;
    let hour = main[offset + 4..offset + 6]
        .parse::<u32>()
        .map_err(|_| format!("touch: invalid date format '{}'\n", raw))?;
    let minute = main[offset + 6..offset + 8]
        .parse::<u32>()
        .map_err(|_| format!("touch: invalid date format '{}'\n", raw))?;

    let naive = NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|date| date.and_hms_opt(hour, minute, seconds))
        .ok_or_else(|| format!("touch: invalid date format '{}'\n", raw))?;

    let utc = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
    Ok(crate::time_compat::from_chrono(utc))
}

const TOUCH_HELP: &str = "Usage: touch [OPTION]... FILE...\nUpdate the access and modification times of each FILE to the current time.\nA FILE argument that does not exist is created empty, unless -c is supplied.\n\n  -a\t\t\tchange only the access time (same as -m: the VFS keeps one timestamp)\n  -c, --no-create\tdo not create any files\n  -d, --date=STRING\tparse STRING and use it instead of current time\n  -f\t\t\t(ignored)\n  -h, --no-dereference\taffect each symbolic link instead of any referenced file\n  -m\t\t\tchange only the modification time\n  -r, --reference=FILE\tuse this file's times instead of current time\n  -t STAMP\t\tuse [[CC]YY]MMDDhhmm[.ss] instead of current time\n      --time=WORD\tchange the specified time (accepted)\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n";

#[async_trait]
impl Builtin for Touch {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) =
            super::check_help_version(ctx.args, TOUCH_HELP, Some("touch (bashkit) 0.1"))
        {
            return Ok(r);
        }

        let fail = |msg: String| Ok(ExecResult::err(msg, 1));
        let mut files = Vec::new();
        let mut target_time = SystemTime::now();
        let mut no_create = false;
        let mut reference: Option<String> = None;
        let mut args = ctx.args.iter();
        while let Some(arg) = args.next() {
            let a = arg.as_str();
            if a == "--" {
                files.extend(args.by_ref());
                break;
            }
            if !a.starts_with('-') || a == "-" {
                files.push(arg);
                continue;
            }
            if let Some(long) = a.strip_prefix("--") {
                let (key, inline) = match long.split_once('=') {
                    Some((k, v)) => (k, Some(v.to_string())),
                    None => (long, None),
                };
                let mut value = || inline.clone().or_else(|| args.next().cloned());
                match key {
                    "no-create" => no_create = true,
                    "no-dereference" => {}
                    "time" => {
                        value();
                    }
                    "date" => {
                        let Some(v) = value() else {
                            return fail("touch: option '--date' requires an argument\n".into());
                        };
                        match self.clock.parse_date(ctx.env.get("TZ"), &v) {
                            Ok(dt) => target_time = crate::time_compat::from_chrono(dt),
                            Err(_) => return fail(format!("touch: invalid date format '{v}'\n")),
                        }
                    }
                    "reference" => reference = value(),
                    _ => return fail(format!("touch: unrecognized option '{a}'\n")),
                }
                continue;
            }
            let chars: Vec<char> = a[1..].chars().collect();
            for (idx, c) in chars.iter().enumerate() {
                let rest: String = chars[idx + 1..].iter().collect();
                let mut value = || {
                    if rest.is_empty() {
                        args.next().cloned()
                    } else {
                        Some(rest.clone())
                    }
                };
                match c {
                    'a' | 'm' | 'f' | 'h' => continue,
                    'c' => {
                        no_create = true;
                        continue;
                    }
                    'd' => {
                        let Some(v) = value() else {
                            return fail("touch: option requires an argument -- 'd'\n".into());
                        };
                        match self.clock.parse_date(ctx.env.get("TZ"), &v) {
                            Ok(dt) => target_time = crate::time_compat::from_chrono(dt),
                            Err(_) => return fail(format!("touch: invalid date format '{v}'\n")),
                        }
                    }
                    't' => {
                        let Some(v) = value() else {
                            return fail("touch: option requires an argument -- 't'\n".into());
                        };
                        match parse_touch_timestamp(&v) {
                            Ok(parsed) => target_time = parsed,
                            Err(err) => return fail(err),
                        }
                    }
                    'r' => {
                        let Some(v) = value() else {
                            return fail("touch: option requires an argument -- 'r'\n".into());
                        };
                        reference = Some(v);
                    }
                    other => return fail(format!("touch: invalid option -- '{other}'\n")),
                }
                break;
            }
        }

        if let Some(r) = reference {
            match ctx.fs.stat(&resolve_path(ctx.cwd, &r)).await {
                Ok(meta) => target_time = meta.modified,
                Err(_) => {
                    return fail(format!(
                        "touch: failed to get attributes of '{r}': No such file or directory\n"
                    ));
                }
            }
        }

        if files.is_empty() {
            return fail("touch: missing file operand\n".to_string());
        }

        for file in files {
            let path = resolve_path(ctx.cwd, file);

            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                if no_create {
                    continue;
                }
                if let Err(e) = ctx.fs.write_file(&path, &[]).await {
                    return fail(format!("touch: cannot touch '{}': {}\n", file, e));
                }
            }

            if let Err(e) = ctx.fs.set_modified_time(&path, target_time).await {
                return fail(format!("touch: cannot touch '{}': {}\n", file, e));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The chmod builtin - change file mode bits.
///
/// Usage: chmod MODE FILE...
///
/// MODE can be octal (e.g., 755) or symbolic (e.g., u+x, a+r, go-w)
pub struct Chmod;

/// Parse a symbolic mode string and apply it to an existing mode.
/// Handles: [ugoa]*[+-=][rwxXst]+ (comma-separated clauses).
/// Examples: +x, u+x, a+r, go-w, u=rwx, ug+rw
pub(super) fn apply_symbolic_mode(mode_str: &str, current_mode: u32) -> Option<u32> {
    let mut mode = current_mode;

    for clause in mode_str.split(',') {
        let clause = clause.trim();
        if clause.is_empty() {
            return None;
        }

        let mut chars = clause.chars().peekable();

        // Parse who: u, g, o, a (default = a if none specified)
        let mut who_u = false;
        let mut who_g = false;
        let mut who_o = false;
        let mut has_who = false;
        while let Some(&c) = chars.peek() {
            match c {
                'u' => {
                    who_u = true;
                    has_who = true;
                    chars.next();
                }
                'g' => {
                    who_g = true;
                    has_who = true;
                    chars.next();
                }
                'o' => {
                    who_o = true;
                    has_who = true;
                    chars.next();
                }
                'a' => {
                    who_u = true;
                    who_g = true;
                    who_o = true;
                    has_who = true;
                    chars.next();
                }
                _ => break,
            }
        }
        // No who specified means all (a)
        if !has_who {
            who_u = true;
            who_g = true;
            who_o = true;
        }

        // Parse operator: +, -, =
        let op = chars.next()?;
        if op != '+' && op != '-' && op != '=' {
            return None;
        }

        // Parse permissions: r, w, x, X, s, t
        let mut perm_bits: u32 = 0;
        for c in chars {
            match c {
                'r' => perm_bits |= 0o4,
                'w' => perm_bits |= 0o2,
                'x' => perm_bits |= 0o1,
                'X' => {
                    // +X: set execute only if it's a directory or already has execute
                    if current_mode & 0o111 != 0 {
                        perm_bits |= 0o1;
                    }
                }
                's' | 't' => {} // setuid/setgid/sticky: accept but ignore for VFS
                _ => return None,
            }
        }

        // Build mask for affected bits
        let mut mask: u32 = 0;
        let mut bits: u32 = 0;
        if who_u {
            mask |= 0o700;
            bits |= perm_bits << 6;
        }
        if who_g {
            mask |= 0o070;
            bits |= perm_bits << 3;
        }
        if who_o {
            mask |= 0o007;
            bits |= perm_bits;
        }

        match op {
            '+' => mode |= bits,
            '-' => mode &= !bits,
            '=' => mode = (mode & !mask) | bits,
            _ => unreachable!(),
        }
    }

    Some(mode)
}

#[async_trait]
impl Builtin for Chmod {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: chmod [OPTION]... MODE[,MODE]... FILE...\nChange the mode of each FILE to MODE.\nMODE can be octal (e.g., 755) or symbolic (e.g., u+x, a+r, go-w).\n\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n",
            Some("chmod (bashkit) 0.1"),
        ) {
            return Ok(r);
        }

        if ctx.args.len() < 2 {
            return Ok(ExecResult::err("chmod: missing operand\n".to_string(), 1));
        }

        let mode_str = &ctx.args[0];
        let files = &ctx.args[1..];

        // Try octal first, then symbolic
        let is_octal = u32::from_str_radix(mode_str, 8).is_ok();

        for file in files.iter().filter(|a| !a.starts_with('-')) {
            let path = resolve_path(ctx.cwd, file);

            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!(
                        "chmod: cannot access '{}': No such file or directory\n",
                        file
                    ),
                    1,
                ));
            }

            let mode = if is_octal {
                u32::from_str_radix(mode_str, 8)
                    .expect("from_str_radix valid: is_octal confirmed by is_ok() check above")
            } else {
                // Symbolic mode - need current permissions
                let current_mode = match ctx.fs.stat(&path).await {
                    Ok(meta) => meta.mode,
                    Err(_) => 0o644, // fallback default
                };
                match apply_symbolic_mode(mode_str, current_mode) {
                    Some(m) => m,
                    None => {
                        return Ok(ExecResult::err(
                            format!("chmod: invalid mode: '{}'\n", mode_str),
                            1,
                        ));
                    }
                }
            };

            if let Err(e) = ctx.fs.chmod(&path, mode).await {
                return Ok(ExecResult::err(
                    format!("chmod: changing permissions of '{}': {}\n", file, e),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The ln builtin - create links.
///
/// Usage: ln [-s] [-f] TARGET LINK_NAME
///        ln [-s] [-f] TARGET... DIRECTORY
///
/// Options:
///   -s   Create symbolic link (default in Bashkit; hard links not supported in VFS)
///   -f   Force: remove existing destination files
///
/// Note: In Bashkit's virtual filesystem, all links are symbolic.
/// Hard links are not supported; `-s` is implied.
pub struct Ln;

#[async_trait]
impl Builtin for Ln {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: ln [OPTION]... TARGET LINK_NAME\nCreate a link to TARGET with the name LINK_NAME.\n\n  -s\t\tmake symbolic links instead of hard links\n  -f\t\tremove existing destination files\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n",
            Some("ln (bashkit) 0.1"),
        ) {
            return Ok(r);
        }

        let mut force = false;
        let mut files: Vec<&str> = Vec::new();

        for arg in ctx.args.iter() {
            if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        's' => {} // symbolic — always symbolic in VFS
                        'f' => force = true,
                        _ => {
                            return Ok(ExecResult::err(
                                format!("ln: invalid option -- '{}'\n", c),
                                1,
                            ));
                        }
                    }
                }
            } else {
                files.push(arg);
            }
        }

        if files.len() < 2 {
            return Ok(ExecResult::err("ln: missing file operand\n".to_string(), 1));
        }

        let target = files[0];
        let link_name = files[1];
        let link_path = resolve_path(ctx.cwd, link_name);

        // If link already exists
        if ctx.fs.exists(&link_path).await.unwrap_or(false) {
            if force {
                // Surface remove failures (e.g. non-empty directory) instead
                // of falling through and overwriting via symlink, which on
                // the in-memory VFS would orphan children and corrupt state
                // (issue #1577).
                if let Err(e) = ctx.fs.remove(&link_path, false).await {
                    return Ok(ExecResult::err(
                        format!("ln: cannot remove '{}': {}\n", link_name, e),
                        1,
                    ));
                }
            } else {
                return Ok(ExecResult::err(
                    format!(
                        "ln: failed to create symbolic link '{}': File exists\n",
                        link_name
                    ),
                    1,
                ));
            }
        }

        let target_path = Path::new(target);
        if let Err(e) = ctx.fs.symlink(target_path, &link_path).await {
            return Ok(ExecResult::err(
                format!(
                    "ln: failed to create symbolic link '{}': {}\n",
                    link_name, e
                ),
                1,
            ));
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The chown builtin - change file ownership (no-op in VFS).
///
/// Usage: chown [-R] OWNER[:GROUP] FILE...
///
/// In the virtual filesystem there are no real UIDs/GIDs, so chown is a no-op
/// that simply validates arguments and succeeds silently.
pub struct Chown;

#[async_trait]
impl Builtin for Chown {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: chown [OPTION]... OWNER[:GROUP] FILE...\nChange file owner and group.\n\n  -R, --recursive\toperate on files and directories recursively\n      --help\t\tdisplay this help and exit\n      --version\t\toutput version information and exit\n",
            Some("chown (bashkit) 0.1"),
        ) {
            return Ok(r);
        }

        // Reject unknown options like GNU chown; accept-and-ignore the rest
        // (ownership is a no-op in the VFS). '--' ends option parsing.
        let mut recursive = false;
        let mut positional: Vec<&str> = Vec::new();
        let mut opts_done = false;
        for arg in ctx.args {
            if opts_done {
                positional.push(arg);
            } else if arg == "--" {
                opts_done = true;
            } else if let Some(long) = arg.strip_prefix("--") {
                match long.split('=').next().unwrap_or("") {
                    "recursive" => recursive = true,
                    "changes" | "dereference" | "no-dereference" | "from" | "silent" | "quiet"
                    | "reference" | "verbose" => {}
                    _ => return Ok(super::invalid_option("chown", arg, 1)),
                }
            } else if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        'R' => recursive = true,
                        'c' | 'f' | 'h' | 'v' | 'H' | 'L' | 'P' => {}
                        _ => return Ok(super::invalid_option("chown", &format!("-{c}"), 1)),
                    }
                }
            } else {
                positional.push(arg);
            }
        }
        let _ = recursive; // accepted but irrelevant in VFS

        if positional.len() < 2 {
            return Ok(ExecResult::err("chown: missing operand\n".to_string(), 1));
        }

        // Validate that target files exist
        let _owner = positional[0]; // accepted but not applied
        for file in &positional[1..] {
            let path = resolve_path(ctx.cwd, file);
            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!(
                        "chown: cannot access '{}': No such file or directory\n",
                        file
                    ),
                    1,
                ));
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The mktemp builtin - create temporary files or directories.
///
/// Argument surface is generated from uutils/coreutils' `uu_app()` via
/// the `bashkit-coreutils-port` codegen tool — see
/// `generated/mktemp_args.rs`. Behaviour is implemented locally
/// against the bashkit VFS.
///
/// Options of note:
///   -d, --directory      Create a directory instead of a file.
///   -p / --tmpdir[=DIR]  Prefix dir (defaults to /tmp).
///   -u, --dry-run        Print the would-be path without creating it.
///   --suffix=SUFFIX      Append SUFFIX after the template's `X`s.
///   -q, --quiet          Suppress error diagnostics on failure.
pub struct Mktemp;

/// `len` random `[A-Za-z0-9]` characters, re-seeded per attempt.
fn mktemp_random(attempt: usize, len: usize) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let state = RandomState::new();
    (0..len)
        .map(|i| {
            let mut hasher = state.build_hasher();
            hasher.write_usize(attempt);
            hasher.write_usize(i);
            CHARS[(hasher.finish() % CHARS.len() as u64) as usize] as char
        })
        .collect()
}

/// Split TEMPLATE into (prefix, X count, suffix) the GNU way: without
/// `--suffix`, the suffix is whatever follows the last `X`.
fn mktemp_split<'a>(
    template: &'a str,
    suffix: Option<&'a str>,
) -> std::result::Result<(&'a str, usize, &'a str), String> {
    let (body, suffix) = match suffix {
        Some(suf) => (template, suf),
        None => match template.rfind('X') {
            Some(i) => (&template[..=i], &template[i + 1..]),
            None => (template, ""),
        },
    };
    let xs = body.bytes().rev().take_while(|b| *b == b'X').count();
    if xs < 3 {
        return Err(format!("mktemp: too few X's in template '{template}'\n"));
    }
    Ok((&body[..body.len() - xs], xs, suffix))
}

// Cached `mktemp` arg surface: pre-built once, cloned per invocation.
// See `builtins::clap_cache` for why it is built, not just constructed.
cached_command!(mktemp_cmd, super::generated::mktemp_args::mktemp_command());

#[async_trait]
impl Builtin for Mktemp {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        use std::ffi::OsString;
        use std::path::PathBuf;

        let argv: Vec<OsString> = std::iter::once(OsString::from("mktemp"))
            .chain(ctx.args.iter().map(OsString::from))
            .collect();

        let matches = match mktemp_cmd().try_get_matches_from(argv) {
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

        let create_dir = matches.get_flag("directory");
        let dry_run = matches.get_flag("dry-run");
        let quiet = matches.get_flag("quiet");
        let use_t = matches.get_flag("t");
        // -p and --tmpdir parse to Option<PathBuf>; an empty value means
        // "use $TMPDIR or /tmp".
        let p_value: Option<&Option<PathBuf>> = matches.get_one("p");
        let tmpdir_value: Option<&Option<PathBuf>> = matches.get_one("tmpdir");
        let dir_arg: Option<String> = match (tmpdir_value, p_value) {
            (Some(Some(p)), _) | (_, Some(Some(p))) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        }
        .filter(|d| !d.is_empty());
        let suffix_arg = matches
            .get_one::<OsString>("suffix")
            .map(|s| s.to_string_lossy().into_owned());
        let template_arg: Option<String> = matches
            .get_one::<OsString>("template")
            .map(|s| s.to_string_lossy().into_owned());

        let fail = |msg: String| ExecResult::err(if quiet { String::new() } else { msg }, 1);

        // GNU: no TEMPLATE implies --tmpdir; -t prefers $TMPDIR over -p.
        let use_dest_dir =
            template_arg.is_none() || p_value.is_some() || tmpdir_value.is_some() || use_t;
        let template = template_arg.unwrap_or_else(|| "tmp.XXXXXXXXXX".to_string());
        if suffix_arg.as_deref().is_some_and(|s| s.contains('/')) {
            return Ok(fail(format!(
                "mktemp: invalid suffix '{}', contains directory separator\n",
                suffix_arg.as_deref().unwrap_or_default()
            )));
        }
        let (prefix, xs, suffix) = match mktemp_split(&template, suffix_arg.as_deref()) {
            Ok(parts) => parts,
            Err(msg) => return Ok(fail(msg)),
        };
        let env_tmpdir = ctx.env.get("TMPDIR").filter(|d| !d.is_empty()).cloned();
        let dest_dir = if !use_dest_dir {
            None
        } else if use_t {
            Some(env_tmpdir.or(dir_arg).unwrap_or_else(|| "/tmp".to_string()))
        } else {
            Some(dir_arg.or(env_tmpdir).unwrap_or_else(|| "/tmp".to_string()))
        };
        if dest_dir.is_some() && template.starts_with('/') {
            return Ok(fail(format!(
                "mktemp: invalid template, '{template}'; with --tmpdir, it may not be absolute\n"
            )));
        }
        if use_t && template.contains('/') {
            return Ok(fail(format!(
                "mktemp: invalid template, '{template}', contains directory separator\n"
            )));
        }
        // Sandbox convenience: a VFS without /tmp still gets the default
        // directory (an explicit -p/TMPDIR directory must already exist).
        if dest_dir.as_deref() == Some("/tmp") {
            let tmp = std::path::Path::new("/tmp");
            if !ctx.fs.exists(tmp).await.unwrap_or(false) {
                let _ = ctx.fs.mkdir(tmp, true).await;
            }
        }
        let shown_template = match &dest_dir {
            Some(dir) => format!("{}/{template}", dir.trim_end_matches('/')),
            None => template.clone(),
        };

        for attempt in 0..MKTEMP_MAX_ATTEMPTS {
            let name = format!("{prefix}{}{suffix}", mktemp_random(attempt, xs));
            // Shown as built (relative stays relative); created under cwd.
            let shown = match &dest_dir {
                Some(dir) => format!("{}/{name}", dir.trim_end_matches('/')),
                None => name,
            };
            let full_path = resolve_path(ctx.cwd, &shown);

            if ctx.fs.exists(&full_path).await.unwrap_or(false) {
                continue;
            }
            if dry_run {
                return Ok(ExecResult::ok(format!("{shown}\n")));
            }
            let created = if create_dir {
                ctx.fs.mkdir(&full_path, false).await
            } else {
                ctx.fs.write_file(&full_path, &[]).await
            };
            match created {
                Ok(_) => return Ok(ExecResult::ok(format!("{shown}\n"))),
                Err(_) if ctx.fs.exists(&full_path).await.unwrap_or(false) => continue,
                Err(_) => {
                    let what = if create_dir { "directory" } else { "file" };
                    let parent_missing = match full_path.parent() {
                        Some(p) => !ctx.fs.exists(p).await.unwrap_or(false),
                        None => false,
                    };
                    let reason = if parent_missing {
                        "No such file or directory"
                    } else {
                        "Permission denied"
                    };
                    return Ok(fail(format!(
                        "mktemp: failed to create {what} via template '{shown_template}': {reason}\n"
                    )));
                }
            }
        }

        Ok(fail(format!(
            "mktemp: failed to create file via template '{shown_template}': File exists\n"
        )))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn create_test_ctx() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();

        // Create the cwd
        fs.mkdir(&cwd, true).await.unwrap();

        (fs, cwd, variables)
    }

    #[tokio::test]
    async fn test_mkdir_simple() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["testdir".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Mkdir.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("testdir")).await.unwrap());
    }

    #[tokio::test]
    async fn test_mkdir_recursive() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-p".to_string(), "a/b/c".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Mkdir.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("a/b/c")).await.unwrap());
    }

    #[tokio::test]
    async fn test_touch_create() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["newfile.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Touch::default().execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("newfile.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_touch_t_sets_existing_file_mtime() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();
        let file = cwd.join("existing.txt");
        fs.write_file(&file, b"content").await.unwrap();

        let args = vec![
            "-t".to_string(),
            "202604061200.00".to_string(),
            "existing.txt".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Touch::default().execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        let metadata = fs.stat(&file).await.unwrap();
        let modified = crate::time_compat::to_chrono_utc(metadata.modified);
        assert_eq!(modified.year(), 2026);
        assert_eq!(modified.month(), 4);
        assert_eq!(modified.day(), 6);
        assert_eq!(modified.hour(), 12);
        assert_eq!(modified.minute(), 0);
        assert_eq!(modified.second(), 0);
    }

    #[tokio::test]
    async fn test_touch_t_rejects_invalid_timestamp() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec![
            "-t".to_string(),
            "not-a-timestamp".to_string(),
            "existing.txt".to_string(),
        ];
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

        let result = Touch::default().execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid date format"));
    }

    #[tokio::test]
    async fn test_rm_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create a file first
        fs.write_file(&cwd.join("testfile.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["testfile.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Rm.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!fs.exists(&cwd.join("testfile.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_rm_force_nonexistent() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-f".to_string(), "nonexistent".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Rm.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0); // No error with -f
    }

    #[tokio::test]
    async fn test_cp_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create source file
        fs.write_file(&cwd.join("source.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["source.txt".to_string(), "dest.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Cp.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("dest.txt")).await.unwrap());

        let content = fs.read_file(&cwd.join("dest.txt")).await.unwrap();
        assert_eq!(content, b"content");
    }

    #[tokio::test]
    async fn test_mv_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create source file
        fs.write_file(&cwd.join("source.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["source.txt".to_string(), "dest.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Mv.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!fs.exists(&cwd.join("source.txt")).await.unwrap());
        assert!(fs.exists(&cwd.join("dest.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_chmod_octal() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create a file
        fs.write_file(&cwd.join("script.sh"), b"#!/bin/bash")
            .await
            .unwrap();

        let args = vec!["755".to_string(), "script.sh".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Chmod.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        let meta = fs.stat(&cwd.join("script.sh")).await.unwrap();
        assert_eq!(meta.mode, 0o755);
    }

    /// Regression: issue #1577. `ln -f` over a non-empty directory must
    /// surface the underlying remove failure instead of silently
    /// proceeding to symlink creation, which on the in-memory VFS would
    /// orphan the directory's children.
    #[tokio::test]
    async fn test_ln_force_over_non_empty_dir_fails() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Set up: target file + non-empty destination directory.
        fs.write_file(&cwd.join("target.txt"), b"content")
            .await
            .unwrap();
        fs.mkdir(&cwd.join("destdir"), false).await.unwrap();
        fs.write_file(&cwd.join("destdir/child.txt"), b"x")
            .await
            .unwrap();

        let args = vec![
            "-sf".to_string(),
            "target.txt".to_string(),
            "destdir".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Ln.execute(ctx).await.unwrap();
        assert_ne!(
            result.exit_code, 0,
            "ln -sf must fail when destination is a non-empty directory"
        );
        // Child must still exist; ln must not have proceeded to symlink.
        assert!(
            fs.exists(&cwd.join("destdir/child.txt")).await.unwrap(),
            "child file must not be orphaned"
        );
    }

    #[test]
    fn test_mktemp_split_template() {
        assert_eq!(
            mktemp_split("/tmp/myapp.XXXXXX", None),
            Ok(("/tmp/myapp.", 6, ""))
        );
        assert_eq!(mktemp_split("aXXXb.txt", None), Ok(("a", 3, "b.txt")));
        assert_eq!(mktemp_split("aXXXX", Some(".c")), Ok(("a", 4, ".c")));
        assert!(mktemp_split("/tmp/myapp", None).is_err());
        assert!(mktemp_split("aXX", None).is_err());
    }

    #[test]
    fn test_mktemp_random_charset() {
        let r = mktemp_random(0, 32);
        assert_eq!(r.len(), 32);
        assert!(r.bytes().all(|b| b.is_ascii_alphanumeric()));
    }

    async fn run_fileop<B: Builtin>(builtin: &B, args: &[&str]) -> ExecResult {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };
        builtin.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_mkdir_rejects_unknown_option() {
        let result = run_fileop(&Mkdir, &["-Q", "dir"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_rm_rejects_unknown_option() {
        let result = run_fileop(&Rm, &["-Q", "file"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_cp_rejects_unknown_option() {
        let result = run_fileop(&Cp, &["-Q", "a", "b"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_mv_rejects_unknown_option() {
        let result = run_fileop(&Mv, &["-Q", "a", "b"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_chown_rejects_unknown_option() {
        let result = run_fileop(&Chown, &["-Q", "user", "file"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }
}
