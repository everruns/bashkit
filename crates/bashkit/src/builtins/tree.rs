//! tree builtin command - display directory tree

use async_trait::async_trait;
use std::path::Path;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::limits::ExecutionLimits;

/// The tree builtin command.
///
/// Usage: tree [-a] [-d] [-L level] [-I pattern] [--noreport] [PATH...]
///
/// Options:
///   -a            Show hidden files
///   -d            Directories only
///   -L level      Limit depth to level
///   -I pattern    Exclude files matching pattern
///   --noreport    Suppress directory/file count report
pub struct Tree;

// `tree` runs as one shell command, so it must enforce its own traversal fuel
// instead of relying only on the interpreter command counter.
const DEFAULT_TREE_MAX_VISITED_ENTRIES: usize = 100_000;

struct TreeOptions {
    show_hidden: bool,
    dirs_only: bool,
    max_depth: Option<usize>,
    exclude_pattern: Option<String>,
    noreport: bool,
}

struct TreeBudget {
    max_visited_entries: usize,
    visited_entries: usize,
    max_output_bytes: usize,
}

enum TreeLimitError {
    TooManyEntries,
    OutputTooLarge,
}

impl TreeLimitError {
    fn message(&self) -> &'static str {
        match self {
            Self::TooManyEntries => "tree: resource limit exceeded: too many entries visited\n",
            Self::OutputTooLarge => "tree: resource limit exceeded: output too large\n",
        }
    }
}

struct TreeCounts {
    dirs: usize,
    files: usize,
}

struct TreeState {
    counts: TreeCounts,
    budget: TreeBudget,
    output: String,
}

#[async_trait]
impl Builtin for Tree {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: tree [OPTION]... [DIRECTORY]...\nList contents of directories in a tree-like format.\n\n  -a\t\tshow hidden files\n  -d\t\tlist directories only\n  -L level\tdescend only level directories deep\n  -I pattern\texclude files matching pattern\n  --noreport\tsuppress the file/directory count at the end\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n",
            Some("tree (bashkit) 0.1"),
        ) {
            return Ok(r);
        }

        let mut opts = TreeOptions {
            show_hidden: false,
            dirs_only: false,
            max_depth: None,
            exclude_pattern: None,
            noreport: false,
        };

        let mut paths: Vec<&str> = Vec::new();
        let mut p = super::arg_parser::ArgParser::new(ctx.args);
        while !p.is_done() {
            if p.flag("-a") {
                opts.show_hidden = true;
            } else if p.flag("-d") {
                opts.dirs_only = true;
            } else if let Some(val) = p.flag_value_opt("-L") {
                match val.parse::<usize>() {
                    Ok(n) if n > 0 => opts.max_depth = Some(n),
                    _ => {
                        return Ok(ExecResult::err(
                            "tree: Invalid level, must be greater than 0.\n".to_string(),
                            1,
                        ));
                    }
                }
            } else if let Some(val) = p.flag_value_opt("-I") {
                opts.exclude_pattern = Some(val.to_string());
            } else if p.is_flag() {
                let Some(s) = p.current() else {
                    p.advance();
                    continue;
                };
                // Handle long options (--foo) before short-flag loop
                if s.starts_with("--") {
                    match s {
                        "--noreport" => opts.noreport = true,
                        _ => {
                            return Ok(ExecResult::err(
                                format!("tree: unrecognized option '{}'\n", s),
                                1,
                            ));
                        }
                    }
                    p.advance();
                    continue;
                }
                // Handle combined short flags like -ad
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
                p.advance();
            } else if let Some(arg) = p.positional() {
                paths.push(arg);
            }
        }

        if paths.is_empty() {
            paths.push(".");
        }

        let limits = ctx
            .execution_extension::<ExecutionLimits>()
            .cloned()
            .unwrap_or_default();
        let budget = TreeBudget {
            max_visited_entries: limits.max_commands.min(DEFAULT_TREE_MAX_VISITED_ENTRIES),
            visited_entries: 0,
            max_output_bytes: limits.max_stdout_bytes,
        };
        let mut state = TreeState {
            counts: TreeCounts { dirs: 0, files: 0 },
            budget,
            output: String::new(),
        };

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

            if let Err(e) =
                push_output(&mut state, path_str).and_then(|()| push_output(&mut state, "\n"))
            {
                return Ok(ExecResult::err(e.message().to_string(), 1));
            }

            state.counts = TreeCounts { dirs: 0, files: 0 };
            if let Err(e) = build_tree(&ctx, &root, "", &opts, 0, &mut state).await {
                return Ok(ExecResult::err(e.message().to_string(), 1));
            }

            if !opts.noreport {
                let summary = if opts.dirs_only {
                    format!(
                        "\n{} director{}\n",
                        state.counts.dirs,
                        if state.counts.dirs == 1 { "y" } else { "ies" }
                    )
                } else {
                    format!(
                        "\n{} director{}, {} file{}\n",
                        state.counts.dirs,
                        if state.counts.dirs == 1 { "y" } else { "ies" },
                        state.counts.files,
                        if state.counts.files == 1 { "" } else { "s" }
                    )
                };
                if let Err(e) = push_output(&mut state, &summary) {
                    return Ok(ExecResult::err(e.message().to_string(), 1));
                }
            }
        }

        Ok(ExecResult::ok(state.output))
    }
}

async fn build_tree(
    ctx: &Context<'_>,
    dir: &Path,
    prefix: &str,
    opts: &TreeOptions,
    depth: usize,
    state: &mut TreeState,
) -> std::result::Result<(), TreeLimitError> {
    if let Some(max) = opts.max_depth
        && depth >= max
    {
        return Ok(());
    }

    let entries = match ctx.fs.read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let remaining = state.budget.remaining_entries();
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
        .take(remaining.saturating_add(1))
        .collect();

    if filtered.len() > remaining {
        return Err(TreeLimitError::TooManyEntries);
    }
    state.budget.visited_entries += filtered.len();

    filtered.sort_by(|a, b| a.name.cmp(&b.name));

    let total = filtered.len();
    for (i, entry) in filtered.iter().enumerate() {
        let is_last = i == total - 1;
        let connector = if is_last {
            "\u{2514}\u{2500}\u{2500} "
        } else {
            "\u{251c}\u{2500}\u{2500} "
        };

        push_output(state, prefix)?;
        push_output(state, connector)?;
        push_output(state, &entry.name)?;
        push_output(state, "\n")?;

        if entry.metadata.file_type.is_dir() {
            state.counts.dirs += 1;
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
                state,
            ))
            .await?;
        } else {
            state.counts.files += 1;
        }
    }

    Ok(())
}

impl TreeBudget {
    fn remaining_entries(&self) -> usize {
        self.max_visited_entries
            .saturating_sub(self.visited_entries)
    }
}

fn push_output(state: &mut TreeState, s: &str) -> std::result::Result<(), TreeLimitError> {
    if state.output.len().saturating_add(s.len()) > state.budget.max_output_bytes {
        return Err(TreeLimitError::OutputTooLarge);
    }
    state.output.push_str(s);
    Ok(())
}

#[cfg(test)]
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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

    #[tokio::test]
    async fn test_tree_noreport() {
        let fs = setup_fs().await;
        let result = run_tree(&["--noreport", "/project"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/project"));
        assert!(result.stdout.contains("src"));
        assert!(result.stdout.contains("Cargo.toml"));
        // --noreport should suppress the summary line
        assert!(!result.stdout.contains("director"));
        assert!(!result.stdout.contains("file"));
    }

    #[tokio::test]
    async fn test_tree_unknown_long_option() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_tree(&["--bogus"], fs).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("unrecognized option"));
    }

    #[tokio::test]
    async fn test_tree_entry_budget_exceeded() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.mkdir(Path::new("/wide"), true).await.unwrap();
        for i in 0..3 {
            fs.write_file(Path::new(&format!("/wide/file{i}.txt")), b"x")
                .await
                .unwrap();
        }

        let mut bash = crate::Bash::builder()
            .fs(fs)
            .limits(ExecutionLimits::new().max_commands(2))
            .build();
        let result = bash.exec("tree /wide").await.expect("tree failed");

        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.contains("too many entries visited"));
    }

    #[tokio::test]
    async fn test_tree_output_budget_exceeded() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.mkdir(Path::new("/wide"), true).await.unwrap();
        fs.write_file(Path::new("/wide/long-name.txt"), b"x")
            .await
            .unwrap();

        let mut bash = crate::Bash::builder()
            .fs(fs)
            .limits(ExecutionLimits::new().max_stdout_bytes(12))
            .build();
        let result = bash.exec("tree /wide").await.expect("tree failed");

        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.contains("output too large"));
    }
}
