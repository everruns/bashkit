//! End-to-end tests for the in-process terminal (`terminal` feature):
//! drive a session the way an agent would (send keys, wait for idle, read the
//! screen as text) and edit a file with `vi`.

use bashkit::terminal::{Terminal, TerminalSize, TerminalStatus};
use bashkit::{Bash, FileSystem};
use std::path::Path;
use std::sync::Arc;

async fn step(term: &mut Terminal, keys: &str) -> String {
    term.send(keys);
    assert_eq!(term.run_until_idle().await, TerminalStatus::Idle);
    term.screen_text()
}

#[tokio::test]
async fn agent_edits_config_with_vi_and_reads_result() {
    let mut term = Terminal::with_size(Bash::builder(), TerminalSize::new(12, 60));
    term.run_until_idle().await;
    step(
        &mut term,
        "mkdir -p /app && printf 'port=80\\nhost=localhost\\n' > /app/conf\r",
    )
    .await;

    let screen = step(&mut term, "vi /app/conf\r").await;
    assert!(term.is_alternate_screen());
    assert!(screen.starts_with("port=80\nhost=localhost\n~"), "{screen}");

    // Change 80 -> 8080 on line 1, append a line at the end, save.
    let screen = step(&mut term, ":s/80/8080/\rGodebug=true\x1b").await;
    assert!(screen.contains("port=8080"), "{screen}");
    assert!(screen.contains("debug=true"), "{screen}");
    let screen = step(&mut term, ":wq\r").await;
    assert!(!term.is_alternate_screen());
    assert!(screen.ends_with("$ vi /app/conf\n$"), "{screen}");

    let fs: Arc<dyn FileSystem> = term.fs();
    let saved = fs.read_file(Path::new("/app/conf")).await.unwrap();
    assert_eq!(saved, b"port=8080\nhost=localhost\ndebug=true\n");

    let screen = step(&mut term, "grep -c = /app/conf\r").await;
    assert!(screen.ends_with("\n3\n$"), "{screen}");

    term.send("exit\r");
    assert_eq!(term.run_until_idle().await, TerminalStatus::Exited(0));
}

#[tokio::test]
async fn raw_output_stream_replays_to_same_screen() {
    // A host renderer fed `take_output()` sees exactly what the screen model saw.
    let mut term = Terminal::new(Bash::builder());
    term.run_until_idle().await;
    step(&mut term, "echo one; echo two\r").await;
    let raw = term.take_output();
    let text = String::from_utf8(raw).unwrap();
    assert!(text.contains("one\r\ntwo\r\n"), "{text:?}");
}

#[tokio::test]
async fn resize_redraws_vi() {
    let mut term = Terminal::with_size(Bash::builder(), TerminalSize::new(10, 40));
    term.run_until_idle().await;
    let body: String = (1..=30).map(|i| format!("l{i}\n")).collect();
    term.fs()
        .write_file(Path::new("/tmp/r.txt"), body.as_bytes())
        .await
        .unwrap();
    let screen = step(&mut term, "vi /tmp/r.txt\r").await;
    assert_eq!(screen.lines().count(), 10, "{screen}");
    term.resize(TerminalSize::new(20, 40));
    term.run_until_idle().await;
    let screen = term.screen_text();
    assert!(screen.contains("l19"), "{screen}");
    step(&mut term, ":q\r").await;
}

#[tokio::test]
async fn dropping_terminal_mid_edit_is_clean() {
    let mut term = Terminal::new(Bash::builder());
    term.run_until_idle().await;
    step(&mut term, "vi /tmp/x\r").await;
    step(&mut term, "iunsaved").await;
    drop(term);
}
