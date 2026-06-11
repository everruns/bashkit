//! find builtin - search for files.

#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use std::path::Path;

use super::glob_match;
use crate::builtins::{Builtin, Context, ExecutionPlan, SubCommand, resolve_path};
use crate::error::Result;
use crate::interpreter::{ControlFlow, ExecResult};

/// Options for find command
pub(super) struct FindOptions {
    pub(super) name_pattern: Option<String>,
    /// -path pattern: match against the full display path
    pub(super) path_pattern: Option<String>,
    pub(super) type_filter: Option<char>,
    pub(super) max_depth: Option<usize>,
    pub(super) min_depth: Option<usize>,
    pub(super) printf_format: Option<String>,
    pub(super) print0: bool,
    /// -exec/-execdir command template (args before \; or +)
    pub(super) exec_args: Vec<String>,
    /// true if -exec uses + (batch mode), false for \; (per-file mode)
    pub(super) exec_batch: bool,
    /// Negate the -name predicate
    pub(super) negate_name: bool,
    /// Negate the -path predicate
    pub(super) negate_path: bool,
    /// Negate the -type predicate
    pub(super) negate_type: bool,
}

/// The find builtin - search for files.
///
/// Usage: find [PATH...] [-name PATTERN] [-type TYPE] [-maxdepth N] [-mindepth N] [-printf FMT] [-exec CMD {} \;]
///
/// Options:
///   -name PATTERN      Match filename against PATTERN (supports * and ?)
///   -type TYPE         Match file type: f (file), d (directory), l (link)
///   -maxdepth N        Descend at most N levels
///   -mindepth N        Do not apply tests at levels less than N
///   -print             Print matching paths (default)
///   -printf FMT        Print using format string (%f %p %P %s %m %M %y %d %T@)
///   -exec CMD {} \;    Execute CMD for each match ({} = path)
///   -exec CMD {} +     Execute CMD once with all matches
pub struct Find;

/// Parse find arguments into search paths and options.
/// Returns (paths, opts) or an error ExecResult.
#[allow(clippy::result_large_err)]
pub(super) fn parse_find_args(
    args: &[String],
) -> std::result::Result<(Vec<String>, FindOptions), ExecResult> {
    let mut paths: Vec<String> = Vec::new();
    let mut opts = FindOptions {
        name_pattern: None,
        path_pattern: None,
        type_filter: None,
        max_depth: None,
        min_depth: None,
        printf_format: None,
        print0: false,
        exec_args: Vec::new(),
        exec_batch: false,
        negate_name: false,
        negate_path: false,
        negate_type: false,
    };
    let mut negate_next = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-name" => {
                i += 1;
                if i >= args.len() {
                    return Err(ExecResult::err(
                        "find: missing argument to '-name'\n".to_string(),
                        1,
                    ));
                }
                opts.name_pattern = Some(args[i].clone());
                if negate_next {
                    opts.negate_name = true;
                    negate_next = false;
                }
            }
            "-path" => {
                i += 1;
                if i >= args.len() {
                    return Err(ExecResult::err(
                        "find: missing argument to '-path'\n".to_string(),
                        1,
                    ));
                }
                opts.path_pattern = Some(args[i].clone());
                if negate_next {
                    opts.negate_path = true;
                    negate_next = false;
                }
            }
            "-type" => {
                i += 1;
                if i >= args.len() {
                    return Err(ExecResult::err(
                        "find: missing argument to '-type'\n".to_string(),
                        1,
                    ));
                }
                let t = &args[i];
                match t.as_str() {
                    "f" | "d" | "l" => {
                        opts.type_filter = Some(t.chars().next().unwrap());
                        opts.negate_type = negate_next;
                        negate_next = false;
                    }
                    _ => {
                        return Err(ExecResult::err(format!("find: unknown type '{}'\n", t), 1));
                    }
                }
            }
            "-maxdepth" => {
                i += 1;
                if i >= args.len() {
                    return Err(ExecResult::err(
                        "find: missing argument to '-maxdepth'\n".to_string(),
                        1,
                    ));
                }
                match args[i].parse::<usize>() {
                    Ok(n) => opts.max_depth = Some(n),
                    Err(_) => {
                        return Err(ExecResult::err(
                            format!("find: invalid maxdepth value '{}'\n", args[i]),
                            1,
                        ));
                    }
                }
                // Consume unsupported negation targets so ! cannot leak to a later test.
                negate_next = false;
            }
            "-mindepth" => {
                i += 1;
                if i >= args.len() {
                    return Err(ExecResult::err(
                        "find: missing argument to '-mindepth'\n".to_string(),
                        1,
                    ));
                }
                match args[i].parse::<usize>() {
                    Ok(n) => opts.min_depth = Some(n),
                    Err(_) => {
                        return Err(ExecResult::err(
                            format!("find: invalid mindepth value '{}'\n", args[i]),
                            1,
                        ));
                    }
                }
                // Consume unsupported negation targets so ! cannot leak to a later test.
                negate_next = false;
            }
            "-print" => {
                // Default action, ignore. Consume ! so it cannot leak to a later test.
                negate_next = false;
            }
            "-print0" => {
                opts.print0 = true;
                negate_next = false;
            }
            "-printf" => {
                i += 1;
                if i >= args.len() {
                    return Err(ExecResult::err(
                        "find: missing argument to '-printf'\n".to_string(),
                        1,
                    ));
                }
                opts.printf_format = Some(args[i].clone());
                // Consume unsupported negation targets so ! cannot leak to a later test.
                negate_next = false;
            }
            "-exec" | "-execdir" => {
                i += 1;
                while i < args.len() {
                    let a = &args[i];
                    if a == ";" || a == "\\;" {
                        break;
                    }
                    if a == "+" {
                        opts.exec_batch = true;
                        break;
                    }
                    opts.exec_args.push(a.clone());
                    i += 1;
                }
                // Consume unsupported negation targets so ! cannot leak to a later test.
                negate_next = false;
            }
            "-not" | "!" => {
                negate_next = true;
            }
            s if s.starts_with('-') => {
                return Err(ExecResult::err(
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

    if negate_next {
        return Err(ExecResult::err(
            "find: missing predicate after '-not'\n".to_string(),
            1,
        ));
    }

    if paths.is_empty() {
        paths.push(".".to_string());
    }

    Ok((paths, opts))
}

pub(super) struct FindPlanData {
    pub(super) matched_paths: Vec<String>,
    pub(super) errors: String,
    pub(super) had_error: bool,
}

/// Collect matched paths and path/traversal errors for find execution planning.
async fn collect_find_plan_data(
    ctx: &Context<'_>,
    search_paths: &[String],
    opts: &FindOptions,
) -> Result<FindPlanData> {
    let mut matched: Vec<String> = Vec::new();
    let mut errors = String::new();
    let mut had_error = false;
    // Reuse find_recursive but with a temporary output buffer
    let temp_opts = FindOptions {
        name_pattern: opts.name_pattern.clone(),
        path_pattern: opts.path_pattern.clone(),
        type_filter: opts.type_filter,
        max_depth: opts.max_depth,
        min_depth: opts.min_depth,
        printf_format: None, // Don't format, just collect paths
        print0: false,
        exec_args: Vec::new(),
        exec_batch: false,
        negate_name: opts.negate_name,
        negate_path: opts.negate_path,
        negate_type: opts.negate_type,
    };
    let mut output = String::new();
    for path_str in search_paths {
        let path = resolve_path(ctx.cwd, path_str);
        if !ctx.fs.exists(&path).await.unwrap_or(false) {
            errors.push_str(&format!(
                "find: '{}': No such file or directory\n",
                path_str
            ));
            had_error = true;
            continue;
        }
        if let Err(e) = find_recursive(ctx, &path, path_str, &temp_opts, 0, &mut output).await {
            errors.push_str(&format!("find: '{}': {}\n", path_str, e));
            had_error = true;
        }
    }
    // Parse the output back into paths (each line is a path)
    for line in output.lines() {
        if !line.is_empty() {
            matched.push(line.to_string());
        }
    }
    Ok(FindPlanData {
        matched_paths: matched,
        errors,
        had_error,
    })
}

/// Build exec sub-commands from matched paths and exec_args template.
pub(super) fn build_find_exec_commands(
    exec_args: &[String],
    matched_paths: &[String],
    batch: bool,
) -> Vec<SubCommand> {
    if exec_args.is_empty() || matched_paths.is_empty() {
        return Vec::new();
    }

    if batch {
        // Batch mode: -exec cmd {} +
        // Replace {} with all paths at once
        let cmd_args: Vec<String> = exec_args
            .iter()
            .flat_map(|arg| {
                if arg == "{}" {
                    matched_paths.to_vec()
                } else {
                    vec![arg.clone()]
                }
            })
            .collect();

        if cmd_args.is_empty() {
            return Vec::new();
        }

        vec![SubCommand {
            name: cmd_args[0].clone(),
            args: cmd_args[1..].to_vec(),
            stdin: None,
        }]
    } else {
        // Per-file mode: -exec cmd {} \;
        matched_paths
            .iter()
            .map(|found_path| {
                let cmd_args: Vec<String> = exec_args
                    .iter()
                    .map(|arg| arg.replace("{}", found_path))
                    .collect();

                SubCommand {
                    name: cmd_args[0].clone(),
                    args: cmd_args[1..].to_vec(),
                    stdin: None,
                }
            })
            .collect()
    }
}

#[async_trait]
impl Builtin for Find {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = crate::builtins::check_help_version(
            ctx.args,
            "Usage: find [PATH...] [EXPRESSION]\nSearch for files in a directory hierarchy.\n\n  -name PATTERN\tmatch filename against PATTERN (supports * and ?)\n  -path PATTERN\tmatch full path against PATTERN\n  -type TYPE\tmatch file type: f (file), d (directory), l (link)\n  -maxdepth N\tdescend at most N levels\n  -mindepth N\tdo not apply tests at levels less than N\n  -print\t\tprint matching paths (default)\n  -printf FMT\tprint using format string (%f %p %P %s %m %M %y %d %T@)\n  -exec CMD {} \\;\texecute CMD for each match ({} = path)\n  -exec CMD {} +\texecute CMD once with all matches\n  -not, !\t\tnegate the next predicate\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n",
            Some("find (bashkit) 0.1"),
        ) {
            return Ok(r);
        }

        let (search_paths, opts) = match parse_find_args(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };

        let mut output = String::new();
        let mut errors = String::new();
        let mut had_error = false;

        for path_str in &search_paths {
            let path = resolve_path(ctx.cwd, path_str);
            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                errors.push_str(&format!(
                    "find: '{}': No such file or directory\n",
                    path_str
                ));
                had_error = true;
                continue;
            }

            if let Err(e) = find_recursive(&ctx, &path, path_str, &opts, 0, &mut output).await {
                errors.push_str(&format!("find: '{}': {}\n", path_str, e));
                had_error = true;
            }
        }

        Ok(ExecResult {
            stdout: output,
            stderr: errors,
            exit_code: if had_error { 1 } else { 0 },
            control_flow: ControlFlow::None,
            ..Default::default()
        })
    }

    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        let (search_paths, opts) = match parse_find_args(ctx.args) {
            Ok(v) => v,
            Err(_) => return Ok(None), // Let execute() handle errors
        };

        // Only return a plan when -exec is present
        if opts.exec_args.is_empty() {
            return Ok(None);
        }

        // Collect matched paths plus any path/traversal errors.
        let plan_data = collect_find_plan_data(ctx, &search_paths, &opts).await?;
        if plan_data.matched_paths.is_empty() && !plan_data.had_error {
            return Ok(None);
        }

        let commands =
            build_find_exec_commands(&opts.exec_args, &plan_data.matched_paths, opts.exec_batch);
        if commands.is_empty() && !plan_data.had_error {
            return Ok(None);
        }

        if plan_data.had_error {
            return Ok(Some(ExecutionPlan::BatchWithStatus {
                commands,
                stderr_prefix: plan_data.errors,
                force_error_exit: true,
            }));
        }

        Ok(Some(ExecutionPlan::Batch { commands }))
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
            Some('f') => {
                let m = metadata.file_type.is_file();
                if opts.negate_type { !m } else { m }
            }
            Some('d') => {
                let m = metadata.file_type.is_dir();
                if opts.negate_type { !m } else { m }
            }
            Some('l') => {
                let m = metadata.file_type.is_symlink();
                if opts.negate_type { !m } else { m }
            }
            _ => true,
        };

        // Check name pattern
        let name_matches = match &opts.name_pattern {
            Some(pattern) => {
                let m = glob_match(&entry_name, pattern);
                if opts.negate_name { !m } else { m }
            }
            None => true,
        };

        // Check path pattern
        let path_matches = match &opts.path_pattern {
            Some(pattern) => {
                let m = glob_match(display_path, pattern);
                if opts.negate_path { !m } else { m }
            }
            None => true,
        };

        // Check min depth before outputting
        let above_min_depth = match opts.min_depth {
            Some(min) => current_depth >= min,
            None => true,
        };

        // Output if matches (or if no filters, show everything)
        if type_matches && name_matches && path_matches && above_min_depth {
            if let Some(ref fmt) = opts.printf_format {
                output.push_str(&find_printf_format(fmt, display_path, &metadata));
            } else {
                output.push_str(display_path);
                output.push(if opts.print0 { '\0' } else { '\n' });
            }
        }

        // Recurse into directories
        if metadata.file_type.is_dir() {
            // Check max depth
            if let Some(max) = opts.max_depth
                && current_depth >= max
            {
                return Ok(());
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

/// Format a path using find's -printf format string.
fn find_printf_format(fmt: &str, display_path: &str, metadata: &crate::fs::Metadata) -> String {
    let mut out = String::new();
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                i += 1;
                if i < chars.len() {
                    match chars[i] {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        '0' => out.push('\0'),
                        '\\' => out.push('\\'),
                        c => {
                            out.push('\\');
                            out.push(c);
                        }
                    }
                }
            }
            '%' => {
                i += 1;
                if i >= chars.len() {
                    out.push('%');
                    continue;
                }
                match chars[i] {
                    'f' => {
                        let name = std::path::Path::new(display_path)
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| display_path.to_string());
                        out.push_str(&name);
                    }
                    'p' => out.push_str(display_path),
                    'P' => {
                        // In builtin context, display_path is already relative
                        let rel = display_path.strip_prefix("./").unwrap_or(display_path);
                        out.push_str(rel);
                    }
                    's' => out.push_str(&metadata.size.to_string()),
                    'm' => out.push_str(&format!("{:o}", metadata.mode & 0o7777)),
                    'M' => {
                        let type_ch = if metadata.file_type.is_dir() {
                            'd'
                        } else if metadata.file_type.is_symlink() {
                            'l'
                        } else {
                            '-'
                        };
                        out.push(type_ch);
                        for shift in [6, 3, 0] {
                            let bits = (metadata.mode >> shift) & 7;
                            out.push(if bits & 4 != 0 { 'r' } else { '-' });
                            out.push(if bits & 2 != 0 { 'w' } else { '-' });
                            out.push(if bits & 1 != 0 { 'x' } else { '-' });
                        }
                    }
                    'y' => {
                        let ch = if metadata.file_type.is_dir() {
                            'd'
                        } else if metadata.file_type.is_symlink() {
                            'l'
                        } else {
                            'f'
                        };
                        out.push(ch);
                    }
                    'd' => {
                        // Approximate depth from display_path
                        let base = display_path.strip_prefix("./").unwrap_or(display_path);
                        let depth = if base == "." || base.is_empty() {
                            0
                        } else {
                            base.matches('/').count() + 1
                        };
                        out.push_str(&depth.to_string());
                    }
                    'T' => {
                        i += 1;
                        if i < chars.len() && chars[i] == '@' {
                            let secs = metadata
                                .modified
                                .duration_since(crate::time::UNIX_EPOCH)
                                .ok()
                                .map(|d| d.as_secs())
                                .unwrap_or(0);
                            out.push_str(&secs.to_string());
                        } else {
                            out.push_str("%T");
                            continue;
                        }
                    }
                    '%' => out.push('%'),
                    c => {
                        out.push('%');
                        out.push(c);
                    }
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    out
}
