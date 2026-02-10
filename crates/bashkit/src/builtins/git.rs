//! Git builtin command.
//!
//! Provides virtual git operations on the virtual filesystem.
//!
//! # Supported Subcommands (Phase 1)
//!
//! - `git init [path]` - Create empty repository
//! - `git config [--global] <key> [value]` - Get/set config
//! - `git add <pathspec>...` - Stage files
//! - `git commit -m <message>` - Record changes
//! - `git status` - Show working tree status
//! - `git log [-n N]` - Show commit history
//!
//! # Security
//!
//! All operations are confined to the virtual filesystem and use
//! the configured author identity. See `specs/006-threat-model.md`
//! Section 9: Git Security (TM-GIT-*).
//!
//! # Example
//!
//! ```bash
//! git init /myrepo
//! cd /myrepo
//! echo "Hello" > README.md
//! git add README.md
//! git commit -m "Initial commit"
//! git log
//! ```

use async_trait::async_trait;

use super::{resolve_path, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Git builtin command.
pub struct Git;

#[async_trait]
impl super::Builtin for Git {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Check if git client is available
        #[cfg(feature = "git")]
        {
            if let Some(git_client) = ctx.git_client {
                return execute_git(ctx, git_client).await;
            }
        }

        // Git not configured
        Ok(ExecResult::err(
            "git: not configured\n\
             Note: Git operations require the 'git' feature and configuration via Bash::builder().git()\n"
                .to_string(),
            1,
        ))
    }
}

#[cfg(feature = "git")]
async fn execute_git(ctx: Context<'_>, git_client: &crate::git::GitClient) -> Result<ExecResult> {
    if ctx.args.is_empty() {
        return Ok(ExecResult::err(
            "usage: git <command> [<args>]\n\n\
             Available commands:\n\
             \tinit      Create an empty Git repository\n\
             \tconfig    Get and set repository options\n\
             \tadd       Add file contents to the index\n\
             \tcommit    Record changes to the repository\n\
             \tstatus    Show the working tree status\n\
             \tlog       Show commit logs\n\
             \tbranch    List, create, or delete branches\n\
             \tcheckout  Switch branches or restore files\n\
             \tdiff      Show changes (simplified)\n\
             \treset     Reset current HEAD\n\
             \tremote    Manage remotes\n\
             \tclone     Clone a repository (URL validation only)\n\
             \tfetch     Fetch from remote (URL validation only)\n\
             \tpush      Push to remote (URL validation only)\n\
             \tpull      Pull from remote (URL validation only)\n"
                .to_string(),
            1,
        ));
    }

    let subcommand = &ctx.args[0];
    let subargs = &ctx.args[1..];

    match subcommand.as_str() {
        "init" => git_init(ctx, git_client, subargs).await,
        "config" => git_config(ctx, git_client, subargs).await,
        "add" => git_add(ctx, git_client, subargs).await,
        "commit" => git_commit(ctx, git_client, subargs).await,
        "status" => git_status(ctx, git_client).await,
        "log" => git_log(ctx, git_client, subargs).await,
        "remote" => git_remote(ctx, git_client, subargs).await,
        "clone" => git_clone(ctx, git_client, subargs).await,
        "fetch" => git_fetch(ctx, git_client, subargs).await,
        "push" => git_push(ctx, git_client, subargs).await,
        "pull" => git_pull(ctx, git_client, subargs).await,
        "branch" => git_branch(ctx, git_client, subargs).await,
        "checkout" => git_checkout(ctx, git_client, subargs).await,
        "diff" => git_diff(ctx, git_client, subargs).await,
        "reset" => git_reset(ctx, git_client, subargs).await,
        _ => Ok(ExecResult::err(
            format!(
                "git: '{}' is not a git command. See 'git --help'.\n",
                subcommand
            ),
            1,
        )),
    }
}

#[cfg(feature = "git")]
async fn git_init(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    // Parse path argument (default to cwd)
    let path = if args.is_empty() {
        ctx.cwd.clone()
    } else {
        resolve_path(ctx.cwd, &args[0])
    };

    // Create directory if it doesn't exist
    if !ctx.fs.exists(&path).await? {
        ctx.fs.mkdir(&path, true).await?;
    }

    match git_client.init(&ctx.fs, &path).await {
        Ok(output) => Ok(ExecResult::ok(output)),
        Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
    }
}

#[cfg(feature = "git")]
async fn git_config(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    // Parse arguments
    let mut key: Option<&str> = None;
    let mut value: Option<&str> = None;
    let mut _global = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--global" || arg == "--system" {
            // Ignore --global/--system flags (we only support repo config)
            _global = true;
        } else if key.is_none() {
            key = Some(arg);
        } else if value.is_none() {
            value = Some(arg);
        }
        i += 1;
    }

    let Some(key) = key else {
        return Ok(ExecResult::err(
            "usage: git config [<options>] <name> [<value>]\n".to_string(),
            129,
        ));
    };

    // Get or set based on whether value is provided
    match value {
        Some(value) => {
            // Set config
            match git_client.config_set(&ctx.fs, ctx.cwd, key, value).await {
                Ok(()) => Ok(ExecResult::ok(String::new())),
                Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
            }
        }
        None => {
            // Get config
            match git_client.config_get(&ctx.fs, ctx.cwd, key).await {
                Ok(Some(value)) => Ok(ExecResult::ok(format!("{}\n", value))),
                Ok(None) => Ok(ExecResult::ok(String::new())),
                Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
            }
        }
    }
}

#[cfg(feature = "git")]
async fn git_add(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    if args.is_empty() {
        return Ok(ExecResult::err(
            "Nothing specified, nothing added.\n\
             hint: Maybe you wanted to say 'git add .'?\n"
                .to_string(),
            0,
        ));
    }

    let paths: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    match git_client.add(&ctx.fs, ctx.cwd, &paths).await {
        Ok(()) => Ok(ExecResult::ok(String::new())),
        Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
    }
}

#[cfg(feature = "git")]
async fn git_commit(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    // Parse -m <message>
    let mut message: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-m" {
            if i + 1 < args.len() {
                message = Some(args[i + 1].clone());
                i += 1;
            }
        } else if let Some(msg) = arg.strip_prefix("-m") {
            message = Some(msg.to_string());
        }
        i += 1;
    }

    let Some(message) = message else {
        return Ok(ExecResult::err(
            "error: switch 'm' requires a value\n".to_string(),
            128,
        ));
    };

    match git_client.commit(&ctx.fs, ctx.cwd, &message).await {
        Ok(output) => Ok(ExecResult::ok(output)),
        Err(e) => Ok(ExecResult::err(format!("{}\n", e), 1)),
    }
}

#[cfg(feature = "git")]
async fn git_status(ctx: Context<'_>, git_client: &crate::git::GitClient) -> Result<ExecResult> {
    match git_client.status(&ctx.fs, ctx.cwd).await {
        Ok(status) => {
            let output = git_client.format_status(&status);
            Ok(ExecResult::ok(output))
        }
        Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
    }
}

#[cfg(feature = "git")]
async fn git_log(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    // Parse -n <number> or -<number>
    let mut limit: Option<usize> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-n" {
            if i + 1 < args.len() {
                limit = args[i + 1].parse().ok();
                i += 1;
            }
        } else if let Some(n) = arg.strip_prefix("-n") {
            limit = n.parse().ok();
        } else if arg.starts_with('-') && arg[1..].parse::<usize>().is_ok() {
            limit = arg[1..].parse().ok();
        }
        i += 1;
    }

    match git_client.log(&ctx.fs, ctx.cwd, limit).await {
        Ok(entries) => {
            if entries.is_empty() {
                // No commits yet
                return Ok(ExecResult::err(
                    "fatal: your current branch 'master' does not have any commits yet\n"
                        .to_string(),
                    128,
                ));
            }
            let output = git_client.format_log(&entries);
            Ok(ExecResult::ok(output))
        }
        Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
    }
}

#[cfg(feature = "git")]
async fn git_remote(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    if args.is_empty() {
        // List remotes (names only)
        match git_client.remote_list(&ctx.fs, ctx.cwd).await {
            Ok(remotes) => {
                let output: String = remotes.iter().map(|r| format!("{}\n", r.name)).collect();
                Ok(ExecResult::ok(output))
            }
            Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
        }
    } else {
        let subcmd = &args[0];
        let subargs = &args[1..];

        match subcmd.as_str() {
            "-v" | "--verbose" => {
                // List remotes with URLs
                match git_client.remote_list(&ctx.fs, ctx.cwd).await {
                    Ok(remotes) => {
                        let output: String = remotes
                            .iter()
                            .flat_map(|r| {
                                vec![
                                    format!("{}\t{} (fetch)\n", r.name, r.url),
                                    format!("{}\t{} (push)\n", r.name, r.url),
                                ]
                            })
                            .collect();
                        Ok(ExecResult::ok(output))
                    }
                    Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
                }
            }
            "add" => {
                if subargs.len() < 2 {
                    return Ok(ExecResult::err(
                        "usage: git remote add <name> <url>\n".to_string(),
                        129,
                    ));
                }
                let name = &subargs[0];
                let url = &subargs[1];
                match git_client.remote_add(&ctx.fs, ctx.cwd, name, url).await {
                    Ok(()) => Ok(ExecResult::ok(String::new())),
                    Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
                }
            }
            "remove" | "rm" => {
                if subargs.is_empty() {
                    return Ok(ExecResult::err(
                        "usage: git remote remove <name>\n".to_string(),
                        129,
                    ));
                }
                let name = &subargs[0];
                match git_client.remote_remove(&ctx.fs, ctx.cwd, name).await {
                    Ok(()) => Ok(ExecResult::ok(String::new())),
                    Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
                }
            }
            _ => Ok(ExecResult::err(
                format!(
                    "error: Unknown subcommand: {}\n\
                     usage: git remote [-v | --verbose]\n\
                            git remote add <name> <url>\n\
                            git remote remove <name>\n",
                    subcmd
                ),
                1,
            )),
        }
    }
}

#[cfg(feature = "git")]
async fn git_clone(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    if args.is_empty() {
        return Ok(ExecResult::err(
            "usage: git clone <repository> [<directory>]\n".to_string(),
            129,
        ));
    }

    let url = &args[0];
    let dest = if args.len() > 1 {
        resolve_path(ctx.cwd, &args[1])
    } else {
        // Extract repo name from URL
        let name = url
            .rsplit('/')
            .next()
            .unwrap_or("repo")
            .trim_end_matches(".git");
        resolve_path(ctx.cwd, name)
    };

    match git_client.clone(&ctx.fs, url, &dest).await {
        Ok(output) => Ok(ExecResult::ok(output)),
        Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
    }
}

#[cfg(feature = "git")]
async fn git_fetch(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    let remote = if args.is_empty() { "origin" } else { &args[0] };

    match git_client.fetch(&ctx.fs, ctx.cwd, remote).await {
        Ok(output) => Ok(ExecResult::ok(output)),
        Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
    }
}

#[cfg(feature = "git")]
async fn git_push(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    let remote = if args.is_empty() { "origin" } else { &args[0] };

    match git_client.push(&ctx.fs, ctx.cwd, remote).await {
        Ok(output) => Ok(ExecResult::ok(output)),
        Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
    }
}

#[cfg(feature = "git")]
async fn git_pull(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    let remote = if args.is_empty() { "origin" } else { &args[0] };

    match git_client.pull(&ctx.fs, ctx.cwd, remote).await {
        Ok(output) => Ok(ExecResult::ok(output)),
        Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
    }
}

#[cfg(feature = "git")]
async fn git_branch(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    if args.is_empty() {
        // List branches
        match git_client.branch_list(&ctx.fs, ctx.cwd).await {
            Ok(branches) => {
                let output = git_client.format_branch_list(&branches);
                Ok(ExecResult::ok(output))
            }
            Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
        }
    } else if args[0] == "-d" || args[0] == "-D" {
        // Delete branch
        if args.len() < 2 {
            return Ok(ExecResult::err(
                "error: branch name required\n".to_string(),
                129,
            ));
        }
        match git_client.branch_delete(&ctx.fs, ctx.cwd, &args[1]).await {
            Ok(()) => Ok(ExecResult::ok(format!("Deleted branch {}.\n", args[1]))),
            Err(e) => Ok(ExecResult::err(format!("{}\n", e), 1)),
        }
    } else {
        // Create branch
        match git_client.branch_create(&ctx.fs, ctx.cwd, &args[0]).await {
            Ok(()) => Ok(ExecResult::ok(String::new())),
            Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
        }
    }
}

#[cfg(feature = "git")]
async fn git_checkout(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    if args.is_empty() {
        return Ok(ExecResult::err(
            "error: you must specify a branch or commit to checkout\n".to_string(),
            129,
        ));
    }

    // Handle -b flag for creating and checking out a new branch
    if args[0] == "-b" {
        if args.len() < 2 {
            return Ok(ExecResult::err(
                "error: switch 'b' requires a value\n".to_string(),
                129,
            ));
        }
        // Create branch first
        if let Err(e) = git_client.branch_create(&ctx.fs, ctx.cwd, &args[1]).await {
            return Ok(ExecResult::err(format!("{}\n", e), 128));
        }
        // Then checkout
        match git_client.checkout(&ctx.fs, ctx.cwd, &args[1]).await {
            Ok(output) => Ok(ExecResult::ok(output)),
            Err(e) => Ok(ExecResult::err(format!("{}\n", e), 1)),
        }
    } else {
        match git_client.checkout(&ctx.fs, ctx.cwd, &args[0]).await {
            Ok(output) => Ok(ExecResult::ok(output)),
            Err(e) => Ok(ExecResult::err(format!("{}\n", e), 1)),
        }
    }
}

#[cfg(feature = "git")]
async fn git_diff(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    let from = args.first().map(|s| s.as_str());
    let to = args.get(1).map(|s| s.as_str());

    match git_client.diff(&ctx.fs, ctx.cwd, from, to).await {
        Ok(output) => Ok(ExecResult::ok(output)),
        Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
    }
}

#[cfg(feature = "git")]
async fn git_reset(
    ctx: Context<'_>,
    git_client: &crate::git::GitClient,
    args: &[String],
) -> Result<ExecResult> {
    // Parse mode and target
    let mut mode = "--mixed"; // default
    let mut target: Option<&str> = None;

    for arg in args {
        if arg.starts_with("--") {
            mode = arg.as_str();
        } else {
            target = Some(arg.as_str());
        }
    }

    match git_client.reset(&ctx.fs, ctx.cwd, mode, target).await {
        Ok(output) => Ok(ExecResult::ok(output)),
        Err(e) => Ok(ExecResult::err(format!("{}\n", e), 128)),
    }
}
