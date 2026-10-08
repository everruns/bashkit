//! Recursive grep must bound traversal and keep only the current file alive.
use bashkit::{Bash, Error, ExecutionLimits, LimitExceeded};
use std::path::Path;

#[tokio::test]
async fn grep_recursive_cycle_is_not_revisited() {
    let mut bash = Bash::new();
    bash.exec("mkdir /d; echo x > /d/f; ln -s . /d/a")
        .await
        .unwrap();
    // One alias has at most 41 visits on the unfixed implementation; safe to run.
    let result = bash.exec("grep -R x /d").await.unwrap();
    assert_eq!(result.stdout.to_string(), "/d/f:x\n");
    assert_eq!(
        result.stderr.to_string(),
        "grep: /d/a: warning: recursive directory loop\n"
    );
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn grep_recursive_collection_respects_work_budget() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_work_units(100))
        .build();
    for i in 0..200 {
        bash.fs()
            .mkdir(Path::new(&format!("/d/{i}")), true)
            .await
            .unwrap();
    }
    let result = bash.exec("grep -Rq x /d").await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
        ),
        "{result:?}"
    );
}

#[tokio::test]
async fn grep_recursive_file_is_admitted_before_reading() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(4096))
        .build();
    bash.fs().mkdir(Path::new("/d"), true).await.unwrap();
    bash.fs()
        .write_file(Path::new("/d/f"), &vec![b'x'; 8192])
        .await
        .unwrap();
    let result = bash.exec("grep -Rq x /d").await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
        ),
        "{result:?}"
    );
}

#[tokio::test]
async fn grep_recursive_two_aliases_terminate_with_and_without_a_match() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_work_units(1000))
        .build();
    bash.exec("mkdir /d; echo x > /d/f; ln -s . /d/a; ln -s . /d/b")
        .await
        .unwrap();
    let quiet = bash.exec("grep -Rq x /d").await.unwrap();
    assert_eq!(quiet.exit_code, 0);
    assert!(quiet.stdout.is_empty());
    assert!(quiet.stderr.is_empty());
    let absent = bash.exec("grep -R missing /d").await.unwrap();
    assert_eq!(absent.exit_code, 1);
    assert!(absent.stdout.is_empty());
    assert_eq!(
        absent.stderr.to_string(),
        "grep: /d/a: warning: recursive directory loop\ngrep: /d/b: warning: recursive directory loop\n"
    );
    let silent = bash.exec("grep -Rs missing /d").await.unwrap();
    assert_eq!(silent.exit_code, 1);
    assert!(silent.stderr.is_empty());
}

#[tokio::test]
async fn grep_recursive_sibling_aliases_are_independent_and_r_skips_links() {
    let mut bash = Bash::new();
    bash.exec("mkdir /d /target; echo x > /target/f; ln -s /target /d/a; ln -s /target /d/b")
        .await
        .unwrap();
    let result = bash.exec("grep -R x /d").await.unwrap();
    assert_eq!(result.stdout.to_string(), "/d/a/f:x\n/d/b/f:x\n");
    assert!(result.stderr.is_empty());
    let skipped = bash.exec("grep -r x /d").await.unwrap();
    assert_eq!(skipped.exit_code, 1);
    assert!(skipped.stdout.is_empty());
}

#[tokio::test]
async fn grep_recursive_root_alias_and_parent_cycle_are_detected() {
    let mut bash = Bash::new();
    bash.exec("mkdir -p /d/child; echo x > /d/f; ln -s .. /d/child/up; ln -s /d /alias")
        .await
        .unwrap();
    let result = bash.exec("grep -R x /alias").await.unwrap();
    assert_eq!(result.stdout.to_string(), "/alias/f:x\n");
    assert_eq!(
        result.stderr.to_string(),
        "grep: /alias/child/up: warning: recursive directory loop\n"
    );
}

#[tokio::test]
async fn grep_recursive_releases_each_file_buffer() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(4096))
        .build();
    bash.fs().mkdir(Path::new("/d"), true).await.unwrap();
    for i in 0..6 {
        bash.fs()
            .write_file(Path::new(&format!("/d/f{i}")), &vec![b'x'; 1024])
            .await
            .unwrap();
    }
    let result = bash.exec("grep -Rc missing /d").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert_eq!(result.stdout.to_string().lines().count(), 6);
}

#[tokio::test]
async fn grep_recursive_files_share_aggregate_input_budget() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_aggregate_input_bytes(1500))
        .build();
    bash.fs().mkdir(Path::new("/d"), true).await.unwrap();
    for i in 0..2 {
        bash.fs()
            .write_file(Path::new(&format!("/d/f{i}")), &vec![b'x'; 1024])
            .await
            .unwrap();
    }
    let result = bash.exec("grep -R missing /d").await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
        ),
        "{result:?}"
    );
}

#[tokio::test]
async fn grep_recursive_cycle_diagnostic_is_bounded_utf8() {
    let mut bash = Bash::new();
    let mut dir = std::path::PathBuf::from("/d");
    for _ in 0..6 {
        dir.push("é".repeat(100));
    }
    bash.fs().mkdir(&dir, true).await.unwrap();
    bash.fs()
        .symlink(Path::new("."), &dir.join("loop"))
        .await
        .unwrap();
    let result = bash
        .exec(&format!("grep -R missing {}", dir.display()))
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.as_bytes().len() <= 1024);
    assert!(std::str::from_utf8(result.stderr.as_bytes()).is_ok());
    assert!(result.stderr.to_string().starts_with("grep: "));
}
