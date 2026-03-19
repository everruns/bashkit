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
    // args_iter.next().unwrap() is safe: guarded by is_empty() check above
    #[allow(clippy::unwrap_used)]
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "basename: missing operand\n".to_string(),
                1,
            ));
        }

        let mut output = String::new();
        let mut args_iter = ctx.args.iter();

        // Get the path argument
        let path_arg = args_iter.next().unwrap();
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
/// Usage: realpath [PATH...]
///
/// Resolves `.` and `..` components and prints absolute canonical paths.
/// In bashkit's virtual filesystem, symlink resolution is not performed.
pub struct Realpath;

#[async_trait]
impl Builtin for Realpath {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "realpath: missing operand\n".to_string(),
                1,
            ));
        }

        let mut output = String::new();
        for arg in ctx.args {
            if arg.starts_with('-') {
                continue; // skip flags like -e, -m, -s
            }
            let resolved = super::resolve_path(ctx.cwd, arg);
            output.push_str(&resolved.to_string_lossy());
            output.push('\n');
        }

        Ok(ExecResult::ok(output))
    }
}

/// The readlink builtin - print resolved symbolic links or canonical file names.
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
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "readlink: missing operand\n".to_string(),
                1,
            ));
        }

        let mut mode = ReadlinkMode::Raw;
        let mut files: Vec<&str> = Vec::new();

        for arg in ctx.args {
            match arg.as_str() {
                "-f" => mode = ReadlinkMode::Canonicalize,
                "-m" => mode = ReadlinkMode::CanonicalizeMissing,
                "-e" => mode = ReadlinkMode::CanonicalizeExisting,
                "-n" | "-v" | "-q" | "-s" | "--no-newline" => { /* silently accept */ }
                s if s.starts_with('-') && s.len() > 1 && !s.starts_with("--") => {
                    // Could be combined flags like -fn
                    for ch in s[1..].chars() {
                        match ch {
                            'f' => mode = ReadlinkMode::Canonicalize,
                            'm' => mode = ReadlinkMode::CanonicalizeMissing,
                            'e' => mode = ReadlinkMode::CanonicalizeExisting,
                            'n' | 'v' | 'q' | 's' => {}
                            _ => {
                                return Ok(ExecResult::err(
                                    format!("readlink: invalid option -- '{}'\n", ch),
                                    1,
                                ));
                            }
                        }
                    }
                }
                _ => files.push(arg),
            }
        }

        if files.is_empty() {
            return Ok(ExecResult::err(
                "readlink: missing operand\n".to_string(),
                1,
            ));
        }

        let mut output = String::new();
        let mut exit_code = 0;

        for file in &files {
            let resolved = super::resolve_path(ctx.cwd, file);

            match mode {
                ReadlinkMode::Raw => {
                    // No flag: read symlink target
                    match ctx.fs.read_link(&resolved).await {
                        Ok(target) => {
                            output.push_str(&target.to_string_lossy());
                            output.push('\n');
                        }
                        Err(_) => {
                            exit_code = 1;
                        }
                    }
                }
                ReadlinkMode::Canonicalize | ReadlinkMode::CanonicalizeMissing => {
                    // -f and -m: canonicalize path (resolve . and ..)
                    // -m doesn't require existence, -f requires all but last
                    let parent_missing = if mode == ReadlinkMode::Canonicalize {
                        resolved
                            .parent()
                            .filter(|p| !p.as_os_str().is_empty())
                            .map(|p| ctx.fs.exists(p))
                    } else {
                        None
                    };
                    if let Some(fut) = parent_missing {
                        if !fut.await.unwrap_or(false) {
                            exit_code = 1;
                            continue;
                        }
                    }
                    output.push_str(&resolved.to_string_lossy());
                    output.push('\n');
                }
                ReadlinkMode::CanonicalizeExisting => {
                    // -e: all components must exist
                    if ctx.fs.exists(&resolved).await.unwrap_or(false) {
                        output.push_str(&resolved.to_string_lossy());
                        output.push('\n');
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

#[derive(PartialEq)]
enum ReadlinkMode {
    Raw,
    Canonicalize,
    CanonicalizeMissing,
    CanonicalizeExisting,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_readlink_with_fs(&["-z", "/file"], fs).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option"));
    }
}
