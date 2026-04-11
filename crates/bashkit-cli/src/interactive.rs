// Interactive shell mode using rustyline for line editing.
// Decision: rustyline over reedline — lighter deps, no SQLite/crossterm.
// Decision: feature-gated behind "interactive" — no deps in library mode.
// Decision: Ctrl-C during execution via signal-hook + cancellation_token.
// Decision: tab completion from builtins + VFS paths + functions + variables.
// Decision: PS1 prompt with bash-compatible escapes (\u, \h, \w, \$).
// See specs/018-interactive-shell.md

use anyhow::Result;
use rustyline::completion::Completer;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hint, Hinter};
use rustyline::validate::Validator;
use rustyline::{Config, Context, Editor, Helper};
use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const DEFAULT_PS1: &str = "\\u@bashkit:\\w\\$ ";
const DEFAULT_PS2: &str = "> ";
const RC_FILE: &str = "/home/user/.bashkitrc";
const MAX_HISTORY: usize = 1000;

// Same list as compgen.rs — keep in sync.
const BUILTIN_COMMANDS: &[&str] = &[
    "alias", "assert", "awk", "base64", "basename", "bc", "break", "cat", "cd", "chmod", "chown",
    "clear", "column", "comm", "compgen", "continue", "cp", "curl", "cut", "date", "declare", "df",
    "diff", "dirname", "dirs", "dotenv", "du", "echo", "env", "envsubst", "eval", "exit", "expand",
    "export", "expr", "false", "fc", "find", "fold", "grep", "gunzip", "gzip", "head", "help",
    "hexdump", "history", "hostname", "iconv", "id", "jq", "json", "join", "kill", "ln", "local",
    "log", "ls", "mkdir", "mktemp", "mv", "nl", "od", "paste", "popd", "printenv", "printf",
    "pushd", "pwd", "read", "readlink", "readonly", "realpath", "retry", "return", "rev", "rg",
    "rm", "rmdir", "sed", "semver", "seq", "set", "shift", "shopt", "sleep", "sort", "source",
    "split", "stat", "strings", "tac", "tail", "tar", "tee", "test", "timeout", "touch", "tr",
    "tree", "true", "type", "uname", "unexpand", "uniq", "unset", "wait", "watch", "wc", "wget",
    "whoami", "xargs", "xxd", "yes",
];

// --- Incomplete input detection ---

fn is_incomplete_input(err_msg: &str) -> bool {
    let lower = err_msg.to_lowercase();
    lower.contains("unterminated")
        || lower.contains("unexpected end of input")
        || lower.contains("unexpected eof")
        || lower.contains("syntax error: empty")
        || lower.contains("expected 'fi'")
        || lower.contains("expected 'done'")
        || lower.contains("expected 'esac'")
        || lower.contains("expected '}' to close brace group")
}

fn error_result(exit_code: i32) -> bashkit::ExecResult {
    bashkit::ExecResult {
        exit_code,
        ..Default::default()
    }
}

// --- PS1 prompt expansion ---

fn expand_ps1(ps1: &str, state: &bashkit::ShellState) -> String {
    let mut out = String::with_capacity(ps1.len());
    let mut chars = ps1.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('u') => {
                    out.push_str(state.env.get("USER").map(|s| s.as_str()).unwrap_or("user"));
                }
                Some('h') => {
                    let host = state
                        .env
                        .get("HOSTNAME")
                        .map(|s| s.as_str())
                        .unwrap_or("bashkit");
                    // \h = short hostname (up to first '.')
                    if let Some(dot) = host.find('.') {
                        out.push_str(&host[..dot]);
                    } else {
                        out.push_str(host);
                    }
                }
                Some('H') => {
                    out.push_str(
                        state
                            .env
                            .get("HOSTNAME")
                            .map(|s| s.as_str())
                            .unwrap_or("bashkit"),
                    );
                }
                Some('w') => {
                    let cwd = state.cwd.display().to_string();
                    let home = state.env.get("HOME").map(|s| s.as_str()).unwrap_or("");
                    if !home.is_empty() && cwd.starts_with(home) {
                        out.push('~');
                        out.push_str(&cwd[home.len()..]);
                    } else {
                        out.push_str(&cwd);
                    }
                }
                Some('W') => {
                    let cwd = state.cwd.display().to_string();
                    if cwd == "/" {
                        out.push('/');
                    } else {
                        out.push_str(
                            Path::new(&cwd)
                                .file_name()
                                .map(|s| s.to_string_lossy())
                                .unwrap_or(Cow::Borrowed("/"))
                                .as_ref(),
                        );
                    }
                }
                Some('$') => {
                    let uid = state
                        .env
                        .get("EUID")
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(1000);
                    out.push(if uid == 0 { '#' } else { '$' });
                }
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('a') => out.push('\x07'),
                Some('e') => out.push('\x1b'),
                Some('[') => {
                    // \[ ... \] — non-printing sequence (for ANSI codes)
                    // Pass through to terminal but don't count for width
                    out.push('\x01'); // RL_PROMPT_START_IGNORE
                }
                Some(']') => {
                    out.push('\x02'); // RL_PROMPT_END_IGNORE
                }
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

// --- Tab completion ---

struct BashkitHelper {
    fs: Arc<dyn bashkit::FileSystem>,
    state_fn: Box<dyn Fn() -> bashkit::ShellState + Send + Sync>,
}

impl BashkitHelper {
    fn complete_path(&self, partial: &str) -> Vec<String> {
        let state = (self.state_fn)();
        let (dir_path, prefix) = if let Some(slash) = partial.rfind('/') {
            let dir = &partial[..=slash];
            let pfx = &partial[slash + 1..];
            // Resolve relative paths against cwd
            let resolved = if dir.starts_with('/') {
                dir.to_string()
            } else {
                format!("{}/{}", state.cwd.display(), dir)
            };
            (resolved, pfx.to_string())
        } else {
            (state.cwd.display().to_string(), partial.to_string())
        };

        let dir = std::path::PathBuf::from(&dir_path);
        // We're inside a single-thread tokio runtime (rustyline calls Completer
        // synchronously).  handle.block_on() would panic with "Cannot start a
        // runtime from within a runtime", so spawn a helper thread with its own
        // runtime to drive the async VFS call.
        let entries = match tokio::runtime::Handle::try_current() {
            Ok(_) => {
                let fs = Arc::clone(&self.fs);
                std::thread::spawn(move || {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .ok()
                        .and_then(|rt| rt.block_on(fs.read_dir(&dir)).ok())
                        .unwrap_or_default()
                })
                .join()
                .unwrap_or_default()
            }
            Err(_) => return Vec::new(),
        };

        let mut results = Vec::new();
        for entry in &entries {
            if entry.name.starts_with(&prefix) {
                let mut candidate = if partial.contains('/') {
                    let base = &partial[..=partial.rfind('/').unwrap()];
                    format!("{}{}", base, entry.name)
                } else {
                    entry.name.clone()
                };
                if entry.metadata.file_type.is_dir() {
                    candidate.push('/');
                }
                results.push(candidate);
            }
        }
        results.sort();
        results
    }
}

impl Completer for BashkitHelper {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<String>)> {
        let line_to_cursor = &line[..pos];
        // Find the word being completed (go back to last space/;/|/&/`)
        let word_start = line_to_cursor
            .rfind(|c: char| c.is_whitespace() || matches!(c, ';' | '|' | '&' | '`' | '(' | '$'))
            .map(|i| i + 1)
            .unwrap_or(0);
        let partial = &line_to_cursor[word_start..];

        if partial.is_empty() {
            return Ok((pos, Vec::new()));
        }

        let is_command_position = {
            let before = line_to_cursor[..word_start].trim_end();
            before.is_empty()
                || before.ends_with(';')
                || before.ends_with('|')
                || before.ends_with("&&")
                || before.ends_with("||")
                || before.ends_with('`')
                || before.ends_with('(')
        };

        let mut candidates: Vec<String> = Vec::new();

        // Variable completion: $VAR
        if let Some(var_prefix) = partial.strip_prefix('$') {
            let state = (self.state_fn)();
            for key in state.env.keys().chain(state.variables.keys()) {
                if key.starts_with(var_prefix) {
                    candidates.push(format!("${key}"));
                }
            }
            candidates.sort();
            candidates.dedup();
            return Ok((word_start, candidates));
        }

        // Path completion if contains / or is an argument position
        if partial.contains('/') || partial.starts_with('.') || partial.starts_with('~') {
            candidates.extend(self.complete_path(partial));
        }

        if is_command_position {
            // Complete builtins
            for &cmd in BUILTIN_COMMANDS {
                if cmd.starts_with(partial) {
                    candidates.push(cmd.to_string());
                }
            }
            // Complete functions and aliases
            let state = (self.state_fn)();
            // Functions are visible via compgen but we don't have direct access
            // to the function names from ShellState. We do have aliases.
            for name in state.aliases.keys() {
                if name.starts_with(partial) {
                    candidates.push(name.clone());
                }
            }
        }

        if !is_command_position && !partial.contains('/') && !partial.starts_with('.') {
            // File/dir completion for arguments
            candidates.extend(self.complete_path(partial));
        }

        candidates.sort();
        candidates.dedup();
        Ok((word_start, candidates))
    }
}

// --- History hints ---

impl Hinter for BashkitHelper {
    type Hint = ShellHint;

    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<ShellHint> {
        if line.is_empty() || pos < line.len() {
            return None;
        }
        // Search history for most recent match
        let history = ctx.history();
        for i in (0..history.len()).rev() {
            if let Ok(Some(entry)) = history.get(i, rustyline::history::SearchDirection::Reverse) {
                let text = entry.entry.as_ref();
                if text.starts_with(line) && text.len() > line.len() {
                    return Some(ShellHint {
                        suffix: text[line.len()..].to_string(),
                    });
                }
            }
        }
        None
    }
}

struct ShellHint {
    suffix: String,
}

impl Hint for ShellHint {
    fn display(&self) -> &str {
        &self.suffix
    }

    fn completion(&self) -> Option<&str> {
        if self.suffix.is_empty() {
            None
        } else {
            Some(&self.suffix)
        }
    }
}

// --- Syntax highlighting ---

impl Highlighter for BashkitHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        // Dim gray for hints
        Cow::Owned(format!("\x1b[38;5;240m{hint}\x1b[0m"))
    }

    fn highlight_char(
        &self,
        _line: &str,
        _pos: usize,
        _kind: rustyline::highlight::CmdKind,
    ) -> bool {
        // Always re-highlight — enables hint coloring
        true
    }
}

// --- Multiline validator ---

// Validator is a no-op — multiline is handled in the REPL loop via
// is_incomplete_input() after exec fails with a parse error. We can't
// use Validator for this because it runs synchronously and the parser
// requires async (tokio block_on deadlocks on a single-thread runtime).
impl Validator for BashkitHelper {}

impl Helper for BashkitHelper {}

// --- Terminal size ---

fn terminal_columns() -> u16 {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0)
        .unwrap_or(80)
}

// --- Main REPL ---

async fn set_interactive_env(bash: &mut bashkit::Bash) {
    let cols = terminal_columns();
    let rows = terminal_size::terminal_size()
        .map(|(_, h)| h.0)
        .unwrap_or(24);
    let env_script = format!(
        "export COLUMNS={cols} LINES={rows} SHLVL=${{SHLVL:-0}}; export SHLVL=$((SHLVL + 1))"
    );
    let _ = bash.exec(&env_script).await;
}

async fn source_rc_file(bash: &mut bashkit::Bash) {
    // Check if ~/.bashkitrc exists in the VFS
    let fs = bash.fs();
    let rc_path = std::path::PathBuf::from(RC_FILE);
    if let Ok(true) = fs.exists(&rc_path).await
        && let Ok(content) = fs.read_file(&rc_path).await
        && let Ok(script) = String::from_utf8(content)
    {
        let _ = bash.exec(&script).await;
    }
}

#[cfg(test)]
fn test_bash() -> bashkit::Bash {
    bashkit::Bash::builder()
        .tty(0, true)
        .tty(1, true)
        .tty(2, true)
        .limits(bashkit::ExecutionLimits::cli())
        .session_limits(bashkit::SessionLimits::unlimited())
        .build()
}

pub async fn run(mut bash: bashkit::Bash) -> Result<i32> {
    // Set up interactive environment
    set_interactive_env(&mut bash).await;

    // Source startup file
    source_rc_file(&mut bash).await;

    // Set up Ctrl-C handler for command interruption
    let cancel_token = bash.cancellation_token();
    let sigint_flag = Arc::new(AtomicBool::new(false));
    let _ = signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&sigint_flag));

    // Build editor with custom helper
    let config = Config::builder()
        .auto_add_history(true)
        .max_history_size(MAX_HISTORY)?
        .completion_type(rustyline::CompletionType::List)
        .build();

    let fs = bash.fs();
    // Shared state snapshot for the helper (completion, hints).
    // Updated before each readline call.
    let state_ref = Arc::new(std::sync::Mutex::new(bash.shell_state()));
    let state_for_helper = Arc::clone(&state_ref);
    let helper = BashkitHelper {
        fs,
        state_fn: Box::new(move || state_for_helper.lock().unwrap().clone()),
    };

    let mut editor = Editor::with_config(config)?;
    editor.set_helper(Some(helper));

    let mut last_exit_code: i32 = 0;

    loop {
        // Update shared state for helper (completion, hints)
        *state_ref.lock().unwrap() = bash.shell_state();

        // Build prompt from PS1
        let state = bash.shell_state();
        let ps1 = state
            .variables
            .get("PS1")
            .or_else(|| state.env.get("PS1"))
            .cloned()
            .unwrap_or_else(|| DEFAULT_PS1.to_string());
        let prompt = expand_ps1(&ps1, &state);

        // Clear any stale SIGINT from previous iteration
        sigint_flag.store(false, Ordering::Relaxed);
        cancel_token.store(false, Ordering::Relaxed);

        let line = match editor.readline(&prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("bashkit: readline error: {e}");
                last_exit_code = 1;
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        // Multiline: accumulate lines when input is incomplete.
        let mut input = line;
        let result = loop {
            // Enable Ctrl-C cancellation during execution
            cancel_token.store(false, Ordering::Relaxed);
            sigint_flag.store(false, Ordering::Relaxed);

            let cancel = Arc::clone(&cancel_token);
            let sigint = Arc::clone(&sigint_flag);
            let cancel_watcher = tokio::spawn(async move {
                loop {
                    if sigint.load(Ordering::Relaxed) {
                        cancel.store(true, Ordering::Relaxed);
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            });

            let exec_result = bash
                .exec_streaming(
                    &input,
                    Box::new(|stdout, stderr| {
                        if !stdout.is_empty() {
                            print!("{stdout}");
                        }
                        if !stderr.is_empty() {
                            eprint!("{stderr}");
                        }
                    }),
                )
                .await;

            cancel_watcher.abort();
            cancel_token.store(false, Ordering::Relaxed);

            match exec_result {
                Ok(r) => break r,
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("cancelled") {
                        eprintln!();
                        break error_result(130);
                    }
                    if !is_incomplete_input(&msg) {
                        eprintln!("bashkit: {msg}");
                        break error_result(2);
                    }
                    // Read continuation line for incomplete input
                    let ps2 = bash
                        .shell_state()
                        .variables
                        .get("PS2")
                        .cloned()
                        .unwrap_or_else(|| DEFAULT_PS2.to_string());
                    match editor.readline(&ps2) {
                        Ok(cont) => {
                            input.push('\n');
                            input.push_str(&cont);
                        }
                        Err(ReadlineError::Interrupted) => break error_result(130),
                        Err(ReadlineError::Eof) => {
                            eprintln!("bashkit: unexpected end of file");
                            break error_result(2);
                        }
                        Err(e) => {
                            eprintln!("bashkit: readline error: {e}");
                            break error_result(1);
                        }
                    }
                }
            }
        };

        last_exit_code = result.exit_code;
    }

    Ok(last_exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_unterminated_single_quote() {
        assert!(is_incomplete_input("unterminated single quote"));
    }

    #[test]
    fn incomplete_unterminated_double_quote() {
        assert!(is_incomplete_input("unterminated double quote"));
    }

    #[test]
    fn incomplete_unexpected_end_of_input() {
        assert!(is_incomplete_input(
            "parse error at line 1, column 15: unexpected end of input in for loop"
        ));
    }

    #[test]
    fn incomplete_empty_body() {
        assert!(is_incomplete_input("syntax error: empty for loop body"));
        assert!(is_incomplete_input("syntax error: empty then clause"));
        assert!(is_incomplete_input("syntax error: empty else clause"));
        assert!(is_incomplete_input("syntax error: empty while loop body"));
        assert!(is_incomplete_input("syntax error: empty brace group"));
    }

    #[test]
    fn incomplete_missing_closing_keyword() {
        assert!(is_incomplete_input("expected 'fi'"));
        assert!(is_incomplete_input("expected 'done'"));
        assert!(is_incomplete_input("expected 'esac'"));
        assert!(is_incomplete_input("expected '}' to close brace group"));
    }

    #[test]
    fn complete_input_not_detected_as_incomplete() {
        assert!(!is_incomplete_input("command not found: foo"));
        assert!(!is_incomplete_input("syntax error near unexpected token"));
        assert!(!is_incomplete_input("execution error: division by zero"));
    }

    // --- PS1 tests ---

    fn empty_state() -> bashkit::ShellState {
        bashkit::ShellState {
            env: std::collections::HashMap::new(),
            variables: std::collections::HashMap::new(),
            arrays: std::collections::HashMap::new(),
            assoc_arrays: std::collections::HashMap::new(),
            cwd: std::path::PathBuf::from("/"),
            last_exit_code: 0,
            aliases: std::collections::HashMap::new(),
            traps: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn ps1_user_and_host() {
        let mut state = empty_state();
        state.env.insert("USER".into(), "mike".into());
        state.env.insert("HOSTNAME".into(), "dev.local".into());
        let result = expand_ps1("\\u@\\h$ ", &state);
        assert_eq!(result, "mike@dev$ ");
    }

    #[test]
    fn ps1_full_hostname() {
        let mut state = empty_state();
        state.env.insert("HOSTNAME".into(), "dev.local".into());
        let result = expand_ps1("\\H$ ", &state);
        assert_eq!(result, "dev.local$ ");
    }

    #[test]
    fn ps1_working_dir_with_tilde() {
        let mut state = empty_state();
        state.env.insert("HOME".into(), "/home/mike".into());
        state.cwd = "/home/mike/projects".into();
        let result = expand_ps1("\\w$ ", &state);
        assert_eq!(result, "~/projects$ ");
    }

    #[test]
    fn ps1_working_dir_basename() {
        let mut state = empty_state();
        state.cwd = "/home/mike/projects".into();
        let result = expand_ps1("\\W$ ", &state);
        assert_eq!(result, "projects$ ");
    }

    #[test]
    fn ps1_dollar_sign_non_root() {
        let mut state = empty_state();
        state.env.insert("EUID".into(), "1000".into());
        let result = expand_ps1("\\$ ", &state);
        assert_eq!(result, "$ ");
    }

    #[test]
    fn ps1_hash_sign_root() {
        let mut state = empty_state();
        state.env.insert("EUID".into(), "0".into());
        let result = expand_ps1("\\$ ", &state);
        assert_eq!(result, "# ");
    }

    #[test]
    fn ps1_default_prompt_format() {
        let mut state = empty_state();
        state.env.insert("USER".into(), "user".into());
        state.env.insert("HOME".into(), "/home/user".into());
        state.env.insert("EUID".into(), "1000".into());
        state.cwd = "/home/user".into();
        let result = expand_ps1(DEFAULT_PS1, &state);
        assert_eq!(result, "user@bashkit:~$ ");
    }

    #[test]
    fn ps1_newline_and_escape() {
        let state = empty_state();
        let result = expand_ps1("line1\\nline2", &state);
        assert_eq!(result, "line1\nline2");
    }

    // --- Prompt integration ---

    #[test]
    fn default_prompt_shows_cwd() {
        let bash = test_bash();
        let state = bash.shell_state();
        let prompt = expand_ps1(DEFAULT_PS1, &state);
        assert!(prompt.contains("user"));
        assert!(prompt.ends_with("$ "));
    }

    // --- Exec tests ---

    #[tokio::test]
    async fn piped_input_executes_and_exits() {
        let mut bash = test_bash();
        let result = bash.exec("echo hello").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn state_persists_across_exec_calls() {
        let mut bash = test_bash();
        bash.exec("X=42").await.unwrap();
        let result = bash.exec("echo $X").await.unwrap();
        assert_eq!(result.stdout, "42\n");
    }

    #[tokio::test]
    async fn cwd_changes_persist() {
        let mut bash = test_bash();
        bash.exec("mkdir -p /tmp/testdir").await.unwrap();
        bash.exec("cd /tmp/testdir").await.unwrap();
        let state = bash.shell_state();
        let prompt = expand_ps1("\\w$ ", &state);
        assert!(prompt.contains("/tmp/testdir"));
    }

    #[tokio::test]
    async fn tty_detection_works() {
        let mut bash = test_bash();
        let result = bash.exec("[ -t 0 ] && echo yes || echo no").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn streaming_output_callback_invoked() {
        let mut bash = test_bash();
        let chunks: Arc<std::sync::Mutex<Vec<String>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let chunks_cb = chunks.clone();
        let result = bash
            .exec_streaming(
                "echo one; echo two",
                Box::new(move |stdout, _stderr| {
                    if !stdout.is_empty() {
                        chunks_cb.lock().unwrap().push(stdout.to_string());
                    }
                }),
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        let collected = chunks.lock().unwrap();
        assert!(!collected.is_empty());
    }

    #[test]
    fn error_result_has_correct_exit_code() {
        let r = error_result(130);
        assert_eq!(r.exit_code, 130);
        assert!(r.stdout.is_empty());
        assert!(r.stderr.is_empty());
    }

    // --- Tab completion inside runtime ---

    #[tokio::test(flavor = "current_thread")]
    async fn tab_completion_does_not_panic_inside_runtime() {
        // Regression: complete_path() called handle.block_on() which panics
        // with "Cannot start a runtime from within a runtime" on single-thread.
        let bash = test_bash();
        let fs = bash.fs();
        // Create a file so completion has something to match
        let _ = fs
            .write_file(&std::path::PathBuf::from("/home/user/aat.txt"), b"test")
            .await;
        let state = bash.shell_state();
        let helper = BashkitHelper {
            fs: Arc::clone(&fs),
            state_fn: Box::new(move || state.clone()),
        };
        // This would panic before the fix with nested runtime error
        let results = helper.complete_path("aa");
        assert!(results.iter().any(|r| r.contains("aat.txt")));
    }

    // --- Source RC ---

    #[tokio::test]
    async fn source_rc_sets_variables() {
        let mut bash = test_bash();
        let fs = bash.fs();
        let rc = std::path::PathBuf::from(RC_FILE);
        let _ = fs.write_file(&rc, b"MY_RC_VAR=loaded\n").await;
        source_rc_file(&mut bash).await;
        let result = bash.exec("echo $MY_RC_VAR").await.unwrap();
        assert_eq!(result.stdout, "loaded\n");
    }
}
