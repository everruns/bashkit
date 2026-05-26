//! Regression test for #290: while/case arg parsing hits MaxLoopIterations

use bashkit::Bash;
use std::path::Path;

#[tokio::test]
async fn issue_290_while_case_shift_loop() {
    let mut bash = Bash::new();
    let fs = bash.fs();
    fs.write_file(
        Path::new("/parse_args.sh"),
        b"#!/bin/bash\nset -e\n\nwhile [[ $# -gt 0 ]]; do\n    case $1 in\n        --name)\n            NAME=\"$2\"\n            shift 2\n            ;;\n        --value)\n            VALUE=\"$2\"\n            shift 2\n            ;;\n        *)\n            echo \"Unknown: $1\"\n            exit 1\n            ;;\n    esac\ndone\n\necho \"name=$NAME value=$VALUE\"",
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/parse_args.sh"), 0o755).await.unwrap();
    let r = bash
        .exec("/parse_args.sh --name foo --value bar")
        .await
        .unwrap();
    assert_eq!(r.stdout.trim(), "name=foo value=bar");
    assert_eq!(r.exit_code, 0);
}

#[tokio::test]
async fn issue_290_shift_1_default() {
    let mut bash = Bash::new();
    let fs = bash.fs();
    fs.write_file(
        Path::new("/shift1.sh"),
        b"#!/bin/bash\nwhile [[ $# -gt 0 ]]; do\n    echo \"$1\"\n    shift\ndone",
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/shift1.sh"), 0o755).await.unwrap();
    let r = bash.exec("/shift1.sh a b c").await.unwrap();
    assert_eq!(r.stdout.trim(), "a\nb\nc");
}
