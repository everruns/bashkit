//! Drive an in-process terminal like an agent would: type keystrokes, wait
//! until the shell needs input, read the screen as text, and edit a file in
//! `vi`.
//!
//! Run with: cargo run --example terminal_vi --features terminal

use bashkit::Bash;
use bashkit::terminal::{Terminal, TerminalSize, TerminalStatus};

async fn show(term: &mut Terminal, keys: &str, label: &str) {
    term.send(keys);
    term.run_until_idle().await;
    println!("--- {label} ---\n{}\n", term.screen_text());
}

#[tokio::main]
async fn main() {
    let mut term = Terminal::with_size(Bash::builder(), TerminalSize::new(8, 50));
    term.run_until_idle().await;

    show(
        &mut term,
        "echo 'todo: write docs' > /tmp/notes.txt\r",
        "shell",
    )
    .await;
    show(&mut term, "vi /tmp/notes.txt\r", "vi opened").await;
    show(&mut term, "A, ship it\x1bo- add tests\x1b", "vi edited").await;
    show(&mut term, ":wq\r", "back at the prompt").await;
    show(&mut term, "cat /tmp/notes.txt\r", "result").await;

    let saved = term
        .fs()
        .read_file("/tmp/notes.txt".as_ref())
        .await
        .unwrap();
    assert_eq!(saved, b"todo: write docs, ship it\n- add tests\n");

    term.send("exit\r");
    assert_eq!(term.run_until_idle().await, TerminalStatus::Exited(0));
    println!("ok");
}
