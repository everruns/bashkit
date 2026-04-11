// Interactive shell mode using rustyline for line editing.
// See specs/018-interactive-shell.md

use anyhow::Result;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

const CONTINUATION_PROMPT: &str = "> ";

fn prompt(bash: &bashkit::Bash) -> String {
    let cwd = bash.shell_state().cwd.display().to_string();
    format!("bashkit:{cwd}$ ")
}

/// Returns true if the parse error indicates incomplete input that
/// should trigger a continuation prompt.
fn is_incomplete_input(err_msg: &str) -> bool {
    let lower = err_msg.to_lowercase();
    lower.contains("unterminated")
        || lower.contains("unexpected end of input")
        || lower.contains("unexpected eof")
}

fn error_result(exit_code: i32) -> bashkit::ExecResult {
    bashkit::ExecResult {
        exit_code,
        ..Default::default()
    }
}

/// Build a Bash instance suitable for interactive mode testing.
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
    let mut editor = DefaultEditor::new()?;
    let mut last_exit_code: i32 = 0;

    loop {
        let current_prompt = prompt(&bash);
        let line = match editor.readline(&current_prompt) {
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

        // Accumulate lines when input is incomplete (multiline support).
        let mut input = line;
        let result = loop {
            match bash
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
                .await
            {
                Ok(result) => break result,
                Err(e) => {
                    let msg = e.to_string();
                    if !is_incomplete_input(&msg) {
                        eprintln!("bashkit: {msg}");
                        break error_result(2);
                    }
                    match editor.readline(CONTINUATION_PROMPT) {
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

        let _ = editor.add_history_entry(&input);
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
    fn complete_input_not_detected_as_incomplete() {
        assert!(!is_incomplete_input("command not found: foo"));
        assert!(!is_incomplete_input("syntax error near unexpected token"));
        assert!(!is_incomplete_input("execution error: division by zero"));
    }

    #[test]
    fn prompt_shows_cwd() {
        let bash = test_bash();
        let p = prompt(&bash);
        assert!(p.starts_with("bashkit:"));
        assert!(p.ends_with("$ "));
        assert!(p.contains("/home/user"));
    }

    #[tokio::test]
    async fn piped_input_executes_and_exits() {
        // Simulate non-interactive stdin by running exec directly
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
        let p = prompt(&bash);
        assert!(p.contains("/tmp/testdir"));
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
        let chunks: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
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
}
