//! Tests for BASH_SOURCE array variable

use bashkit::Bash;
use std::path::Path;

/// BASH_SOURCE[0] is set when executing a script by path
#[tokio::test]
async fn bash_source_set_in_script() {
    let mut bash = Bash::new();
    let fs = bash.fs();
    fs.write_file(
        Path::new("/test.sh"),
        b"#!/bin/bash\necho \"source=${BASH_SOURCE[0]}\"",
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/test.sh"), 0o755).await.unwrap();

    let result = bash.exec("/test.sh").await.unwrap();
    assert_eq!(result.stdout.trim(), "source=/test.sh");
}

/// BASH_SOURCE[0] is set when sourcing a file
#[tokio::test]
async fn bash_source_set_in_sourced_file() {
    let mut bash = Bash::new();
    let fs = bash.fs();
    fs.write_file(Path::new("/lib.sh"), b"echo \"source=${BASH_SOURCE[0]}\"")
        .await
        .unwrap();

    let result = bash.exec("source /lib.sh").await.unwrap();
    assert_eq!(result.stdout.trim(), "source=/lib.sh");
}

/// Source guard pattern: BASH_SOURCE[0] == $0 when executed directly
#[tokio::test]
async fn bash_source_guard_direct_execution() {
    let mut bash = Bash::new();
    let fs = bash.fs();
    fs.write_file(
        Path::new("/guard.sh"),
        b"#!/bin/bash\nif [[ \"${BASH_SOURCE[0]}\" == \"$0\" ]]; then echo direct; else echo sourced; fi",
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/guard.sh"), 0o755).await.unwrap();

    let result = bash.exec("/guard.sh").await.unwrap();
    assert_eq!(result.stdout.trim(), "direct");
}

/// Source guard pattern: BASH_SOURCE[0] != $0 when sourced
#[tokio::test]
async fn bash_source_guard_sourced() {
    let mut bash = Bash::new();
    let fs = bash.fs();
    fs.write_file(
        Path::new("/guard.sh"),
        b"if [[ \"${BASH_SOURCE[0]}\" == \"$0\" ]]; then echo direct; else echo sourced; fi",
    )
    .await
    .unwrap();

    let result = bash.exec("source /guard.sh").await.unwrap();
    assert_eq!(result.stdout.trim(), "sourced");
}
