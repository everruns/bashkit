//! ls builtin - list directory contents.

// Uses unwrap() for validated single-char strings.
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use std::ffi::OsString;
use std::path::Path;

use crate::builtins::clap_env::apply_env_defaults;
use crate::builtins::generated::ls_args::{LS_ENV_DEFAULTS, ls_command};
use crate::builtins::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::fs::FileType;
use crate::interpreter::ExecResult;

/// Argument IDs from the generated `ls_command()` that bashkit currently
/// implements. Anything else clap accepts is reported as "not yet
/// implemented" so scripts get a deterministic error instead of silently
/// missing behavior. See spec `coreutils-args-port.md`.
const LS_SUPPORTED_IDS: &[&str] = &[
    // The 8 short flags the original bashkit ls accepted.
    "long",           // -l
    "all",            // -a
    "human-readable", // -h
    "1",              // -1
    "recursive",      // -R
    "t",              // -t
    "classify",       // -F / --classify
    "C",              // -C
    "directory",      // -d / --directory
    // Non-flag positional + always-supported infrastructure.
    "paths",
    "help",
];

/// Options for ls command
pub(super) struct LsOptions {
    pub(super) long: bool,
    pub(super) all: bool,
    pub(super) human: bool,
    pub(super) one_per_line: bool,
    pub(super) recursive: bool,
    pub(super) sort_by_time: bool,
    pub(super) classify: bool,
    pub(super) columns: bool,
    pub(super) directory: bool,
}

/// The ls builtin - list directory contents.
///
/// Usage: ls [-l] [-a] [-h] [-1] [-R] [-t] [-F] [-C] [PATH...]
///
/// Options:
///   -l   Use long listing format
///   -a   Show hidden files (starting with .)
///   -h   Human-readable sizes (with -l)
///   -1   One entry per line
///   -R   List subdirectories recursively
///   -t   Sort by modification time, newest first
///   -F   Append indicator (/ for dirs, * for executables, @ for symlinks, | for FIFOs)
///   -C   List entries in columns (multi-column output)
///   -d   List directories themselves, not their contents
pub struct Ls;

#[async_trait]
impl Builtin for Ls {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // clap expects argv[0] = program name; bashkit's ctx.args excludes it.
        let argv: Vec<OsString> = std::iter::once(OsString::from("ls"))
            .chain(ctx.args.iter().map(OsString::from))
            .collect();
        // Layer bashkit's virtual env over argv before clap parses it.
        // Mirrors uutils' `Arg::env(...)` precedence (argv > env > default)
        // but sources values from `ctx.env` instead of the host process —
        // see TM-INF-024 and `builtins/clap_env.rs`.
        let argv = apply_env_defaults(argv, LS_ENV_DEFAULTS, ctx.env);

        // GNU coreutils' help layout opens with the usage line; clap's
        // default template leads with the `about`. uutils handles this via
        // uucore's `localized_help_template`, which we drop during codegen
        // because it pulls in Fluent. Re-apply a GNU-equivalent template.
        let cmd = ls_command().help_template("Usage: {usage}\n{about}\n\n{all-args}\n");
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

        // Reject any uu_ls flag bashkit hasn't implemented yet. Reporting
        // them up front beats silently parsing them and producing GNU-
        // incompatible output. Same pattern as `tac -b/-r/-s`.
        let unsupported: Vec<String> = matches
            .ids()
            .filter(|id| {
                let name = id.as_str();
                !LS_SUPPORTED_IDS.contains(&name)
                    && matches.value_source(name) != Some(clap::parser::ValueSource::DefaultValue)
            })
            .map(|id| id.as_str().to_string())
            .collect();
        if !unsupported.is_empty() {
            return Ok(ExecResult::err(
                format!(
                    "ls: option(s) not yet implemented in bashkit: {}\n",
                    unsupported.join(", ")
                ),
                2,
            ));
        }

        // -F/--classify takes an optional value (`always`/`auto`/`never`).
        // The bashkit implementation only knows "classify or not"; treat
        // an explicit `never` as off and anything else (including the
        // default "always") as on.
        let classify = matches.contains_id("classify")
            && matches
                .get_one::<String>("classify")
                .map(|v| v != "never")
                .unwrap_or(true);

        let opts = LsOptions {
            long: matches.get_flag("long"),
            all: matches.get_flag("all"),
            human: matches.get_flag("human-readable"),
            one_per_line: matches.get_flag("1"),
            recursive: matches.get_flag("recursive"),
            sort_by_time: matches.get_flag("t"),
            classify,
            columns: matches.get_flag("C"),
            directory: matches.get_flag("directory"),
        };

        // PATHS holds OsString values; convert to owned strings for the
        // existing rendering loop.
        let paths_owned: Vec<String> = matches
            .get_many::<OsString>("paths")
            .map(|vs| vs.map(|v| v.to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        let mut paths: Vec<&str> = paths_owned.iter().map(String::as_str).collect();

        // Default to current directory
        if paths.is_empty() {
            paths.push(".");
        }

        let mut output = String::new();
        let multiple_paths = paths.len() > 1 || opts.recursive;

        // Separate file and directory arguments (like real ls)
        let mut file_args: Vec<(&str, crate::fs::Metadata)> = Vec::new();
        let mut dir_args: Vec<(usize, &str, std::path::PathBuf)> = Vec::new();

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

            let metadata = ctx.fs.stat(&path).await?;

            // With -d, a directory operand is listed as itself (like a file
            // argument) rather than being descended into. POSIX: enables the
            // common `ls -d */` idiom to enumerate subdirectories.
            if metadata.file_type.is_file() || opts.directory {
                file_args.push((path_str, metadata));
            } else {
                dir_args.push((i, path_str, path));
            }
        }

        // Sort file arguments by time if -t, preserving original paths
        if opts.sort_by_time {
            file_args.sort_by_key(|entry| std::cmp::Reverse(entry.1.modified));
        }

        // Output file arguments first (preserving path as given by user)
        if opts.long {
            for (path_str, metadata) in &file_args {
                let mut entry = format_long_entry(path_str, metadata, opts.human);
                if opts.classify {
                    // Insert suffix before the trailing newline
                    let suffix = classify_suffix(metadata);
                    if !suffix.is_empty() {
                        entry.insert_str(entry.len() - 1, suffix);
                    }
                }
                output.push_str(&entry);
            }
        } else if !file_args.is_empty() {
            let names: Vec<String> = file_args
                .iter()
                .map(|(path_str, metadata)| {
                    let mut name = (*path_str).to_string();
                    if opts.classify {
                        name.push_str(classify_suffix(metadata));
                    }
                    name
                })
                .collect();
            if opts.columns && !opts.one_per_line {
                output.push_str(&format_columns(&names, 80));
            } else {
                for name in &names {
                    output.push_str(name);
                    output.push('\n');
                }
            }
        }

        // Then output directory listings
        for (i, path_str, path) in &dir_args {
            if let Err(e) = list_directory(
                &ctx,
                path,
                path_str,
                &mut output,
                &opts,
                multiple_paths,
                *i > 0 || !file_args.is_empty(),
            )
            .await
            {
                return Ok(ExecResult::err(format!("ls: {}\n", e), 2));
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
        sorted_entries.sort_by_key(|entry| std::cmp::Reverse(entry.metadata.modified));
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
            let mut line = format_long_entry(&entry.name, &entry.metadata, opts.human);
            if opts.classify {
                let suffix = classify_suffix(&entry.metadata);
                if !suffix.is_empty() {
                    line.insert_str(line.len() - 1, suffix);
                }
            }
            output.push_str(&line);
            if opts.recursive && entry.metadata.file_type.is_dir() {
                subdirs.push((
                    path.join(&entry.name),
                    format!("{}/{}", display_path, entry.name),
                ));
            }
        }
    } else {
        // Collect entry names for potential column formatting
        let mut names: Vec<String> = Vec::new();
        for entry in &filtered {
            let mut name = entry.name.clone();
            if opts.classify {
                name.push_str(classify_suffix(&entry.metadata));
            }
            names.push(name);
            if opts.recursive && entry.metadata.file_type.is_dir() {
                subdirs.push((
                    path.join(&entry.name),
                    format!("{}/{}", display_path, entry.name),
                ));
            }
        }

        // Precedence: -l > -1 > -C > default (one-per-line)
        if opts.columns && !opts.one_per_line {
            output.push_str(&format_columns(&names, 80));
        } else {
            for name in &names {
                output.push_str(name);
                output.push('\n');
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

/// Return the classify indicator character for a file type.
/// `/` for directories, `*` for executables, `@` for symlinks, `|` for FIFOs.
pub(super) fn classify_suffix(metadata: &crate::fs::Metadata) -> &'static str {
    match metadata.file_type {
        FileType::Directory => "/",
        FileType::Symlink => "@",
        FileType::Fifo => "|",
        FileType::File => {
            // Executable if any execute bit is set
            if metadata.mode & 0o111 != 0 { "*" } else { "" }
        }
    }
}

/// Format entries in column-major order, like `ls -C`.
/// Uses a fixed terminal width (80) since VFS has no real terminal.
/// Per-column widths match GNU coreutils behavior.
pub(super) fn format_columns(entries: &[String], terminal_width: usize) -> String {
    if entries.is_empty() {
        return String::new();
    }

    // Try fitting as many columns as possible, starting from the maximum
    let max_width = entries.iter().map(|e| e.len()).max().unwrap_or(0);
    let max_possible_cols = (terminal_width / (max_width.min(1) + 2)).max(1);

    let mut num_cols = 1;
    let mut col_widths: Vec<usize> = vec![0];
    let mut num_rows = entries.len();

    // Try increasing column counts to find the best fit
    for try_cols in 2..=max_possible_cols.min(entries.len()) {
        let try_rows = entries.len().div_ceil(try_cols);
        // Calculate per-column widths (max entry width in each column)
        let mut widths = vec![0usize; try_cols];
        for (i, entry) in entries.iter().enumerate() {
            let col = i / try_rows;
            if col < try_cols {
                widths[col] = widths[col].max(entry.len());
            }
        }
        // Total width: each column except last gets 2-space padding
        let total: usize = widths.iter().sum::<usize>() + (try_cols - 1) * 2;
        if total <= terminal_width {
            num_cols = try_cols;
            col_widths = widths;
            num_rows = try_rows;
        }
    }

    let mut output = String::new();
    for row in 0..num_rows {
        for (col, col_w) in col_widths.iter().enumerate() {
            // Column-major order: fill columns top-to-bottom, left-to-right
            let idx = col * num_rows + row;
            if idx < entries.len() {
                let is_last = col == num_cols - 1 || idx + num_rows >= entries.len();
                if is_last {
                    output.push_str(&entries[idx]);
                } else {
                    let width = col_w + 2; // entry width + 2 spaces
                    output.push_str(&format!("{:<width$}", entries[idx], width = width));
                }
            }
        }
        output.push('\n');
    }
    output
}

pub(super) fn format_long_entry(name: &str, metadata: &crate::fs::Metadata, human: bool) -> String {
    let file_type = match metadata.file_type {
        FileType::Directory => 'd',
        FileType::Symlink => 'l',
        FileType::Fifo => 'p',
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
