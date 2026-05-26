//! Tests for `set -a` (allexport) behavior.
//!
//! When allexport is enabled, every variable assignment should also mark
//! the variable as exported (visible to child scripts via env).

use bashkit::Bash;
use std::path::Path;

/// Basic allexport: variables assigned while set -a is active are exported
#[tokio::test]
async fn allexport_basic_export_to_subprocess() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/check.sh"),
        b"#!/bin/bash\necho \"FOO=${FOO:-unset}\"\necho \"BAZ=${BAZ:-unset}\"\necho \"AFTER=${AFTER:-unset}\"",
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/check.sh"), 0o755).await.unwrap();

    let result = bash
        .exec(
            r#"
set -a
FOO="bar"
BAZ="qux"
set +a
AFTER="not-exported"
/check.sh
"#,
        )
        .await
        .unwrap();

    let lines: Vec<&str> = result.stdout.trim().lines().collect();
    assert_eq!(lines[0], "FOO=bar");
    assert_eq!(lines[1], "BAZ=qux");
    assert_eq!(lines[2], "AFTER=unset");
}

/// allexport with source: sourced env files get exported
#[tokio::test]
async fn allexport_with_source() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/vars.env"), b"DB_HOST=localhost\nDB_PORT=5432")
        .await
        .unwrap();

    fs.write_file(
        Path::new("/check.sh"),
        b"#!/bin/bash\necho \"host=${DB_HOST:-unset}\"\necho \"port=${DB_PORT:-unset}\"",
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/check.sh"), 0o755).await.unwrap();

    let result = bash
        .exec(
            r#"
set -a
source /vars.env
set +a
/check.sh
"#,
        )
        .await
        .unwrap();

    let lines: Vec<&str> = result.stdout.trim().lines().collect();
    assert_eq!(lines[0], "host=localhost");
    assert_eq!(lines[1], "port=5432");
}

/// set -o allexport / set +o allexport work as aliases
#[tokio::test]
async fn allexport_long_option_form() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/check.sh"),
        b"#!/bin/bash\necho \"X=${X:-unset}\"\necho \"Y=${Y:-unset}\"",
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/check.sh"), 0o755).await.unwrap();

    let result = bash
        .exec(
            r#"
set -o allexport
X="hello"
set +o allexport
Y="world"
/check.sh
"#,
        )
        .await
        .unwrap();

    let lines: Vec<&str> = result.stdout.trim().lines().collect();
    assert_eq!(lines[0], "X=hello");
    assert_eq!(lines[1], "Y=unset");
}

/// Variables assigned before set -a are not retroactively exported
#[tokio::test]
async fn allexport_not_retroactive() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/check.sh"),
        b"#!/bin/bash\necho \"BEFORE=${BEFORE:-unset}\"\necho \"DURING=${DURING:-unset}\"",
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/check.sh"), 0o755).await.unwrap();

    let result = bash
        .exec(
            r#"
BEFORE="exists"
set -a
DURING="new"
set +a
/check.sh
"#,
        )
        .await
        .unwrap();

    let lines: Vec<&str> = result.stdout.trim().lines().collect();
    assert_eq!(lines[0], "BEFORE=unset");
    assert_eq!(lines[1], "DURING=new");
}

/// allexport applies to for loop variable
#[tokio::test]
async fn allexport_for_loop_variable() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/check.sh"),
        b"#!/bin/bash\necho \"item=${item:-unset}\"",
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/check.sh"), 0o755).await.unwrap();

    let result = bash
        .exec(
            r#"
set -a
for item in alpha beta; do
  :
done
set +a
/check.sh
"#,
        )
        .await
        .unwrap();

    assert_eq!(result.stdout.trim(), "item=beta");
}
