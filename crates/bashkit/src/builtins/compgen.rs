//! compgen builtin - programmable completion generator
//!
//! Generates possible completions for a word, used by Bash's
//! programmable completion system.
//!
//! Usage:
//!   compgen -W "start stop restart" -- st    # words matching prefix
//!   compgen -f                                # filenames
//!   compgen -d                                # directories
//!   compgen -v                                # variables
//!   compgen -c                                # commands (builtins)
//!   compgen -A function                       # by action type

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// compgen builtin - bash completion generator.
pub struct Compgen;

/// Hardcoded list of known builtin command names.
const BUILTIN_COMMANDS: &[&str] = &[
    "alias", "assert", "awk", "base64", "basename", "bc", "break", "cat", "cd", "chmod", "chown",
    "clear", "column", "comm", "compgen", "continue", "cp", "curl", "cut", "date", "declare", "df",
    "diff", "dirname", "dirs", "dotenv", "du", "echo", "env", "envsubst", "eval", "exit", "expand",
    "export", "expr", "false", "find", "fold", "grep", "gunzip", "gzip", "head", "hexdump",
    "history", "hostname", "iconv", "id", "jq", "json", "join", "kill", "ln", "local", "log", "ls",
    "mkdir", "mktemp", "mv", "nl", "od", "paste", "popd", "printenv", "printf", "pushd", "pwd",
    "read", "readlink", "readonly", "realpath", "retry", "return", "rev", "rm", "rmdir", "sed",
    "semver", "seq", "set", "shift", "shopt", "sleep", "sort", "source", "split", "stat",
    "strings", "tac", "tail", "tar", "tee", "test", "timeout", "touch", "tr", "tree", "true",
    "uname", "unexpand", "uniq", "unset", "wait", "watch", "wc", "wget", "whoami", "xargs", "xxd",
    "yes",
];

#[async_trait]
impl Builtin for Compgen {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut wordlist: Option<String> = None;
        let mut gen_files = false;
        let mut gen_dirs = false;
        let mut gen_commands = false;
        let mut gen_variables = false;
        let mut actions: Vec<String> = Vec::new();
        let mut prefix: Option<String> = None;

        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-W" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "compgen: -W: option requires an argument\n".to_string(),
                            1,
                        ));
                    }
                    wordlist = Some(ctx.args[i].clone());
                }
                "-f" => gen_files = true,
                "-d" => gen_dirs = true,
                "-c" => gen_commands = true,
                "-v" => gen_variables = true,
                "-A" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "compgen: -A: option requires an argument\n".to_string(),
                            1,
                        ));
                    }
                    actions.push(ctx.args[i].clone());
                }
                "--" => {
                    // Next arg is the prefix word
                    i += 1;
                    if i < ctx.args.len() {
                        prefix = Some(ctx.args[i].clone());
                    }
                }
                arg if arg.starts_with('-') => {
                    return Ok(ExecResult::err(
                        format!("compgen: unknown option '{arg}'\n"),
                        1,
                    ));
                }
                _ => {
                    // Positional arg is the prefix word
                    if prefix.is_none() {
                        prefix = Some(ctx.args[i].clone());
                    }
                }
            }
            i += 1;
        }

        // Process -A actions into flags
        for action in &actions {
            match action.as_str() {
                "file" => gen_files = true,
                "directory" => gen_dirs = true,
                "command" | "builtin" => gen_commands = true,
                "variable" => gen_variables = true,
                "function" | "alias" => {
                    // No functions/aliases in virtual env, produce empty
                }
                _ => {
                    return Ok(ExecResult::err(
                        format!("compgen: unknown action '{action}'\n"),
                        1,
                    ));
                }
            }
        }

        let pfx = prefix.as_deref().unwrap_or("");
        let mut completions: Vec<String> = Vec::new();

        // -W wordlist
        if let Some(ref wl) = wordlist {
            for word in wl.split_whitespace() {
                if word.starts_with(pfx) {
                    completions.push(word.to_string());
                }
            }
        }

        // -f: filenames from cwd
        if gen_files && let Ok(entries) = ctx.fs.read_dir(ctx.cwd).await {
            for entry in entries {
                if entry.name.starts_with(pfx) {
                    completions.push(entry.name);
                }
            }
        }

        // -d: directories from cwd
        if gen_dirs && let Ok(entries) = ctx.fs.read_dir(ctx.cwd).await {
            for entry in entries {
                if entry.metadata.file_type.is_dir() && entry.name.starts_with(pfx) {
                    completions.push(entry.name);
                }
            }
        }

        // -c: commands (builtins + functions + aliases + PATH executables)
        if gen_commands {
            // Builtins
            for &cmd in BUILTIN_COMMANDS {
                if cmd.starts_with(pfx) {
                    completions.push(cmd.to_string());
                }
            }
            // Functions from shell context
            if let Some(ref shell) = ctx.shell {
                for name in shell.functions.keys() {
                    if name.starts_with(pfx) {
                        completions.push(name.clone());
                    }
                }
                for name in shell.aliases.keys() {
                    if name.starts_with(pfx) {
                        completions.push(name.clone());
                    }
                }
            }
            // PATH executables from VFS
            let path_var = ctx
                .variables
                .get("PATH")
                .or_else(|| ctx.env.get("PATH"))
                .cloned()
                .unwrap_or_default();
            for dir in path_var.split(':') {
                if dir.is_empty() {
                    continue;
                }
                let dir_path = std::path::PathBuf::from(dir);
                if let Ok(entries) = ctx.fs.read_dir(&dir_path).await {
                    for entry in entries {
                        if entry.name.starts_with(pfx)
                            && entry.metadata.file_type.is_file()
                            && (entry.metadata.mode & 0o111) != 0
                        {
                            completions.push(entry.name);
                        }
                    }
                }
            }
        }

        // -v: variable names
        if gen_variables {
            for name in ctx.variables.keys() {
                if name.starts_with(pfx) {
                    completions.push(name.clone());
                }
            }
        }

        // No generators specified and no wordlist - error
        if wordlist.is_none()
            && !gen_files
            && !gen_dirs
            && !gen_commands
            && !gen_variables
            && actions.is_empty()
        {
            return Ok(ExecResult::err(
                "compgen: usage: compgen [-W wordlist] [-f] [-d] [-c] [-v] [-A action] [word]\n"
                    .to_string(),
                1,
            ));
        }

        completions.sort();
        completions.dedup();

        if completions.is_empty() {
            return Ok(ExecResult::with_code("", 1));
        }

        let mut out = String::new();
        for c in &completions {
            out.push_str(c);
            out.push('\n');
        }
        Ok(ExecResult::ok(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run(
        args: &[&str],
        variables: Option<HashMap<String, String>>,
        fs: Option<Arc<InMemoryFs>>,
    ) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut vars = variables.unwrap_or_default();
        let mut cwd = PathBuf::from("/");
        let fs = fs.unwrap_or_else(|| Arc::new(InMemoryFs::new()));
        let fs_dyn = fs as Arc<dyn crate::fs::FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut vars,
            cwd: &mut cwd,
            fs: fs_dyn,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            shell: None,
        };
        Compgen.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_wordlist_basic() {
        let r = run(&["-W", "start stop restart"], None, None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("start\n"));
        assert!(r.stdout.contains("stop\n"));
        assert!(r.stdout.contains("restart\n"));
    }

    #[tokio::test]
    async fn test_wordlist_with_prefix() {
        let r = run(&["-W", "start stop restart", "--", "st"], None, None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("start\n"));
        assert!(r.stdout.contains("stop\n"));
        assert!(!r.stdout.contains("restart"));
    }

    #[tokio::test]
    async fn test_wordlist_no_match() {
        let r = run(&["-W", "start stop restart", "--", "xyz"], None, None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_commands() {
        let r = run(&["-c", "--", "ec"], None, None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("echo\n"));
    }

    #[tokio::test]
    async fn test_variables() {
        let mut vars = HashMap::new();
        vars.insert("HOME".to_string(), "/home/user".to_string());
        vars.insert("HOSTNAME".to_string(), "localhost".to_string());
        vars.insert("PATH".to_string(), "/bin".to_string());

        let r = run(&["-v", "--", "HO"], Some(vars), None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("HOME\n"));
        assert!(r.stdout.contains("HOSTNAME\n"));
        assert!(!r.stdout.contains("PATH"));
    }

    #[tokio::test]
    async fn test_files() {
        let fs = Arc::new(InMemoryFs::new());
        let fs_dyn = fs.clone() as Arc<dyn crate::fs::FileSystem>;
        fs_dyn
            .write_file(std::path::Path::new("/hello.txt"), b"hi")
            .await
            .unwrap();
        fs_dyn
            .write_file(std::path::Path::new("/help.md"), b"x")
            .await
            .unwrap();
        fs_dyn
            .write_file(std::path::Path::new("/other.txt"), b"o")
            .await
            .unwrap();

        let r = run(&["-f", "--", "hel"], None, Some(fs)).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("hello.txt\n"));
        assert!(r.stdout.contains("help.md\n"));
        assert!(!r.stdout.contains("other"));
    }

    #[tokio::test]
    async fn test_directories() {
        let fs = Arc::new(InMemoryFs::new());
        let fs_dyn = fs.clone() as Arc<dyn crate::fs::FileSystem>;
        fs_dyn
            .mkdir(std::path::Path::new("/docs"), false)
            .await
            .unwrap();
        fs_dyn
            .write_file(std::path::Path::new("/data.txt"), b"x")
            .await
            .unwrap();

        let r = run(&["-d", "--", "d"], None, Some(fs)).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("docs\n"));
        assert!(!r.stdout.contains("data.txt"));
    }

    #[tokio::test]
    async fn test_action_flag() {
        let r = run(&["-A", "command", "--", "ec"], None, None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("echo\n"));
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let r = run(&["-A", "nosuch"], None, None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("unknown action"));
    }

    #[tokio::test]
    async fn test_no_options() {
        let r = run(&[], None, None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("usage"));
    }

    #[tokio::test]
    async fn test_w_missing_arg() {
        let r = run(&["-W"], None, None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("requires an argument"));
    }
}
