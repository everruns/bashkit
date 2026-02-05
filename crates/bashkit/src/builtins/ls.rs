//! Directory listing builtins - ls, find, rmdir

// Uses unwrap() for validated single-char strings (e.g., "f".chars().next())
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use std::path::Path;

use super::{resolve_path, Builtin, Context};
use crate::error::Result;
use crate::fs::FileType;
use crate::interpreter::ExecResult;

/// Options for ls command
struct LsOptions {
    long: bool,
    all: bool,
    human: bool,
    one_per_line: bool,
    recursive: bool,
    sort_by_time: bool,
}

/// The ls builtin - list directory contents.
///
/// Usage: ls [-l] [-a] [-h] [-1] [-R] [-t] [PATH...]
///
/// Options:
///   -l   Use long listing format
///   -a   Show hidden files (starting with .)
///   -h   Human-readable sizes (with -l)
///   -1   One entry per line
///   -R   List subdirectories recursively
///   -t   Sort by modification time, newest first
pub struct Ls;

#[async_trait]
impl Builtin for Ls {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut opts = LsOptions {
            long: false,
            all: false,
            human: false,
            one_per_line: false,
            recursive: false,
            sort_by_time: false,
        };

        // Parse flags
        let mut paths: Vec<&str> = Vec::new();
        for arg in ctx.args {
            if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
                for c in arg[1..].chars() {
                    match c {
                        'l' => opts.long = true,
                        'a' => opts.all = true,
                        'h' => opts.human = true,
                        '1' => opts.one_per_line = true,
                        'R' => opts.recursive = true,
                        't' => opts.sort_by_time = true,
                        _ => {
                            return Ok(ExecResult::err(
                                format!("ls: invalid option -- '{}'\n", c),
                                2,
                            ));
                        }
                    }
                }
            } else {
                paths.push(arg);
            }
        }

        // Default to current directory
        if paths.is_empty() {
            paths.push(".");
        }

        let mut output = String::new();
        let multiple_paths = paths.len() > 1 || opts.recursive;

        for (i, path_str) in paths.iter().enumerate() {
            let path = resolve_path(ctx.cwd, path_str);

            // Check if path exists
            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!(
                        "ls: cannot access '{}': No such file or directory\n",
                        path_str
                    ),
                    2,
                ));
            }

            // Check if it's a file or directory
            let metadata = ctx.fs.stat(&path).await?;

            if metadata.file_type.is_file() {
                // Single file - just list it
                let name = Path::new(path_str)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path_str.to_string());

                if opts.long {
                    output.push_str(&format_long_entry(&name, &metadata, opts.human));
                } else {
                    output.push_str(&name);
                    output.push('\n');
                }
            } else {
                // Directory
                if let Err(e) = list_directory(
                    &ctx,
                    &path,
                    path_str,
                    &mut output,
                    &opts,
                    multiple_paths,
                    i > 0,
                )
                .await
                {
                    return Ok(ExecResult::err(format!("ls: {}\n", e), 2));
                }
            }
        }

        Ok(ExecResult::ok(output))
    }
}

async fn list_directory(
    ctx: &Context<'_>,
    path: &Path,
    display_path: &str,
    output: &mut String,
    opts: &LsOptions,
    show_header: bool,
    add_newline: bool,
) -> std::result::Result<(), String> {
    if add_newline {
        output.push('\n');
    }

    if show_header {
        output.push_str(&format!("{}:\n", display_path));
    }

    let entries = ctx
        .fs
        .read_dir(path)
        .await
        .map_err(|e| format!("cannot open directory '{}': {}", display_path, e))?;

    // Sort entries
    let mut sorted_entries = entries;
    if opts.sort_by_time {
        // Sort by modification time, newest first
        sorted_entries.sort_by(|a, b| b.metadata.modified.cmp(&a.metadata.modified));
    } else {
        // Sort alphabetically
        sorted_entries.sort_by(|a, b| a.name.cmp(&b.name));
    }

    // Filter hidden files unless -a
    let filtered: Vec<_> = sorted_entries
        .iter()
        .filter(|e| opts.all || !e.name.starts_with('.'))
        .collect();

    // Collect subdirectories for recursive listing
    let mut subdirs: Vec<(std::path::PathBuf, String)> = Vec::new();

    if opts.long {
        for entry in &filtered {
            output.push_str(&format_long_entry(&entry.name, &entry.metadata, opts.human));
            if opts.recursive && entry.metadata.file_type.is_dir() {
                subdirs.push((
                    path.join(&entry.name),
                    format!("{}/{}", display_path, entry.name),
                ));
            }
        }
    } else if opts.one_per_line {
        for entry in &filtered {
            output.push_str(&entry.name);
            output.push('\n');
            if opts.recursive && entry.metadata.file_type.is_dir() {
                subdirs.push((
                    path.join(&entry.name),
                    format!("{}/{}", display_path, entry.name),
                ));
            }
        }
    } else {
        // Simple columnar output (simplified - one per line for now)
        for entry in &filtered {
            output.push_str(&entry.name);
            output.push('\n');
            if opts.recursive && entry.metadata.file_type.is_dir() {
                subdirs.push((
                    path.join(&entry.name),
                    format!("{}/{}", display_path, entry.name),
                ));
            }
        }
    }

    // Recursive listing
    if opts.recursive {
        for (subpath, display) in subdirs {
            // Box the future to avoid infinite recursion type size
            Box::pin(list_directory(
                ctx, &subpath, &display, output, opts, true, true,
            ))
            .await?;
        }
    }

    Ok(())
}

fn format_long_entry(name: &str, metadata: &crate::fs::Metadata, human: bool) -> String {
    let file_type = match metadata.file_type {
        FileType::Directory => 'd',
        FileType::Symlink => 'l',
        FileType::File => '-',
    };

    let mode = metadata.mode;
    let perms = format!(
        "{}{}{}{}{}{}{}{}{}",
        if mode & 0o400 != 0 { 'r' } else { '-' },
        if mode & 0o200 != 0 { 'w' } else { '-' },
        if mode & 0o100 != 0 { 'x' } else { '-' },
        if mode & 0o040 != 0 { 'r' } else { '-' },
        if mode & 0o020 != 0 { 'w' } else { '-' },
        if mode & 0o010 != 0 { 'x' } else { '-' },
        if mode & 0o004 != 0 { 'r' } else { '-' },
        if mode & 0o002 != 0 { 'w' } else { '-' },
        if mode & 0o001 != 0 { 'x' } else { '-' },
    );

    let size = if human {
        human_readable_size(metadata.size)
    } else {
        format!("{:>8}", metadata.size)
    };

    // Format modified time
    let modified = metadata
        .modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            // Simple date formatting: YYYY-MM-DD HH:MM
            let days = secs / 86400;
            let hours = (secs % 86400) / 3600;
            let mins = (secs % 3600) / 60;
            // Approximate date calculation
            let years = 1970 + (days / 365);
            let remaining_days = days % 365;
            let month = remaining_days / 30 + 1;
            let day = remaining_days % 30 + 1;
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}",
                years, month, day, hours, mins
            )
        })
        .unwrap_or_else(|_| "????-??-?? ??:??".to_string());

    format!("{}{} {} {} {}\n", file_type, perms, size, modified, name)
}

fn human_readable_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if size >= GB {
        format!("{:>5.1}G", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:>5.1}M", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:>5.1}K", size as f64 / KB as f64)
    } else {
        format!("{:>6}", size)
    }
}

/// Options for find command
struct FindOptions {
    name_pattern: Option<String>,
    type_filter: Option<char>,
    max_depth: Option<usize>,
}

/// The find builtin - search for files.
///
/// Usage: find [PATH...] [-name PATTERN] [-type TYPE] [-maxdepth N]
///
/// Options:
///   -name PATTERN   Match filename against PATTERN (supports * and ?)
///   -type TYPE      Match file type: f (file), d (directory), l (link)
///   -maxdepth N     Descend at most N levels
///   -print          Print matching paths (default)
pub struct Find;

#[async_trait]
impl Builtin for Find {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut paths: Vec<String> = Vec::new();
        let mut opts = FindOptions {
            name_pattern: None,
            type_filter: None,
            max_depth: None,
        };

        // Parse arguments
        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            match arg.as_str() {
                "-name" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "find: missing argument to '-name'\n".to_string(),
                            1,
                        ));
                    }
                    opts.name_pattern = Some(ctx.args[i].clone());
                }
                "-type" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "find: missing argument to '-type'\n".to_string(),
                            1,
                        ));
                    }
                    let t = &ctx.args[i];
                    match t.as_str() {
                        "f" | "d" | "l" => opts.type_filter = Some(t.chars().next().unwrap()),
                        _ => {
                            return Ok(ExecResult::err(format!("find: unknown type '{}'\n", t), 1));
                        }
                    }
                }
                "-maxdepth" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "find: missing argument to '-maxdepth'\n".to_string(),
                            1,
                        ));
                    }
                    match ctx.args[i].parse::<usize>() {
                        Ok(n) => opts.max_depth = Some(n),
                        Err(_) => {
                            return Ok(ExecResult::err(
                                format!("find: invalid maxdepth value '{}'\n", ctx.args[i]),
                                1,
                            ));
                        }
                    }
                }
                "-print" => {
                    // Default action, ignore
                }
                s if s.starts_with('-') => {
                    return Ok(ExecResult::err(
                        format!("find: unknown predicate '{}'\n", s),
                        1,
                    ));
                }
                _ => {
                    paths.push(arg.clone());
                }
            }
            i += 1;
        }

        // Default to current directory
        if paths.is_empty() {
            paths.push(".".to_string());
        }

        let mut output = String::new();

        for path_str in &paths {
            let path = resolve_path(ctx.cwd, path_str);

            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!("find: '{}': No such file or directory\n", path_str),
                    1,
                ));
            }

            find_recursive(&ctx, &path, path_str, &opts, 0, &mut output).await?;
        }

        Ok(ExecResult::ok(output))
    }
}

fn find_recursive<'a>(
    ctx: &'a Context<'_>,
    path: &'a Path,
    display_path: &'a str,
    opts: &'a FindOptions,
    current_depth: usize,
    output: &'a mut String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        // Check if this entry matches
        let metadata = ctx.fs.stat(path).await?;
        let entry_name = Path::new(display_path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| display_path.to_string());

        // Check type filter
        let type_matches = match opts.type_filter {
            Some('f') => metadata.file_type.is_file(),
            Some('d') => metadata.file_type.is_dir(),
            Some('l') => metadata.file_type.is_symlink(),
            _ => true,
        };

        // Check name pattern
        let name_matches = match &opts.name_pattern {
            Some(pattern) => glob_match(&entry_name, pattern),
            None => true,
        };

        // Output if matches (or if no filters, show everything)
        if type_matches && name_matches {
            output.push_str(display_path);
            output.push('\n');
        }

        // Recurse into directories
        if metadata.file_type.is_dir() {
            // Check max depth
            if let Some(max) = opts.max_depth {
                if current_depth >= max {
                    return Ok(());
                }
            }

            let entries = ctx.fs.read_dir(path).await?;
            let mut sorted_entries = entries;
            sorted_entries.sort_by(|a, b| a.name.cmp(&b.name));

            for entry in sorted_entries {
                let child_path = path.join(&entry.name);
                let child_display = if display_path == "." {
                    format!("./{}", entry.name)
                } else {
                    format!("{}/{}", display_path, entry.name)
                };

                find_recursive(
                    ctx,
                    &child_path,
                    &child_display,
                    opts,
                    current_depth + 1,
                    output,
                )
                .await?;
            }
        }

        Ok(())
    })
}

/// Simple glob pattern matching for find -name
fn glob_match(value: &str, pattern: &str) -> bool {
    let mut value_chars = value.chars().peekable();
    let mut pattern_chars = pattern.chars().peekable();

    loop {
        match (pattern_chars.peek(), value_chars.peek()) {
            (None, None) => return true,
            (None, Some(_)) => return false,
            (Some('*'), _) => {
                pattern_chars.next();
                if pattern_chars.peek().is_none() {
                    return true;
                }
                while value_chars.peek().is_some() {
                    let remaining_value: String = value_chars.clone().collect();
                    let remaining_pattern: String = pattern_chars.clone().collect();
                    if glob_match(&remaining_value, &remaining_pattern) {
                        return true;
                    }
                    value_chars.next();
                }
                let remaining_pattern: String = pattern_chars.collect();
                return glob_match("", &remaining_pattern);
            }
            (Some('?'), Some(_)) => {
                pattern_chars.next();
                value_chars.next();
            }
            (Some('?'), None) => return false,
            (Some(p), Some(v)) => {
                if *p == *v {
                    pattern_chars.next();
                    value_chars.next();
                } else {
                    return false;
                }
            }
            (Some(_), None) => return false,
        }
    }
}

/// The rmdir builtin - remove empty directories.
///
/// Usage: rmdir [-p] DIRECTORY...
///
/// Options:
///   -p   Remove parent directories as well if they become empty
pub struct Rmdir;

#[async_trait]
impl Builtin for Rmdir {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err("rmdir: missing operand\n".to_string(), 1));
        }

        let parents = ctx.args.iter().any(|a| a == "-p");
        let dirs: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        if dirs.is_empty() {
            return Ok(ExecResult::err("rmdir: missing operand\n".to_string(), 1));
        }

        for dir in dirs {
            let path = resolve_path(ctx.cwd, dir);

            // Check if exists
            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!(
                        "rmdir: failed to remove '{}': No such file or directory\n",
                        dir
                    ),
                    1,
                ));
            }

            // Check if it's a directory
            let metadata = ctx.fs.stat(&path).await?;
            if !metadata.file_type.is_dir() {
                return Ok(ExecResult::err(
                    format!("rmdir: failed to remove '{}': Not a directory\n", dir),
                    1,
                ));
            }

            // Check if directory is empty
            let entries = ctx.fs.read_dir(&path).await?;
            if !entries.is_empty() {
                return Ok(ExecResult::err(
                    format!("rmdir: failed to remove '{}': Directory not empty\n", dir),
                    1,
                ));
            }

            // Remove the directory
            if let Err(e) = ctx.fs.remove(&path, false).await {
                return Ok(ExecResult::err(
                    format!("rmdir: failed to remove '{}': {}\n", dir, e),
                    1,
                ));
            }

            // If -p, try to remove parent directories
            if parents {
                let mut current = path.parent();
                while let Some(parent) = current {
                    // Don't remove root or cwd
                    if parent.as_os_str().is_empty() || parent == ctx.cwd.as_path() {
                        break;
                    }

                    // Check if parent is empty
                    if let Ok(entries) = ctx.fs.read_dir(parent).await {
                        if entries.is_empty() {
                            if ctx.fs.remove(parent, false).await.is_err() {
                                break;
                            }
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }

                    current = parent.parent();
                }
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn create_test_ctx() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();

        fs.mkdir(&cwd, true).await.unwrap();

        (fs, cwd, variables)
    }

    // ==================== ls tests ====================

    #[tokio::test]
    async fn test_ls_empty_dir() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_ls_with_files() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create some files
        fs.write_file(&cwd.join("file1.txt"), b"content1")
            .await
            .unwrap();
        fs.write_file(&cwd.join("file2.txt"), b"content2")
            .await
            .unwrap();

        let args: Vec<String> = vec![];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("file1.txt"));
        assert!(result.stdout.contains("file2.txt"));
    }

    #[tokio::test]
    async fn test_ls_hidden_files() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join(".hidden"), b"hidden")
            .await
            .unwrap();
        fs.write_file(&cwd.join("visible"), b"visible")
            .await
            .unwrap();

        // Without -a
        let args: Vec<String> = vec![];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!result.stdout.contains(".hidden"));
        assert!(result.stdout.contains("visible"));

        // With -a
        let args = vec!["-a".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains(".hidden"));
        assert!(result.stdout.contains("visible"));
    }

    #[tokio::test]
    async fn test_ls_long_format() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("test.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["-l".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        // Long format should include permissions
        assert!(result.stdout.contains("rw"));
        assert!(result.stdout.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_ls_nonexistent() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["nonexistent".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("No such file or directory"));
    }

    #[tokio::test]
    async fn test_ls_invalid_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-z".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("invalid option"));
    }

    #[tokio::test]
    async fn test_ls_sort_by_time() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create files with different modification times
        fs.write_file(&cwd.join("older.txt"), b"older")
            .await
            .unwrap();
        fs.write_file(&cwd.join("newer.txt"), b"newer")
            .await
            .unwrap();

        let args = vec!["-t".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        // -t should be accepted (not cause an error)
        assert!(result.stdout.contains("older.txt"));
        assert!(result.stdout.contains("newer.txt"));
    }

    #[tokio::test]
    async fn test_ls_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("test.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["test.txt".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_ls_recursive() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.mkdir(&cwd.join("subdir"), false).await.unwrap();
        fs.write_file(&cwd.join("file.txt"), b"content")
            .await
            .unwrap();
        fs.write_file(&cwd.join("subdir/nested.txt"), b"nested")
            .await
            .unwrap();

        let args = vec!["-R".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("file.txt"));
        assert!(result.stdout.contains("subdir"));
        assert!(result.stdout.contains("nested.txt"));
    }

    // ==================== find tests ====================

    #[tokio::test]
    async fn test_find_current_dir() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("file.txt"), b"content")
            .await
            .unwrap();

        let args: Vec<String> = vec![];
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
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("."));
        assert!(result.stdout.contains("file.txt"));
    }

    #[tokio::test]
    async fn test_find_name_pattern() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("file.txt"), b"content")
            .await
            .unwrap();
        fs.write_file(&cwd.join("other.md"), b"content")
            .await
            .unwrap();

        let args = vec!["-name".to_string(), "*.txt".to_string()];
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
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("file.txt"));
        assert!(!result.stdout.contains("other.md"));
    }

    #[tokio::test]
    async fn test_find_type_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("file.txt"), b"content")
            .await
            .unwrap();
        fs.mkdir(&cwd.join("subdir"), false).await.unwrap();

        let args = vec!["-type".to_string(), "f".to_string()];
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
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("file.txt"));
        assert!(!result.stdout.contains("subdir"));
    }

    #[tokio::test]
    async fn test_find_type_directory() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("file.txt"), b"content")
            .await
            .unwrap();
        fs.mkdir(&cwd.join("subdir"), false).await.unwrap();

        let args = vec!["-type".to_string(), "d".to_string()];
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
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!result.stdout.contains("file.txt"));
        // Should contain the directory
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert!(lines.iter().any(|l| l.contains("subdir") || *l == "."));
    }

    #[tokio::test]
    async fn test_find_maxdepth() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.mkdir(&cwd.join("a"), false).await.unwrap();
        fs.mkdir(&cwd.join("a/b"), false).await.unwrap();
        fs.write_file(&cwd.join("a/b/deep.txt"), b"deep")
            .await
            .unwrap();

        let args = vec!["-maxdepth".to_string(), "1".to_string()];
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
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("./a"));
        assert!(!result.stdout.contains("deep.txt"));
    }

    #[tokio::test]
    async fn test_find_nonexistent() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["nonexistent".to_string()];
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
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("No such file or directory"));
    }

    #[tokio::test]
    async fn test_find_missing_name_arg() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-name".to_string()];
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
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing argument"));
    }

    #[tokio::test]
    async fn test_find_unknown_type() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-type".to_string(), "x".to_string()];
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
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("unknown type"));
    }

    #[tokio::test]
    async fn test_find_deep_recursion() {
        // Test that find without maxdepth descends into all subdirectory levels
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create a deep directory structure: a/b/c/d/deep.txt
        fs.mkdir(&cwd.join("a"), false).await.unwrap();
        fs.mkdir(&cwd.join("a/b"), false).await.unwrap();
        fs.mkdir(&cwd.join("a/b/c"), false).await.unwrap();
        fs.mkdir(&cwd.join("a/b/c/d"), false).await.unwrap();
        fs.write_file(&cwd.join("a/b/c/d/deep.txt"), b"deep content")
            .await
            .unwrap();

        // Also add files at each level
        fs.write_file(&cwd.join("a/file1.txt"), b"level1")
            .await
            .unwrap();
        fs.write_file(&cwd.join("a/b/file2.txt"), b"level2")
            .await
            .unwrap();
        fs.write_file(&cwd.join("a/b/c/file3.txt"), b"level3")
            .await
            .unwrap();

        let args: Vec<String> = vec![];
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
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Should find the root
        assert!(result.stdout.contains("."), "Should contain current dir");

        // Should find all directories at all levels
        assert!(result.stdout.contains("./a"), "Should contain ./a");
        assert!(result.stdout.contains("./a/b"), "Should contain ./a/b");
        assert!(result.stdout.contains("./a/b/c"), "Should contain ./a/b/c");
        assert!(
            result.stdout.contains("./a/b/c/d"),
            "Should contain ./a/b/c/d"
        );

        // Should find all files at all levels
        assert!(
            result.stdout.contains("file1.txt"),
            "Should contain file1.txt at level 1"
        );
        assert!(
            result.stdout.contains("file2.txt"),
            "Should contain file2.txt at level 2"
        );
        assert!(
            result.stdout.contains("file3.txt"),
            "Should contain file3.txt at level 3"
        );
        assert!(
            result.stdout.contains("deep.txt"),
            "Should contain deep.txt at level 4"
        );
    }

    #[tokio::test]
    async fn test_ls_recursive_deep() {
        // Test that ls -R descends into all subdirectory levels
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create a deep directory structure: a/b/c/deep.txt
        fs.mkdir(&cwd.join("a"), false).await.unwrap();
        fs.mkdir(&cwd.join("a/b"), false).await.unwrap();
        fs.mkdir(&cwd.join("a/b/c"), false).await.unwrap();
        fs.write_file(&cwd.join("a/b/c/deep.txt"), b"deep content")
            .await
            .unwrap();

        let args = vec!["-R".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Should list all directories and their contents
        assert!(result.stdout.contains("a"), "Should list dir a");
        assert!(result.stdout.contains("b"), "Should list dir b under a");
        assert!(result.stdout.contains("c"), "Should list dir c under a/b");
        assert!(
            result.stdout.contains("deep.txt"),
            "Should list deep.txt under a/b/c"
        );
    }

    #[tokio::test]
    async fn test_find_very_deep_nesting() {
        // Test 10 levels of nesting to ensure no recursion limits
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create 10 levels deep
        let mut path = cwd.clone();
        for i in 0..10 {
            path = path.join(format!("level{}", i));
            fs.mkdir(&path, false).await.unwrap();
            fs.write_file(
                &path.join(format!("file{}.txt", i)),
                format!("content{}", i).as_bytes(),
            )
            .await
            .unwrap();
        }

        let args: Vec<String> = vec![];
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
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Verify all 10 levels are found
        for i in 0..10 {
            assert!(
                result.stdout.contains(&format!("level{}", i)),
                "Should find level{} directory",
                i
            );
            assert!(
                result.stdout.contains(&format!("file{}.txt", i)),
                "Should find file{}.txt",
                i
            );
        }
    }

    #[tokio::test]
    async fn test_find_and_ls_consistency() {
        // Ensure find and ls -R find the same nested structure
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create test structure
        fs.mkdir(&cwd.join("top"), false).await.unwrap();
        fs.mkdir(&cwd.join("top/middle"), false).await.unwrap();
        fs.mkdir(&cwd.join("top/middle/bottom"), false)
            .await
            .unwrap();
        fs.write_file(&cwd.join("top/a.txt"), b"a").await.unwrap();
        fs.write_file(&cwd.join("top/middle/b.txt"), b"b")
            .await
            .unwrap();
        fs.write_file(&cwd.join("top/middle/bottom/c.txt"), b"c")
            .await
            .unwrap();

        // Run find
        let args_find: Vec<String> = vec![];
        let ctx_find = Context {
            args: &args_find,
            env: &env,
            variables: &mut variables.clone(),
            cwd: &mut cwd.clone(),
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let find_result = Find.execute(ctx_find).await.unwrap();

        // Run ls -R
        let args_ls = vec!["-R".to_string()];
        let ctx_ls = Context {
            args: &args_ls,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let ls_result = Ls.execute(ctx_ls).await.unwrap();

        // Both should find all the nested content
        assert!(find_result.stdout.contains("top"));
        assert!(find_result.stdout.contains("middle"));
        assert!(find_result.stdout.contains("bottom"));
        assert!(find_result.stdout.contains("a.txt"));
        assert!(find_result.stdout.contains("b.txt"));
        assert!(find_result.stdout.contains("c.txt"));

        assert!(ls_result.stdout.contains("top"));
        assert!(ls_result.stdout.contains("middle"));
        assert!(ls_result.stdout.contains("bottom"));
        assert!(ls_result.stdout.contains("a.txt"));
        assert!(ls_result.stdout.contains("b.txt"));
        assert!(ls_result.stdout.contains("c.txt"));
    }

    #[tokio::test]
    async fn test_find_with_empty_subdirs() {
        // Ensure empty subdirectories are still traversed
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create structure with some empty dirs in the path
        fs.mkdir(&cwd.join("a"), false).await.unwrap();
        fs.mkdir(&cwd.join("a/empty1"), false).await.unwrap();
        fs.mkdir(&cwd.join("a/empty2"), false).await.unwrap();
        fs.mkdir(&cwd.join("a/empty1/deep"), false).await.unwrap();
        fs.write_file(&cwd.join("a/empty1/deep/file.txt"), b"found")
            .await
            .unwrap();

        let args: Vec<String> = vec![];
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
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Should find the file through the empty directories
        assert!(result.stdout.contains("file.txt"));
        assert!(result.stdout.contains("empty1"));
        assert!(result.stdout.contains("empty2"));
        assert!(result.stdout.contains("deep"));
    }

    #[tokio::test]
    async fn test_find_from_specific_path() {
        // Test finding from a specific starting path (not cwd)
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create nested structure
        fs.mkdir(&cwd.join("start"), false).await.unwrap();
        fs.mkdir(&cwd.join("start/sub1"), false).await.unwrap();
        fs.mkdir(&cwd.join("start/sub1/sub2"), false).await.unwrap();
        fs.write_file(&cwd.join("start/sub1/sub2/target.txt"), b"target")
            .await
            .unwrap();

        // Find from a specific starting path
        let args = vec!["start".to_string()];
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
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        assert!(
            result.stdout.contains("start"),
            "Should contain starting path"
        );
        assert!(result.stdout.contains("sub1"), "Should descend into sub1");
        assert!(result.stdout.contains("sub2"), "Should descend into sub2");
        assert!(
            result.stdout.contains("target.txt"),
            "Should find target.txt"
        );
    }

    // ==================== rmdir tests ====================

    #[tokio::test]
    async fn test_rmdir_empty() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.mkdir(&cwd.join("emptydir"), false).await.unwrap();

        let args = vec!["emptydir".to_string()];
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
        };

        let result = Rmdir.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!fs.exists(&cwd.join("emptydir")).await.unwrap());
    }

    #[tokio::test]
    async fn test_rmdir_not_empty() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.mkdir(&cwd.join("notempty"), false).await.unwrap();
        fs.write_file(&cwd.join("notempty/file.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["notempty".to_string()];
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
        };

        let result = Rmdir.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("not empty"));
    }

    #[tokio::test]
    async fn test_rmdir_nonexistent() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["nonexistent".to_string()];
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
        };

        let result = Rmdir.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("No such file or directory"));
    }

    #[tokio::test]
    async fn test_rmdir_not_directory() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("file.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["file.txt".to_string()];
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
        };

        let result = Rmdir.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("Not a directory"));
    }

    #[tokio::test]
    async fn test_rmdir_parents() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.mkdir(&cwd.join("a/b/c"), true).await.unwrap();

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
        };

        let result = Rmdir.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!fs.exists(&cwd.join("a/b/c")).await.unwrap());
        assert!(!fs.exists(&cwd.join("a/b")).await.unwrap());
        assert!(!fs.exists(&cwd.join("a")).await.unwrap());
    }

    #[tokio::test]
    async fn test_rmdir_missing_operand() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
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
        };

        let result = Rmdir.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing operand"));
    }

    // ==================== Custom filesystem tests ====================

    #[tokio::test]
    async fn test_find_with_overlay_fs() {
        use crate::fs::OverlayFs;

        // Create base filesystem with nested structure
        let base = Arc::new(InMemoryFs::new());
        base.mkdir(Path::new("/home/user"), true).await.unwrap();
        base.mkdir(Path::new("/home/user/base"), false)
            .await
            .unwrap();
        base.mkdir(Path::new("/home/user/base/sub1"), false)
            .await
            .unwrap();
        base.mkdir(Path::new("/home/user/base/sub1/sub2"), false)
            .await
            .unwrap();
        base.write_file(Path::new("/home/user/base/file1.txt"), b"base1")
            .await
            .unwrap();
        base.write_file(Path::new("/home/user/base/sub1/file2.txt"), b"base2")
            .await
            .unwrap();
        base.write_file(Path::new("/home/user/base/sub1/sub2/file3.txt"), b"base3")
            .await
            .unwrap();

        // Create overlay
        let overlay: Arc<dyn FileSystem> = Arc::new(OverlayFs::new(base));

        // Add a file in the overlay layer (use recursive to ensure parent exists in upper)
        overlay
            .mkdir(Path::new("/home/user/base/overlay_dir"), true)
            .await
            .unwrap();
        overlay
            .write_file(
                Path::new("/home/user/base/overlay_dir/overlay_file.txt"),
                b"overlay",
            )
            .await
            .unwrap();

        let mut cwd = PathBuf::from("/home/user");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        // Run find on the overlay filesystem
        let args = vec!["base".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: overlay.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Should find files from base layer
        assert!(
            result.stdout.contains("file1.txt"),
            "Should find file1.txt from base"
        );
        assert!(
            result.stdout.contains("file2.txt"),
            "Should find file2.txt from base/sub1"
        );
        assert!(
            result.stdout.contains("file3.txt"),
            "Should find file3.txt from base/sub1/sub2"
        );

        // Should find files from overlay layer
        assert!(
            result.stdout.contains("overlay_dir"),
            "Should find overlay_dir"
        );
        assert!(
            result.stdout.contains("overlay_file.txt"),
            "Should find overlay_file.txt"
        );

        // Should descend into all subdirectories
        assert!(result.stdout.contains("sub1"), "Should find sub1");
        assert!(result.stdout.contains("sub2"), "Should find sub2");
    }

    #[tokio::test]
    async fn test_find_with_mountable_fs() {
        use crate::fs::MountableFs;

        // Create root filesystem
        let root = Arc::new(InMemoryFs::new());
        root.mkdir(Path::new("/home/user"), true).await.unwrap();
        root.write_file(Path::new("/home/user/root_file.txt"), b"root")
            .await
            .unwrap();

        // Create a nested filesystem to mount
        let nested = Arc::new(InMemoryFs::new());
        nested.mkdir(Path::new("/level1"), false).await.unwrap();
        nested
            .mkdir(Path::new("/level1/level2"), false)
            .await
            .unwrap();
        nested
            .mkdir(Path::new("/level1/level2/level3"), false)
            .await
            .unwrap();
        nested
            .write_file(Path::new("/level1/nested1.txt"), b"n1")
            .await
            .unwrap();
        nested
            .write_file(Path::new("/level1/level2/nested2.txt"), b"n2")
            .await
            .unwrap();
        nested
            .write_file(Path::new("/level1/level2/level3/nested3.txt"), b"n3")
            .await
            .unwrap();

        // Create mountable filesystem and mount nested at /home/user/mounted
        let mountable = MountableFs::new(root.clone());
        mountable
            .mount("/home/user/mounted", nested.clone())
            .unwrap();

        let fs: Arc<dyn FileSystem> = Arc::new(mountable);
        let mut cwd = PathBuf::from("/home/user");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        // Run find from cwd - should find both root files and mounted files
        let args: Vec<String> = vec![];
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
        };

        let result = Find.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Should find root file
        assert!(
            result.stdout.contains("root_file.txt"),
            "Should find root_file.txt"
        );

        // Should find mount point
        assert!(
            result.stdout.contains("mounted"),
            "Should find mounted directory"
        );

        // Should descend into mounted filesystem
        assert!(
            result.stdout.contains("level1"),
            "Should find level1 in mounted fs"
        );
        assert!(
            result.stdout.contains("level2"),
            "Should find level2 in mounted fs"
        );
        assert!(
            result.stdout.contains("level3"),
            "Should find level3 in mounted fs"
        );

        // Should find files deep in mounted filesystem
        assert!(
            result.stdout.contains("nested1.txt"),
            "Should find nested1.txt"
        );
        assert!(
            result.stdout.contains("nested2.txt"),
            "Should find nested2.txt"
        );
        assert!(
            result.stdout.contains("nested3.txt"),
            "Should find nested3.txt"
        );
    }

    #[tokio::test]
    async fn test_ls_recursive_with_overlay_fs() {
        use crate::fs::OverlayFs;

        // Create base filesystem with nested structure
        let base = Arc::new(InMemoryFs::new());
        base.mkdir(Path::new("/home/user"), true).await.unwrap();
        base.mkdir(Path::new("/home/user/dir"), false)
            .await
            .unwrap();
        base.mkdir(Path::new("/home/user/dir/subdir"), false)
            .await
            .unwrap();
        base.write_file(Path::new("/home/user/dir/base.txt"), b"base")
            .await
            .unwrap();
        base.write_file(Path::new("/home/user/dir/subdir/deep.txt"), b"deep")
            .await
            .unwrap();

        let overlay: Arc<dyn FileSystem> = Arc::new(OverlayFs::new(base));

        let mut cwd = PathBuf::from("/home/user");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        let args = vec!["-R".to_string(), "dir".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: overlay,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        assert!(result.stdout.contains("base.txt"), "Should list base.txt");
        assert!(result.stdout.contains("subdir"), "Should list subdir");
        assert!(result.stdout.contains("deep.txt"), "Should list deep.txt");
    }

    #[tokio::test]
    async fn test_ls_recursive_with_mountable_fs() {
        use crate::fs::MountableFs;

        let root = Arc::new(InMemoryFs::new());
        root.mkdir(Path::new("/home/user"), true).await.unwrap();

        let mounted = Arc::new(InMemoryFs::new());
        mounted.mkdir(Path::new("/a"), false).await.unwrap();
        mounted.mkdir(Path::new("/a/b"), false).await.unwrap();
        mounted
            .write_file(Path::new("/a/file_a.txt"), b"a")
            .await
            .unwrap();
        mounted
            .write_file(Path::new("/a/b/file_b.txt"), b"b")
            .await
            .unwrap();

        let mountable = MountableFs::new(root);
        mountable.mount("/home/user/mnt", mounted.clone()).unwrap();

        let fs: Arc<dyn FileSystem> = Arc::new(mountable);
        let mut cwd = PathBuf::from("/home/user");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        let args = vec!["-R".to_string(), "mnt".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        assert!(result.stdout.contains("a"), "Should list directory a");
        assert!(result.stdout.contains("b"), "Should list directory b");
        assert!(
            result.stdout.contains("file_a.txt"),
            "Should list file_a.txt"
        );
        assert!(
            result.stdout.contains("file_b.txt"),
            "Should list file_b.txt"
        );
    }

    // ==================== root directory tests ====================

    #[tokio::test]
    async fn test_ls_root_directory() {
        // Test listing the root directory directly
        let fs = Arc::new(InMemoryFs::new());
        let mut cwd = PathBuf::from("/home/user");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        // ls / should work
        let args = vec!["/".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "ls / should succeed: {}",
            result.stderr
        );
        // Root should contain at least tmp, home, dev
        assert!(result.stdout.contains("tmp"), "Root should contain tmp");
        assert!(result.stdout.contains("home"), "Root should contain home");
        assert!(result.stdout.contains("dev"), "Root should contain dev");
    }

    #[tokio::test]
    async fn test_ls_dot_from_root() {
        // Test: when cwd is /, ls . should list root contents
        let fs = Arc::new(InMemoryFs::new());
        let mut cwd = PathBuf::from("/");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        // ls . with cwd=/ should work
        let args = vec![".".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "ls . from / should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("tmp"), "Root should contain tmp");
        assert!(result.stdout.contains("home"), "Root should contain home");
    }

    #[tokio::test]
    async fn test_ls_default_from_root() {
        // Test: when cwd is /, ls (no args) should list root contents
        let fs = Arc::new(InMemoryFs::new());
        let mut cwd = PathBuf::from("/");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        // ls with no args and cwd=/ should work
        let args: Vec<String> = vec![];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "ls from / should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("tmp"), "Root should contain tmp");
        assert!(result.stdout.contains("home"), "Root should contain home");
    }

    #[tokio::test]
    async fn test_ls_root_with_overlay_fs() {
        // Test: ls / with OverlayFs should work
        use crate::fs::OverlayFs;

        let base = Arc::new(InMemoryFs::new());
        let overlay: Arc<dyn FileSystem> = Arc::new(OverlayFs::new(base));

        let mut cwd = PathBuf::from("/");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        // ls / should work with overlay
        let args = vec!["/".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: overlay.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "ls / with overlay should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("tmp"), "Root should contain tmp");
        assert!(result.stdout.contains("home"), "Root should contain home");
    }

    #[tokio::test]
    async fn test_ls_dot_from_root_with_overlay_fs() {
        // Test: cd / && ls . with OverlayFs should work
        use crate::fs::OverlayFs;

        let base = Arc::new(InMemoryFs::new());
        let overlay: Arc<dyn FileSystem> = Arc::new(OverlayFs::new(base));

        let mut cwd = PathBuf::from("/");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        // ls . from / with overlay
        let args = vec![".".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: overlay.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "ls . from / with overlay should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("tmp"), "Root should contain tmp");
        assert!(result.stdout.contains("home"), "Root should contain home");
    }

    #[tokio::test]
    async fn test_resolve_path_slash_dot_normalized() {
        // Verify that "/." path (from /join(".")) resolves correctly to root
        let fs = Arc::new(InMemoryFs::new());

        // The path "/." should normalize to "/" and exist
        assert!(fs.exists(Path::new("/.")).await.unwrap(), "/. should exist");
        assert!(fs.exists(Path::new("/")).await.unwrap(), "/ should exist");

        // Both should return the same stat info
        let stat_root = fs.stat(Path::new("/")).await.unwrap();
        let stat_dot = fs.stat(Path::new("/.")).await.unwrap();
        assert!(stat_root.file_type.is_dir());
        assert!(stat_dot.file_type.is_dir());
    }

    // ==================== negative tests ====================

    #[tokio::test]
    async fn test_ls_nonexistent_path() {
        // Negative test: ls on path that doesn't exist should fail
        let fs = Arc::new(InMemoryFs::new());
        let mut cwd = PathBuf::from("/home/user");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        let args = vec!["/nonexistent/path".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2, "ls on nonexistent path should fail");
        assert!(
            result.stderr.contains("No such file or directory"),
            "Should report file not found"
        );
    }

    #[tokio::test]
    async fn test_ls_path_traversal_normalized() {
        // Positive test: path traversal with .. should be normalized and work
        let fs = Arc::new(InMemoryFs::new());
        let mut cwd = PathBuf::from("/home/user");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        // /home/user/../user should normalize to /home/user
        let args = vec!["../user".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        // /home/user is an empty directory by default, so it should succeed with empty output
        assert_eq!(
            result.exit_code, 0,
            "ls with .. should succeed after normalization: {}",
            result.stderr
        );
    }

    #[tokio::test]
    async fn test_ls_excessive_dotdot_stays_at_root() {
        // Positive test: excessive .. should stay at root
        let fs = Arc::new(InMemoryFs::new());
        let mut cwd = PathBuf::from("/home/user");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        // ../../../../.. from /home/user should normalize to /
        let args = vec!["../../../../..".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "Excessive .. should normalize to root: {}",
            result.stderr
        );
        // Should list root contents
        assert!(result.stdout.contains("tmp"), "Should list root (tmp)");
        assert!(result.stdout.contains("home"), "Should list root (home)");
    }

    #[tokio::test]
    async fn test_ls_dot_in_middle_of_path() {
        // Positive test: . in middle of path should be normalized
        let fs = Arc::new(InMemoryFs::new());
        let mut cwd = PathBuf::from("/");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        // /./home/./user/. should normalize to /home/user
        let args = vec!["./home/./user/.".to_string()];
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
        };

        let result = Ls.execute(ctx).await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "Path with . components should work: {}",
            result.stderr
        );
    }

    // ==================== glob_match tests ====================

    #[test]
    fn test_glob_match_star() {
        assert!(glob_match("file.txt", "*.txt"));
        assert!(glob_match("test.txt", "*.txt"));
        assert!(!glob_match("file.md", "*.txt"));
    }

    #[test]
    fn test_glob_match_question() {
        assert!(glob_match("ab", "a?"));
        assert!(glob_match("ac", "a?"));
        assert!(!glob_match("abc", "a?"));
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("test", "test"));
        assert!(!glob_match("test", "other"));
    }

    #[test]
    fn test_glob_match_star_middle() {
        assert!(glob_match("test.backup.txt", "test*.txt"));
        assert!(glob_match("test.txt", "test*.txt"));
    }
}
