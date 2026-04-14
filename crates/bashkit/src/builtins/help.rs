//! help builtin - shell-wide command discovery and usage info
//!
//! Non-standard enhanced help that lists all available builtins,
//! provides usage information, and supports search/filtering.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// help builtin - display information about builtin commands.
///
/// Usage: help [OPTIONS] [COMMAND]
///
/// Options:
///   -s         Short description only
///   --list     List all available builtins
///   --search TERM  Search builtins by name
///   --json     Output in JSON format
///
/// Without arguments, lists all builtin categories.
/// With a command name, shows usage for that command.
pub struct Help;

/// Builtin command metadata
struct CmdInfo {
    name: &'static str,
    category: &'static str,
    usage: &'static str,
    description: &'static str,
}

const BUILTINS: &[CmdInfo] = &[
    // Core shell
    CmdInfo {
        name: "echo",
        category: "output",
        usage: "echo [-neE] [STRING...]",
        description: "Display text",
    },
    CmdInfo {
        name: "printf",
        category: "output",
        usage: "printf FORMAT [ARGS...]",
        description: "Formatted output",
    },
    CmdInfo {
        name: "true",
        category: "flow",
        usage: "true",
        description: "Exit with status 0",
    },
    CmdInfo {
        name: "false",
        category: "flow",
        usage: "false",
        description: "Exit with status 1",
    },
    CmdInfo {
        name: "exit",
        category: "flow",
        usage: "exit [N]",
        description: "Exit shell",
    },
    CmdInfo {
        name: "return",
        category: "flow",
        usage: "return [N]",
        description: "Return from function",
    },
    CmdInfo {
        name: "break",
        category: "flow",
        usage: "break [N]",
        description: "Break from loop",
    },
    CmdInfo {
        name: "continue",
        category: "flow",
        usage: "continue [N]",
        description: "Continue loop",
    },
    CmdInfo {
        name: "cd",
        category: "navigation",
        usage: "cd [DIR]",
        description: "Change directory",
    },
    CmdInfo {
        name: "pwd",
        category: "navigation",
        usage: "pwd [-LP]",
        description: "Print working directory",
    },
    CmdInfo {
        name: "export",
        category: "variables",
        usage: "export NAME[=VALUE]",
        description: "Export variable",
    },
    CmdInfo {
        name: "local",
        category: "variables",
        usage: "local NAME[=VALUE]",
        description: "Local variable",
    },
    CmdInfo {
        name: "set",
        category: "variables",
        usage: "set [-euo pipefail]",
        description: "Set shell options",
    },
    CmdInfo {
        name: "unset",
        category: "variables",
        usage: "unset NAME",
        description: "Unset variable",
    },
    CmdInfo {
        name: "shift",
        category: "variables",
        usage: "shift [N]",
        description: "Shift positional params",
    },
    CmdInfo {
        name: "source",
        category: "execution",
        usage: "source FILE [ARGS]",
        description: "Execute file in current shell",
    },
    CmdInfo {
        name: "eval",
        category: "execution",
        usage: "eval [ARGS]",
        description: "Evaluate arguments as command",
    },
    CmdInfo {
        name: "test",
        category: "conditionals",
        usage: "test EXPR",
        description: "Evaluate expression",
    },
    CmdInfo {
        name: "[",
        category: "conditionals",
        usage: "[ EXPR ]",
        description: "Evaluate expression",
    },
    CmdInfo {
        name: "read",
        category: "input",
        usage: "read [-r] [-p PROMPT] VAR...",
        description: "Read input",
    },
    // File operations
    CmdInfo {
        name: "cat",
        category: "text",
        usage: "cat [-nvET] [FILE...]",
        description: "Concatenate files",
    },
    CmdInfo {
        name: "head",
        category: "text",
        usage: "head [-n N] [FILE]",
        description: "First N lines",
    },
    CmdInfo {
        name: "tail",
        category: "text",
        usage: "tail [-n N] [FILE]",
        description: "Last N lines",
    },
    CmdInfo {
        name: "grep",
        category: "text",
        usage: "grep [-ivncowElFPq] PATTERN [FILE...]",
        description: "Search patterns",
    },
    CmdInfo {
        name: "sed",
        category: "text",
        usage: "sed [-inE] SCRIPT [FILE]",
        description: "Stream editor",
    },
    CmdInfo {
        name: "awk",
        category: "text",
        usage: "awk [-F SEP] PROGRAM [FILE]",
        description: "Text processing",
    },
    CmdInfo {
        name: "sort",
        category: "text",
        usage: "sort [-rnu] [FILE]",
        description: "Sort lines",
    },
    CmdInfo {
        name: "uniq",
        category: "text",
        usage: "uniq [-cdu] [FILE]",
        description: "Filter duplicates",
    },
    CmdInfo {
        name: "cut",
        category: "text",
        usage: "cut -d DELIM -f FIELDS [FILE]",
        description: "Extract fields",
    },
    CmdInfo {
        name: "tr",
        category: "text",
        usage: "tr [-d] SET1 [SET2]",
        description: "Translate characters",
    },
    CmdInfo {
        name: "wc",
        category: "text",
        usage: "wc [-lwc] [FILE...]",
        description: "Count lines/words/bytes",
    },
    CmdInfo {
        name: "jq",
        category: "text",
        usage: "jq [-rcsne] FILTER [FILE]",
        description: "JSON processing",
    },
    CmdInfo {
        name: "diff",
        category: "text",
        usage: "diff [-uq] FILE1 FILE2",
        description: "Compare files",
    },
    // File operations
    CmdInfo {
        name: "mkdir",
        category: "files",
        usage: "mkdir [-p] DIR...",
        description: "Create directories",
    },
    CmdInfo {
        name: "rm",
        category: "files",
        usage: "rm [-rf] FILE...",
        description: "Remove files",
    },
    CmdInfo {
        name: "cp",
        category: "files",
        usage: "cp [-r] SRC DEST",
        description: "Copy files",
    },
    CmdInfo {
        name: "mv",
        category: "files",
        usage: "mv SRC DEST",
        description: "Move/rename files",
    },
    CmdInfo {
        name: "touch",
        category: "files",
        usage: "touch [-t STAMP] FILE...",
        description: "Create/update files",
    },
    CmdInfo {
        name: "chmod",
        category: "files",
        usage: "chmod MODE FILE...",
        description: "Change permissions",
    },
    CmdInfo {
        name: "ln",
        category: "files",
        usage: "ln [-sf] TARGET LINK",
        description: "Create links",
    },
    CmdInfo {
        name: "ls",
        category: "files",
        usage: "ls [-lahR1t] [DIR]",
        description: "List directory",
    },
    CmdInfo {
        name: "find",
        category: "files",
        usage: "find [PATH] [-name PAT] [-type TYPE]",
        description: "Search files",
    },
    CmdInfo {
        name: "tree",
        category: "files",
        usage: "tree [-adL N] [DIR]",
        description: "Directory tree",
    },
    CmdInfo {
        name: "stat",
        category: "files",
        usage: "stat [-c FMT] FILE",
        description: "File metadata",
    },
    // Utilities
    CmdInfo {
        name: "date",
        category: "utility",
        usage: "date [-u] [+FORMAT]",
        description: "Date/time",
    },
    CmdInfo {
        name: "sleep",
        category: "utility",
        usage: "sleep SECONDS",
        description: "Pause execution",
    },
    CmdInfo {
        name: "basename",
        category: "utility",
        usage: "basename PATH [SUFFIX]",
        description: "Strip directory",
    },
    CmdInfo {
        name: "dirname",
        category: "utility",
        usage: "dirname PATH",
        description: "Strip filename",
    },
    CmdInfo {
        name: "seq",
        category: "utility",
        usage: "seq [FIRST [INCR]] LAST",
        description: "Print sequence",
    },
    CmdInfo {
        name: "expr",
        category: "utility",
        usage: "expr ARG...",
        description: "Evaluate expression",
    },
    CmdInfo {
        name: "bc",
        category: "utility",
        usage: "bc [-l]",
        description: "Calculator",
    },
    CmdInfo {
        name: "base64",
        category: "utility",
        usage: "base64 [-d] [FILE]",
        description: "Base64 encode/decode",
    },
    // Non-standard
    CmdInfo {
        name: "assert",
        category: "non-standard",
        usage: "assert EXPR [MESSAGE]",
        description: "Test assertions",
    },
    CmdInfo {
        name: "retry",
        category: "non-standard",
        usage: "retry [OPTS] -- CMD",
        description: "Retry commands",
    },
    CmdInfo {
        name: "log",
        category: "non-standard",
        usage: "log LEVEL MSG [K=V...]",
        description: "Structured logging",
    },
    CmdInfo {
        name: "semver",
        category: "non-standard",
        usage: "semver SUBCMD ARGS...",
        description: "Version operations",
    },
    CmdInfo {
        name: "dotenv",
        category: "non-standard",
        usage: "dotenv [OPTS] [FILE]",
        description: "Load .env files",
    },
    CmdInfo {
        name: "verify",
        category: "non-standard",
        usage: "verify [OPTS] FILE [HASH]",
        description: "File verification",
    },
    CmdInfo {
        name: "glob",
        category: "non-standard",
        usage: "glob [OPTS] PATTERN [STR...]",
        description: "Glob matching",
    },
];

#[async_trait]
impl Builtin for Help {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut short = false;
        let mut list = false;
        let mut json = false;
        let mut search: Option<String> = None;
        let mut command: Option<String> = None;

        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-s" => short = true,
                "--list" => list = true,
                "--json" => json = true,
                "--search" => {
                    i += 1;
                    search = ctx.args.get(i).cloned();
                }
                arg if !arg.starts_with('-') => command = Some(arg.to_string()),
                other => {
                    return Ok(ExecResult::err(
                        format!("help: unknown option '{other}'\n"),
                        1,
                    ));
                }
            }
            i += 1;
        }

        // Specific command help
        if let Some(ref cmd) = command {
            if let Some(info) = BUILTINS.iter().find(|b| b.name == cmd.as_str()) {
                if json {
                    return Ok(ExecResult::ok(format!(
                        "{{\"name\":\"{}\",\"category\":\"{}\",\"usage\":\"{}\",\"description\":\"{}\"}}\n",
                        info.name, info.category, info.usage, info.description
                    )));
                }
                if short {
                    return Ok(ExecResult::ok(format!(
                        "{}: {}\n",
                        info.name, info.description
                    )));
                }
                return Ok(ExecResult::ok(format!(
                    "{}: {}\nUsage: {}\nCategory: {}\n",
                    info.name, info.description, info.usage, info.category
                )));
            }
            return Ok(ExecResult::err(format!("help: no help for '{cmd}'\n"), 1));
        }

        // Search mode
        if let Some(ref term) = search {
            let term_lower = term.to_lowercase();
            let matches: Vec<&CmdInfo> = BUILTINS
                .iter()
                .filter(|b| {
                    b.name.contains(&term_lower)
                        || b.description.to_lowercase().contains(&term_lower)
                        || b.category.contains(&term_lower)
                })
                .collect();

            if matches.is_empty() {
                return Ok(ExecResult::ok(format!(
                    "help: no commands matching '{term}'\n"
                )));
            }

            let mut output = String::new();
            for info in matches {
                output.push_str(&format!("  {:12} {}\n", info.name, info.description));
            }
            return Ok(ExecResult::ok(output));
        }

        // List mode or default: show categories
        if list {
            let mut output = String::new();
            for info in BUILTINS {
                if short {
                    output.push_str(&format!("{}\n", info.name));
                } else {
                    output.push_str(&format!("  {:12} {}\n", info.name, info.description));
                }
            }
            return Ok(ExecResult::ok(output));
        }

        // Default: show categories with counts
        let mut categories: Vec<(&str, usize)> = Vec::new();
        for info in BUILTINS {
            if let Some(entry) = categories.iter_mut().find(|(c, _)| *c == info.category) {
                entry.1 += 1;
            } else {
                categories.push((info.category, 1));
            }
        }

        let mut output = String::from("Bashkit builtin commands:\n\n");
        for (cat, count) in &categories {
            output.push_str(&format!("  {:16} ({count} commands)\n", cat));
        }
        output.push_str(&format!("\nTotal: {} builtins\n", BUILTINS.len()));
        output.push_str("Use 'help <command>' for details, 'help --list' to list all.\n");

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_help(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Help.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_default_categories() {
        let result = run_help(&[]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Bashkit builtin commands"));
        assert!(result.stdout.contains("Total:"));
    }

    #[tokio::test]
    async fn test_list_all() {
        let result = run_help(&["--list"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("echo"));
        assert!(result.stdout.contains("grep"));
        assert!(result.stdout.contains("mkdir"));
    }

    #[tokio::test]
    async fn test_list_short() {
        let result = run_help(&["--list", "-s"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("echo\n"));
    }

    #[tokio::test]
    async fn test_specific_command() {
        let result = run_help(&["echo"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("echo"));
        assert!(result.stdout.contains("Usage:"));
        assert!(result.stdout.contains("Category:"));
    }

    #[tokio::test]
    async fn test_command_short() {
        let result = run_help(&["-s", "grep"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("grep:"));
    }

    #[tokio::test]
    async fn test_command_json() {
        let result = run_help(&["--json", "cat"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("\"name\":\"cat\""));
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let result = run_help(&["nonexistent"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("no help"));
    }

    #[tokio::test]
    async fn test_search() {
        let result = run_help(&["--search", "text"]).await;
        assert_eq!(result.exit_code, 0);
        // Should find text-processing commands
        assert!(result.stdout.contains("grep") || result.stdout.contains("sed"));
    }

    #[tokio::test]
    async fn test_search_no_results() {
        let result = run_help(&["--search", "xyznonexistent"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("no commands matching"));
    }

    #[tokio::test]
    async fn test_invalid_option() {
        let result = run_help(&["--foo"]).await;
        assert_eq!(result.exit_code, 1);
    }
}
