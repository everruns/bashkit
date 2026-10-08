//! Caps and isolation for the interpreter's interactive state: history
//! (TM-DOS-131), completion specs and key bindings (TM-DOS-132). Each
//! `Bash` instance owns its state; a `bash -i` child's history is its own.

use std::sync::Arc;

use bashkit::{Bash, ExecutionLimits, FileSystem, InMemoryFs};

async fn run(bash: &mut Bash, script: &str) -> bashkit::ExecResult {
    bash.exec(script).await.unwrap()
}

#[tokio::test]
async fn completion_specs_are_bounded() {
    let mut bash = Bash::new();
    let r = run(
        &mut bash,
        "for i in $(seq 1024); do complete -W x c$i || echo bad$i; done\n\
         complete -W x one_more; echo status=$?\n\
         complete -p | wc -l",
    )
    .await;
    assert!(!r.stdout.contains("bad"), "stdout: {}", r.stdout);
    assert!(r.stdout.contains("status=1"), "stdout: {}", r.stdout);
    assert!(
        r.stdout.trim_end().ends_with("1024"),
        "stdout: {}",
        r.stdout
    );
    // Replacing an existing spec still works at the cap.
    let r = run(&mut bash, "complete -W y c1; echo $?").await;
    assert_eq!(r.stdout, "0\n");
}

#[tokio::test]
async fn completion_spec_text_is_capped() {
    let mut bash = Bash::new();
    let r = run(
        &mut bash,
        "big=$(head -c 70000 /dev/zero | tr '\\0' a)\n\
         complete -W \"$big\" cmd; echo status=$?\n\
         complete -p cmd >/dev/null 2>&1; echo listed=$?",
    )
    .await;
    assert!(r.stdout.contains("status=1"), "stdout: {}", r.stdout);
    assert!(r.stdout.contains("listed=1"), "stdout: {}", r.stdout);
}

#[tokio::test]
async fn binding_changes_are_bounded() {
    let mut bash = Bash::new();
    let r = run(
        &mut bash,
        "for i in $(seq 1025); do bind \"\\\"\\\\C-x$i\\\": abort\" 2>/dev/null || echo fail$i; done",
    )
    .await;
    assert_eq!(r.stdout, "fail1025\n");
}

#[tokio::test]
async fn completion_state_is_per_instance() {
    let mut a = Bash::new();
    let mut b = Bash::new();
    run(
        &mut a,
        "complete -W 'x y' tool; bind '\"\\C-xq\": abort' 2>/dev/null",
    )
    .await;
    let r = run(
        &mut b,
        "complete -p tool; echo st=$?; bind -s; bind -p | grep -c C-xq",
    )
    .await;
    assert!(r.stdout.contains("st=1"), "stdout: {}", r.stdout);
    assert!(r.stdout.ends_with("0\n"), "stdout: {}", r.stdout);
    let r = run(&mut a, "complete -p tool").await;
    assert_eq!(r.stdout, "complete -W 'x y' tool\n");
}

#[tokio::test]
async fn history_lines_stay_bounded() {
    let limits = ExecutionLimits::new().max_history_entries(5);
    let mut bash = Bash::builder().limits(limits).build();
    let mut script = String::from("HISTSIZE=100000; set -o history\n");
    for i in 0..40 {
        script.push_str(&format!("echo {i} >/dev/null\n"));
    }
    script.push_str("history | wc -l\n");
    let r = run(&mut bash, &script).await;
    let n: usize = r.stdout.trim().parse().unwrap();
    assert!(n <= 5, "history kept {n} lines");
}

#[tokio::test]
async fn histfile_read_is_capped() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(std::path::Path::new("/tmp/big"), &vec![b'a'; 4096])
        .await
        .unwrap();
    fs.write_file(std::path::Path::new("/tmp/small"), b"echo small\n")
        .await
        .unwrap();
    let limits = ExecutionLimits::new().max_input_bytes(2048);
    let mut bash = Bash::builder().fs(fs).limits(limits).build();
    let r = run(
        &mut bash,
        "history -r /tmp/big; echo big=$?; history -r /tmp/small; echo small=$?; history | grep -c aaaa",
    )
    .await;
    assert_eq!(r.stdout, "big=1\nsmall=0\n0\n", "stderr: {}", r.stderr);
}

#[tokio::test]
async fn interactive_child_history_stays_in_child() {
    let mut bash = Bash::new();
    let r = run(
        &mut bash,
        "bash -i -c 'history -s child_secret; history | grep -c child_secret' 2>/dev/null\n\
         history | grep -c child_secret",
    )
    .await;
    assert_eq!(r.stdout, "1\n0\n");
}

#[tokio::test]
async fn interactive_histfile_is_written_to_vfs() {
    let fs = Arc::new(InMemoryFs::new());
    let mut bash = Bash::builder().fs(fs.clone()).build();
    let r = run(
        &mut bash,
        "mkdir -p \"$HOME\"; printf 'echo one\\necho two\\n' | bash -i 2>/dev/null; cat \"$HOME/.bash_history\"",
    )
    .await;
    assert_eq!(r.stdout, "one\ntwo\necho one\necho two\n");
    let home = run(&mut bash, "echo $HOME").await.stdout;
    let path = format!("{}/.bash_history", home.trim());
    let saved = fs.read_file(std::path::Path::new(&path)).await.unwrap();
    assert_eq!(saved, b"echo one\necho two\n");
}
