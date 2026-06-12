//! Path manipulation builtins - basename, dirname

use async_trait::async_trait;
use std::path::Path;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The basename builtin - strip directory and suffix from filenames.
///
/// Usage: basename NAME [SUFFIX]
///        basename OPTION... NAME...
///
/// Print NAME with any leading directory components removed.
/// If SUFFIX is specified, also remove a trailing SUFFIX.
pub struct Basename;

#[async_trait]
impl Builtin for Basename {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: basename NAME [SUFFIX]\nPrint NAME with leading directory components removed.\nIf SUFFIX is specified, also remove a trailing SUFFIX.\n\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("basename (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "basename: missing operand\n".to_string(),
                1,
            ));
        }

        let mut output = String::new();
        let mut args_iter = ctx.args.iter();

        // Get the path argument
        let path_arg = args_iter
            .next()
            .expect("args_iter.next() valid: guarded by is_empty() check above");
        let path = Path::new(path_arg);

        // Get the basename
        let basename = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| {
                // Handle special cases like "/" or empty
                if path_arg == "/" {
                    "/".to_string()
                } else if path_arg.is_empty() {
                    String::new()
                } else {
                    path_arg.clone()
                }
            });

        // Check for suffix argument
        let result = if let Some(suffix) = args_iter.next() {
            if let Some(stripped) = basename.strip_suffix(suffix.as_str()) {
                stripped.to_string()
            } else {
                basename
            }
        } else {
            basename
        };

        output.push_str(&result);
        output.push('\n');

        Ok(ExecResult::ok(output))
    }
}

/// The dirname builtin - strip last component from file name.
///
/// Usage: dirname NAME...
///
/// Output each NAME with its last non-slash component and trailing slashes removed.
/// If NAME contains no slashes, output "." (current directory).
pub struct Dirname;

#[async_trait]
impl Builtin for Dirname {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: dirname NAME...\nOutput each NAME with its last non-slash component and trailing slashes removed.\nIf NAME contains no slashes, output '.' (current directory).\n\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("dirname (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        if ctx.args.is_empty() {
            return Ok(ExecResult::err("dirname: missing operand\n".to_string(), 1));
        }

        let mut output = String::new();

        for (i, arg) in ctx.args.iter().enumerate() {
            if i > 0 {
                output.push('\n');
            }

            let path = Path::new(arg);
            let dirname = path
                .parent()
                .map(|p| {
                    let s = p.to_string_lossy();
                    if s.is_empty() {
                        ".".to_string()
                    } else {
                        s.to_string()
                    }
                })
                .unwrap_or_else(|| {
                    // Handle special cases
                    if arg == "/" {
                        "/".to_string()
                    } else {
                        ".".to_string()
                    }
                });

            output.push_str(&dirname);
        }

        output.push('\n');
        Ok(ExecResult::ok(output))
    }
}

/// The realpath builtin - resolve absolute pathname.
///
/// Argument surface is generated from uutils/coreutils' `uu_app()` via
/// the `bashkit-coreutils-port` codegen tool — see
/// `generated/realpath_args.rs`. Behaviour is implemented locally
/// against the bashkit VFS.
///
/// Symlink resolution stays disabled per L-FS-001 in `specs/limitations.md`'s
/// "Intentionally Unimplemented" entry: bashkit's VFS does not model
/// symlinks, so `-e`/`-m` only differ on existence checks, and
/// `-L`/`-P`/`--strip` collapse to the same lexical canonicalisation.
pub struct Realpath;

#[async_trait]
impl Builtin for Realpath {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        use super::generated::realpath_args::realpath_command;
        use std::ffi::OsString;

        let argv: Vec<OsString> = std::iter::once(OsString::from("realpath"))
            .chain(ctx.args.iter().map(OsString::from))
            .collect();

        let cmd = realpath_command().help_template("Usage: {usage}\n{about}\n\n{all-args}\n");
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

        let quiet = matches.get_flag("quiet");
        let zero = matches.get_flag("zero");
        let canonicalize_existing = matches.get_flag("canonicalize-existing");
        let relative_to = matches
            .get_one::<OsString>("relative-to")
            .map(|s| s.to_string_lossy().into_owned());
        let relative_base = matches
            .get_one::<OsString>("relative-base")
            .map(|s| s.to_string_lossy().into_owned());

        let files: Vec<String> = matches
            .get_many::<OsString>("files")
            .map(|vs| vs.map(|v| v.to_string_lossy().into_owned()).collect())
            .unwrap_or_default();

        let separator = if zero { '\0' } else { '\n' };
        let mut output = String::new();
        let mut had_error = false;

        for arg in &files {
            let resolved = super::resolve_path(ctx.cwd, arg);

            // -e: every component must exist.
            // -m (default in bashkit): no existence requirement.
            // -E (default upstream): only the parent must exist —
            //   without symlinks the distinction between -E and -m is
            //   purely existence-based, so we treat them identically.
            if canonicalize_existing && !ctx.fs.exists(&resolved).await.unwrap_or(false) {
                had_error = true;
                if !quiet {
                    return Ok(ExecResult::err(
                        format!(
                            "realpath: {}: No such file or directory\n",
                            resolved.display()
                        ),
                        1,
                    ));
                }
                continue;
            }

            let mut printed = resolved.to_string_lossy().into_owned();
            if let Some(base) = relative_to.as_ref().or(relative_base.as_ref()) {
                let base_path = super::resolve_path(ctx.cwd, base);
                if let Ok(rel) = std::path::Path::new(&printed)
                    .strip_prefix(base_path.to_string_lossy().as_ref())
                {
                    let rel_str = rel.to_string_lossy();
                    printed = if rel_str.is_empty() {
                        ".".to_string()
                    } else {
                        rel_str.into_owned()
                    };
                }
            }
            output.push_str(&printed);
            output.push(separator);
        }

        if had_error {
            return Ok(ExecResult::err(output, 1));
        }
        Ok(ExecResult::ok(output))
    }
}

/// The readlink builtin - print resolved symbolic links or canonical file names.
///
/// Argument surface is generated from uutils/coreutils' `uu_app()` via the
/// `bashkit-coreutils-port` codegen tool — see `generated/readlink_args.rs`.
/// Behaviour stays local against the bashkit VFS.
///
/// Canonical modes follow VFS symlinks with a depth cap to avoid loops while
/// preserving sandbox boundaries enforced by the filesystem backend.
///
/// Usage: readlink [-f|-m|-e] FILE...
///
/// Options:
///   -f    canonicalize: follow symlinks, resolve `.`/`..`; all but last component must exist
///   -m    canonicalize-missing: like -f but no component needs to exist
///   -e    canonicalize-existing: like -f but all components must exist
///   (no flag) print symlink target without canonicalization
pub struct Readlink;

#[async_trait]
impl Builtin for Readlink {
    #[allow(clippy::collapsible_if)]
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let argv: Vec<std::ffi::OsString> = std::iter::once(std::ffi::OsString::from("readlink"))
            .chain(ctx.args.iter().map(std::ffi::OsString::from))
            .collect();

        let cmd = super::generated::readlink_args::readlink_command()
            .help_template("Usage: {usage}\n{about}\n\n{all-args}\n");
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

        // -e/-m/-f are mutually exclusive in spirit; check most-restrictive
        // first, matching uutils' precedence.
        let mode = if matches.get_flag("canonicalize-existing") {
            ReadlinkMode::CanonicalizeExisting
        } else if matches.get_flag("canonicalize-missing") {
            ReadlinkMode::CanonicalizeMissing
        } else if matches.get_flag("canonicalize") {
            ReadlinkMode::Canonicalize
        } else {
            ReadlinkMode::Raw
        };

        // -n suppresses the trailing terminator entirely; -z swaps it
        // to NUL. Both can come from the codegen-generated args now
        // that the parser handles them; the previous handwritten path
        // silently accepted -n as a no-op, so honoring it is a strict
        // improvement.
        let suppress_terminator = matches.get_flag("no-newline");
        let zero_terminated = matches.get_flag("zero");
        let terminator: char = if zero_terminated { '\0' } else { '\n' };

        let files: Vec<String> = matches
            .get_many::<std::ffi::OsString>("files")
            .map(|vs| vs.map(|v| v.to_string_lossy().into_owned()).collect())
            .unwrap_or_default();

        if files.is_empty() {
            return Ok(ExecResult::err(
                "readlink: missing operand\n".to_string(),
                1,
            ));
        }

        let mut output = String::new();
        let mut exit_code = 0;
        let total_files = files.len();

        for (idx, file) in files.iter().enumerate() {
            let resolved = super::resolve_path(ctx.cwd, file);
            let is_last = idx + 1 == total_files;
            let needs_terminator = !(suppress_terminator && is_last);

            match mode {
                ReadlinkMode::Raw => {
                    // No flag: read symlink target
                    match ctx.fs.read_link(&resolved).await {
                        Ok(target) => {
                            output.push_str(&target.to_string_lossy());
                            if needs_terminator {
                                output.push(terminator);
                            }
                        }
                        Err(_) => {
                            exit_code = 1;
                        }
                    }
                }
                ReadlinkMode::Canonicalize
                | ReadlinkMode::CanonicalizeMissing
                | ReadlinkMode::CanonicalizeExisting => {
                    if let Some(canonical) =
                        canonicalize_readlink_path(ctx.fs.as_ref(), ctx.cwd, file, mode).await
                    {
                        output.push_str(&canonical.to_string_lossy());
                        if needs_terminator {
                            output.push(terminator);
                        }
                    } else {
                        exit_code = 1;
                    }
                }
            }
        }

        if exit_code != 0 && output.is_empty() {
            Ok(ExecResult::err(String::new(), exit_code))
        } else if exit_code != 0 {
            // Some files succeeded, some failed
            let mut result = ExecResult::with_code(output, exit_code);
            result.exit_code = exit_code;
            Ok(result)
        } else {
            Ok(ExecResult::ok(output))
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ReadlinkMode {
    Raw,
    Canonicalize,
    CanonicalizeMissing,
    CanonicalizeExisting,
}

const READLINK_MAX_SYMLINK_DEPTH: usize = 40;

fn readlink_path_within_limits(fs: &dyn crate::fs::FileSystem, path: &Path) -> bool {
    // THREAT[TM-DOS-013]: Canonicalization may compose symlink targets plus
    // remaining components repeatedly; keep every intermediate path inside the
    // VFS path budget before collecting components or normalizing again.
    //
    // Validate against both the active filesystem's configured limits (which
    // may be stricter than the default) and the default bounded limits — the
    // latter still caps unlimited backends (e.g. RealFs reports
    // `FsLimits::unlimited()`), so the effective bound is the intersection.
    fs.limits().validate_path(path).is_ok()
        && crate::fs::FsLimits::default().validate_path(path).is_ok()
}

async fn canonicalize_readlink_path(
    fs: &dyn crate::fs::FileSystem,
    cwd: &Path,
    file: &str,
    mode: ReadlinkMode,
) -> Option<std::path::PathBuf> {
    let canonical = follow_readlink_symlinks(fs, super::resolve_path(cwd, file)).await?;

    match mode {
        ReadlinkMode::CanonicalizeMissing => Some(canonical),
        ReadlinkMode::Canonicalize => {
            let parent = canonical
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("/"));
            fs.exists(parent)
                .await
                .unwrap_or(false)
                .then_some(canonical)
        }
        ReadlinkMode::CanonicalizeExisting => fs
            .exists(&canonical)
            .await
            .unwrap_or(false)
            .then_some(canonical),
        ReadlinkMode::Raw => None,
    }
}

async fn follow_readlink_symlinks(
    fs: &dyn crate::fs::FileSystem,
    path: std::path::PathBuf,
) -> Option<std::path::PathBuf> {
    let mut path = path;
    let mut depth = 0;

    loop {
        if depth > READLINK_MAX_SYMLINK_DEPTH || !readlink_path_within_limits(fs, &path) {
            return None;
        }

        let normalized = crate::fs::normalize_path(&path);
        if !readlink_path_within_limits(fs, &normalized) {
            return None;
        }
        let components: Vec<_> = normalized.components().collect();
        let mut current = std::path::PathBuf::from("/");
        let mut followed = false;

        for (idx, component) in components.iter().enumerate() {
            use std::path::Component;

            match component {
                Component::RootDir => continue,
                Component::Normal(part) => current.push(part),
                _ => continue,
            }

            if let Ok(target) = fs.read_link(&current).await {
                depth += 1;
                if !readlink_path_within_limits(fs, &target) {
                    return None;
                }
                let link_parent = current.parent().unwrap_or_else(|| Path::new("/"));
                let mut next = if target.is_absolute() {
                    target
                } else {
                    link_parent.join(target)
                };

                for remaining in components.iter().skip(idx + 1) {
                    if let Component::Normal(part) = remaining {
                        next.push(part);
                    }
                }

                if !readlink_path_within_limits(fs, &next) {
                    return None;
                }
                path = crate::fs::normalize_path(&next);
                followed = true;
                break;
            }
        }

        if !followed {
            return Some(normalized);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_basename(args: &[&str]) -> ExecResult {
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
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Basename.execute(ctx).await.unwrap()
    }

    async fn run_dirname(args: &[&str]) -> ExecResult {
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
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Dirname.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_basename_simple() {
        let result = run_basename(&["/usr/bin/sort"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "sort\n");
    }

    #[tokio::test]
    async fn test_basename_with_suffix() {
        let result = run_basename(&["file.txt", ".txt"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "file\n");
    }

    #[tokio::test]
    async fn test_basename_no_suffix_match() {
        let result = run_basename(&["file.txt", ".doc"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "file.txt\n");
    }

    #[tokio::test]
    async fn test_basename_no_dir() {
        let result = run_basename(&["filename"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "filename\n");
    }

    #[tokio::test]
    async fn test_basename_trailing_slash() {
        let result = run_basename(&["/usr/bin/"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "bin\n");
    }

    #[tokio::test]
    async fn test_basename_missing_operand() {
        let result = run_basename(&[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing operand"));
    }

    #[tokio::test]
    async fn test_dirname_simple() {
        let result = run_dirname(&["/usr/bin/sort"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/usr/bin\n");
    }

    #[tokio::test]
    async fn test_dirname_no_dir() {
        let result = run_dirname(&["filename"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, ".\n");
    }

    #[tokio::test]
    async fn test_dirname_root() {
        let result = run_dirname(&["/"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/\n");
    }

    #[tokio::test]
    async fn test_dirname_trailing_slash() {
        let result = run_dirname(&["/usr/bin/"]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/usr\n");
    }

    #[tokio::test]
    async fn test_dirname_missing_operand() {
        let result = run_dirname(&[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing operand"));
    }

    // readlink tests

    use crate::fs::FileSystem;

    async fn run_readlink_with_fs(args: &[&str], fs: Arc<dyn FileSystem>) -> ExecResult {
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

        Readlink.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_readlink_missing_operand() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_readlink_with_fs(&[], fs).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing operand"));
    }

    #[tokio::test]
    async fn test_readlink_raw_symlink() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.symlink(Path::new("/target"), Path::new("/link"))
            .await
            .unwrap();
        let result = run_readlink_with_fs(&["/link"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/target\n");
    }

    #[tokio::test]
    async fn test_readlink_raw_not_symlink() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.write_file(Path::new("/file"), b"data").await.unwrap(); // write a regular file
        let result = run_readlink_with_fs(&["/file"], fs).await;
        // Not a symlink → failure, no output
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_readlink_raw_nonexistent() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_readlink_with_fs(&["/nonexistent"], fs).await;
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_readlink_f_canonicalize() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.mkdir(Path::new("/home"), true).await.unwrap();
        fs.mkdir(Path::new("/home/user"), true).await.unwrap();
        let result = run_readlink_with_fs(&["-f", "/home/user/../user/./file"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/home/user/file\n");
    }

    #[tokio::test]
    async fn test_readlink_m_canonicalize_missing() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        // -m doesn't require existence
        let result = run_readlink_with_fs(&["-m", "/a/b/../c"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/a/c\n");
    }

    #[tokio::test]
    async fn test_readlink_f_canonicalize_root() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_readlink_with_fs(&["-f", "/"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/\n");
    }

    #[tokio::test]
    async fn test_readlink_f_canonicalize_follows_symlink() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.mkdir(Path::new("/target"), false).await.unwrap();
        fs.symlink(Path::new("/target/file"), Path::new("/link"))
            .await
            .unwrap();

        let result = run_readlink_with_fs(&["-f", "/link"], fs).await;

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/target/file\n");
    }

    #[tokio::test]
    async fn test_readlink_m_canonicalize_follows_dangling_symlink() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.symlink(Path::new("/missing/../target"), Path::new("/link"))
            .await
            .unwrap();

        let result = run_readlink_with_fs(&["-m", "/link"], fs).await;

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/target\n");
    }

    #[tokio::test]
    async fn test_readlink_e_canonicalize_follows_intermediate_symlink() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.mkdir(Path::new("/real"), false).await.unwrap();
        fs.write_file(Path::new("/real/file"), b"data")
            .await
            .unwrap();
        fs.symlink(Path::new("/real"), Path::new("/dirlink"))
            .await
            .unwrap();

        let result = run_readlink_with_fs(&["-e", "/dirlink/file"], fs).await;

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/real/file\n");
    }

    #[tokio::test]
    async fn test_readlink_canonicalize_rejects_symlink_loop() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.symlink(Path::new("/b"), Path::new("/a")).await.unwrap();
        fs.symlink(Path::new("/a"), Path::new("/b")).await.unwrap();

        let result = run_readlink_with_fs(&["-m", "/a"], fs).await;

        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_readlink_canonicalize_rejects_growing_symlink_path() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let target = format!("/a/{}", "x".repeat(128));
        fs.symlink(Path::new(&target), Path::new("/a"))
            .await
            .unwrap();

        let result = run_readlink_with_fs(&["-m", "/a"], fs).await;

        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_readlink_respects_stricter_filesystem_path_limit() {
        // `/a` -> `/bbb...(40)`; canonicalizing `/a/ccc...(15)` composes a
        // ~57-byte path. That fits under the default 4096 budget but exceeds a
        // stricter per-filesystem limit, which must be honored.
        let link_target = format!("/{}", "b".repeat(40));
        let query = format!("/a/{}", "c".repeat(15));

        // Stricter fs: composed path exceeds max_path_length(50) -> rejected.
        let strict_fs = Arc::new(InMemoryFs::with_limits(
            crate::fs::FsLimits::default().max_path_length(50),
        )) as Arc<dyn FileSystem>;
        strict_fs
            .symlink(Path::new(&link_target), Path::new("/a"))
            .await
            .unwrap();
        let strict = run_readlink_with_fs(&["-m", &query], strict_fs).await;
        assert_eq!(strict.exit_code, 1, "stricter fs limit should reject");
        assert!(strict.stdout.is_empty());

        // Default fs: same composition is within budget -> resolves.
        let lax_fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        lax_fs
            .symlink(Path::new(&link_target), Path::new("/a"))
            .await
            .unwrap();
        let lax = run_readlink_with_fs(&["-m", &query], lax_fs).await;
        assert_eq!(lax.exit_code, 0, "default budget should resolve");
    }

    #[tokio::test]
    async fn test_readlink_e_existing() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.mkdir(Path::new("/existing"), false).await.unwrap();
        let result = run_readlink_with_fs(&["-e", "/existing"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/existing\n");
    }

    #[tokio::test]
    async fn test_readlink_e_nonexistent() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_readlink_with_fs(&["-e", "/nonexistent"], fs).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_readlink_invalid_option() {
        // The codegen-ported argument surface uses clap, which exits 2
        // (not the GNU coreutils convention of 1) on unknown flags.
        // The clap-vs-GNU exit-code divergence is documented in
        // `tests/spec_cases/bash/readlink.test.sh` (### bash_diff).
        // -z is a valid flag now (zero-terminate output), so the test
        // uses a string that is plainly not a flag bashkit ports.
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_readlink_with_fs(&["--definitely-not-a-flag", "/file"], fs).await;
        assert_eq!(result.exit_code, 2);
        let stderr_lower = result.stderr.to_lowercase();
        assert!(
            stderr_lower.contains("unexpected argument")
                || stderr_lower.contains("unknown argument")
                || stderr_lower.contains("invalid option"),
            "expected clap unknown-flag stderr, got {}",
            result.stderr
        );
    }
}
