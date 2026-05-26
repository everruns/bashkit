//! Regression test for #291: [ -f ] doesn't see VFS files after cd in script

use bashkit::Bash;
use std::path::Path;

#[tokio::test]
async fn issue_291_file_test_after_cd_in_script() {
    let mut bash = Bash::new();
    let fs = bash.fs();
    fs.mkdir(Path::new("/project"), true).await.unwrap();
    fs.write_file(Path::new("/project/test.txt"), b"hello")
        .await
        .unwrap();
    fs.write_file(
        Path::new("/check.sh"),
        b"#!/bin/bash\n[ -f test.txt ] && echo found || echo not-found",
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/check.sh"), 0o755).await.unwrap();

    let r = bash.exec("cd /project\n/check.sh").await.unwrap();
    assert_eq!(r.stdout.trim(), "found");
}

#[tokio::test]
async fn issue_291_double_bracket_file_test_after_cd() {
    let mut bash = Bash::new();
    let fs = bash.fs();
    fs.mkdir(Path::new("/mydir"), true).await.unwrap();
    fs.write_file(Path::new("/mydir/data.json"), b"{}")
        .await
        .unwrap();

    let r = bash
        .exec("cd /mydir\n[[ -f data.json ]] && echo ok || echo no")
        .await
        .unwrap();
    assert_eq!(r.stdout.trim(), "ok");
}
