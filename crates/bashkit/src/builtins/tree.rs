//! tree builtin command - display directory tree

use async_trait::async_trait;
use std::path::Path;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The tree builtin command.
///
/// Usage: tree [-a] [-d] [-L level] [-I pattern] [PATH...]
///
/// Options:
///   -a          Show hidden files
///   -d          Directories only
///   -L level    Limit depth to level
///   -I pattern  Exclude files matching pattern
pub struct Tree;

struct TreeOptions {
    show_hidden: bool,
    dirs_only: bool,
    max_depth: Option<usize>,
    exclude_pattern: Option<String>,
}

struct TreeCounts {
    dirs: usize,
    files: usize,
}

#[async_trait]
impl Builtin for Tree {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut opts = TreeOptions {
            show_hidden: false,
            dirs_only: false,
            max_depth: None,
            exclude_pattern: None,
        };

        let mut paths: Vec<&str> = Vec::new();
        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-a" => opts.show_hidden = true,
                "-d" => opts.dirs_only = true,
                "-L" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "tree: option requires an argument -- 'L'\n".to_string(),
                            1,
                        ));
                    }
                    match ctx.args[i].parse::<usize>() {
                        Ok(n) if n > 0 => opts.max_depth = Some(n),
                        _ => {
                            return Ok(ExecResult::err(
                                "tree: Invalid level, must be greater than 0.\n".to_string(),
                                1,
                            ));
                        }
                    }
                }
                "-I" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "tree: option requires an argument -- 'I'\n".to_string(),
                            1,
                        ));
                    }
                    opts.exclude_pattern = Some(ctx.args[i].clone());
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    for ch in s[1..].chars() {
                        match ch {
                            'a' => opts.show_hidden = true,
                            'd' => opts.dirs_only = true,
                            _ => {
                                return Ok(ExecResult::err(
                                    format!("tree: invalid option -- '{}'\n", ch),
                                    1,
                                ));
                            }
                        }
                    }
                }
                _ => paths.push(&ctx.args[i]),
            }
            i += 1;
        }

        if paths.is_empty() {
            paths.push(".");
        }

        let mut output = String::new();

        for path_str in &paths {
            let root = resolve_path(ctx.cwd, path_str);

            if !ctx.fs.exists(&root).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!(
                        "{} [error opening dir]\n\n0 directories, 0 files\n",
                        path_str
                    ),
                    2,
                ));
            }

            output.push_str(path_str);
            output.push('\n');

            let mut counts = TreeCounts { dirs: 0, files: 0 };
            build_tree(&ctx, &root, "", &opts, 0, &mut counts, &mut output).await;

            if opts.dirs_only {
                output.push_str(&format!(
                    "\n{} director{}\n",
                    counts.dirs,
                    if counts.dirs == 1 { "y" } else { "ies" }
                ));
            } else {
                output.push_str(&format!(
                    "\n{} director{}, {} file{}\n",
                    counts.dirs,
                    if counts.dirs == 1 { "y" } else { "ies" },
                    counts.files,
                    if counts.files == 1 { "" } else { "s" }
                ));
            }
        }

        Ok(ExecResult::ok(output))
    }
}

async fn build_tree(
    ctx: &Context<'_>,
    dir: &Path,
    prefix: &str,
    opts: &TreeOptions,
    depth: usize,
    counts: &mut TreeCounts,
    output: &mut String,
) {
    if let Some(max) = opts.max_depth
        && depth >= max
    {
        return;
    }

    let entries = match ctx.fs.read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut filtered: Vec<_> = entries
        .into_iter()
        .filter(|e| {
            if !opts.show_hidden && e.name.starts_with('.') {
                return false;
            }
            if opts.dirs_only && !e.metadata.file_type.is_dir() {
                return false;
            }
            if let Some(ref pattern) = opts.exclude_pattern
                && e.name.contains(pattern.as_str())
            {
                return false;
            }
            true
        })
        .collect();

    filtered.sort_by(|a, b| a.name.cmp(&b.name));

    let total = filtered.len();
    for (i, entry) in filtered.iter().enumerate() {
        let is_last = i == total - 1;
        let connector = if is_last {
            "\u{2514}\u{2500}\u{2500} "
        } else {
            "\u{251c}\u{2500}\u{2500} "
        };

        output.push_str(prefix);
        output.push_str(connector);
        output.push_str(&entry.name);
        output.push('\n');

        if entry.metadata.file_type.is_dir() {
            counts.dirs += 1;
            let new_prefix = if is_last {
                format!("{}    ", prefix)
            } else {
                format!("{}\u{2502}   ", prefix)
            };
            let child_path = dir.join(&entry.name);
            Box::pin(build_tree(
                ctx,
                &child_path,
                &new_prefix,
                opts,
                depth + 1,
                counts,
                output,
            ))
            .await;
        } else {
            counts.files += 1;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFs};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn run_tree(args: &[&str], fs: Arc<dyn FileSystem>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
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
        };
        Tree.execute(ctx).await.expect("tree execute failed")
    }

    async fn setup_fs() -> Arc<dyn FileSystem> {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.mkdir(Path::new("/project"), true).await.unwrap();
        fs.mkdir(Path::new("/project/src"), true).await.unwrap();
        fs.write_file(Path::new("/project/src/main.rs"), b"fn main() {}")
            .await
            .unwrap();
        fs.write_file(Path::new("/project/src/lib.rs"), b"pub mod lib;")
            .await
            .unwrap();
        fs.mkdir(Path::new("/project/tests"), true).await.unwrap();
        fs.write_file(Path::new("/project/tests/test.rs"), b"#[test]")
            .await
            .unwrap();
        fs.write_file(Path::new("/project/Cargo.toml"), b"[package]")
            .await
            .unwrap();
        fs.write_file(Path::new("/project/.gitignore"), b"target/")
            .await
            .unwrap();
        fs
    }

    #[tokio::test]
    async fn test_tree_basic() {
        let fs = setup_fs().await;
        let result = run_tree(&["/project"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/project"));
        assert!(result.stdout.contains("Cargo.toml"));
        assert!(result.stdout.contains("src"));
        assert!(result.stdout.contains("main.rs"));
        // Should not show hidden files by default
        assert!(!result.stdout.contains(".gitignore"));
        // Should have summary
        assert!(result.stdout.contains("director"));
        assert!(result.stdout.contains("file"));
    }

    #[tokio::test]
    async fn test_tree_show_hidden() {
        let fs = setup_fs().await;
        let result = run_tree(&["-a", "/project"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains(".gitignore"));
    }

    #[tokio::test]
    async fn test_tree_dirs_only() {
        let fs = setup_fs().await;
        let result = run_tree(&["-d", "/project"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("src"));
        assert!(result.stdout.contains("tests"));
        assert!(!result.stdout.contains("Cargo.toml"));
        assert!(!result.stdout.contains("main.rs"));
        // Summary should only mention directories
        assert!(result.stdout.contains("director"));
        assert!(!result.stdout.contains("file"));
    }

    #[tokio::test]
    async fn test_tree_depth_limit() {
        let fs = setup_fs().await;
        let result = run_tree(&["-L", "1", "/project"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("src"));
        assert!(result.stdout.contains("Cargo.toml"));
        // Should NOT show nested files
        assert!(!result.stdout.contains("main.rs"));
    }

    #[tokio::test]
    async fn test_tree_exclude_pattern() {
        let fs = setup_fs().await;
        let result = run_tree(&["-I", "test", "/project"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("src"));
        assert!(!result.stdout.contains("tests"));
    }

    #[tokio::test]
    async fn test_tree_nonexistent_dir() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_tree(&["/nonexistent"], fs).await;
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("error opening dir"));
    }

    #[tokio::test]
    async fn test_tree_invalid_depth() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_tree(&["-L", "0"], fs).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("Invalid level"));
    }

    #[tokio::test]
    async fn test_tree_empty_dir() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.mkdir(Path::new("/empty"), true).await.unwrap();
        let result = run_tree(&["/empty"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/empty"));
        assert!(result.stdout.contains("0 directories, 0 files"));
    }

    #[tokio::test]
    async fn test_tree_cwd_default() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.mkdir(Path::new("/mydir"), true).await.unwrap();
        fs.write_file(Path::new("/mydir/file.txt"), b"content")
            .await
            .unwrap();

        // Run with cwd=/mydir, no path argument
        let args: Vec<String> = Vec::new();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/mydir");
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
        };
        let result = Tree.execute(ctx).await.expect("tree failed");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("file.txt"));
    }

    #[tokio::test]
    async fn test_tree_invalid_option() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_tree(&["-z"], fs).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option"));
    }
}
