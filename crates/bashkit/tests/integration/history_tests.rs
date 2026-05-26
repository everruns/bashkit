// Integration tests for persistent searchable history (issue #578)

use bashkit::{Bash, FileSystem};

async fn run(script: &str) -> bashkit::ExecResult {
    let mut bash = Bash::new();
    bash.exec(script).await.unwrap()
}

#[tokio::test]
async fn history_shows_previous_commands() {
    let mut bash = Bash::new();
    bash.exec("echo hello").await.unwrap();
    bash.exec("echo world").await.unwrap();
    let result = bash.exec("history").await.unwrap();
    assert!(
        result.stdout.contains("echo hello"),
        "should contain first command"
    );
    assert!(
        result.stdout.contains("echo world"),
        "should contain second command"
    );
}

#[tokio::test]
async fn history_n_limits_output() {
    let mut bash = Bash::new();
    bash.exec("echo a").await.unwrap();
    bash.exec("echo b").await.unwrap();
    bash.exec("echo c").await.unwrap();
    let result = bash.exec("history 2").await.unwrap();
    // Should show last 2 entries (echo c and history 2 itself won't be in history yet
    // because history is recorded after exec, and history builtin runs during exec)
    // Actually: echo a, echo b, echo c are recorded. history 2 shows last 2.
    assert!(
        !result.stdout.contains("echo a"),
        "should not contain first command"
    );
    assert!(result.stdout.contains("echo b") || result.stdout.contains("echo c"));
}

#[tokio::test]
async fn history_clear() {
    let mut bash = Bash::new();
    bash.exec("echo hello").await.unwrap();
    bash.exec("history -c").await.unwrap();
    let result = bash.exec("history").await.unwrap();
    // After clear, only the "history -c" line may be gone, and "history" itself hasn't been recorded yet
    // The history -c command itself is recorded AFTER exec, but clear happens DURING exec.
    // So: echo hello -> recorded after exec. history -c -> clears during exec, then records "history -c" after exec.
    // Then history -> shows "history -c" only.
    assert!(
        !result.stdout.contains("echo hello"),
        "history should be cleared"
    );
}

#[tokio::test]
async fn history_grep() {
    let mut bash = Bash::new();
    bash.exec("echo hello").await.unwrap();
    bash.exec("ls /tmp").await.unwrap();
    bash.exec("echo world").await.unwrap();
    let result = bash.exec("history --grep echo").await.unwrap();
    assert!(result.stdout.contains("echo hello"));
    assert!(result.stdout.contains("echo world"));
    assert!(!result.stdout.contains("ls /tmp"));
}

#[tokio::test]
async fn history_failed() {
    let mut bash = Bash::new();
    bash.exec("true").await.unwrap();
    bash.exec("false").await.unwrap();
    let result = bash.exec("history --failed").await.unwrap();
    assert!(
        result.stdout.contains("false"),
        "should show failed command"
    );
    assert!(
        !result.stdout.contains("true"),
        "should not show successful command"
    );
}

#[tokio::test]
async fn history_cwd_filter() {
    let mut bash = Bash::new();
    bash.exec("echo in-home").await.unwrap();
    bash.exec("cd /tmp && echo in-tmp").await.unwrap();
    let result = bash.exec("history --cwd /tmp").await.unwrap();
    // Commands executed while cwd was /tmp
    // Note: cd /tmp && echo in-tmp is recorded with the cwd at exec time.
    // Since cwd changes during the script, the recorded cwd is whatever it was at the start of exec.
    // Actually, the cwd is captured in lib.rs AFTER execution, so it will be /tmp for that script.
    assert!(result.stdout.contains("echo in-tmp") || result.stdout.contains("cd /tmp"));
}

#[tokio::test]
async fn history_invalid_option() {
    let result = run("history --invalid").await;
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("unrecognized option"));
}

#[tokio::test]
async fn history_grep_missing_arg() {
    let result = run("history --grep").await;
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("requires an argument"));
}

#[tokio::test]
async fn history_since_filter() {
    let mut bash = Bash::new();
    bash.exec("echo recent").await.unwrap();
    let result = bash.exec("history --since 1h").await.unwrap();
    assert!(
        result.stdout.contains("echo recent"),
        "recent entry should appear"
    );
}

#[tokio::test]
async fn history_since_invalid_duration() {
    let result = run("history --since xyz").await;
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("invalid duration"));
}

#[tokio::test]
async fn history_numbered_output() {
    let mut bash = Bash::new();
    bash.exec("echo test").await.unwrap();
    let result = bash.exec("history").await.unwrap();
    // Should have bash-style numbered output like "  1  echo test"
    assert!(
        result.stdout.contains("  1  echo test"),
        "output should be numbered: {}",
        result.stdout
    );
}

#[tokio::test]
async fn history_persists_to_vfs() {
    let mut bash = Bash::builder()
        .history_file("/home/user/.bash_history")
        .build();
    bash.exec("echo persisted").await.unwrap();

    // Create a new Bash instance with same history file and VFS
    // Since they share the same VFS through builder, history should persist
    // For this test, we verify the file was written
    let result = bash.exec("cat /home/user/.bash_history").await.unwrap();
    assert!(
        result.stdout.contains("echo persisted"),
        "history file should contain command: {}",
        result.stdout
    );
}

#[tokio::test]
async fn history_loads_from_vfs() {
    use std::sync::Arc;

    let fs = Arc::new(bashkit::InMemoryFs::new());
    // Pre-populate a history file
    let history_content = "1700000000|0|10|/home/user|echo preloaded\n";
    fs.mkdir(std::path::Path::new("/home/user"), true)
        .await
        .unwrap();
    fs.write_file(
        std::path::Path::new("/home/user/.bash_history"),
        history_content.as_bytes(),
    )
    .await
    .unwrap();

    let mut bash = Bash::builder()
        .fs(fs)
        .history_file("/home/user/.bash_history")
        .build();
    let result = bash.exec("history").await.unwrap();
    assert!(
        result.stdout.contains("echo preloaded"),
        "should load preexisting history: {}",
        result.stdout
    );
}

#[tokio::test]
async fn history_empty_when_no_commands() {
    let result = run("history").await;
    assert_eq!(result.stdout, "");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn history_does_not_record_comments() {
    let mut bash = Bash::new();
    bash.exec("# this is a comment").await.unwrap();
    bash.exec("echo visible").await.unwrap();
    let result = bash.exec("history").await.unwrap();
    assert!(!result.stdout.contains("comment"));
    assert!(result.stdout.contains("echo visible"));
}

#[tokio::test]
async fn history_does_not_record_blank_lines() {
    let mut bash = Bash::new();
    bash.exec("   ").await.unwrap();
    bash.exec("echo visible").await.unwrap();
    let result = bash.exec("history").await.unwrap();
    let lines: Vec<&str> = result.stdout.lines().collect();
    assert_eq!(lines.len(), 1, "should only have one entry: {:?}", lines);
}
