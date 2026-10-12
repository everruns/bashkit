use bashkit::{
    Bash, Error, ExecOptions, ExecutionBudgetExceeded, ExecutionLimits, LimitExceeded, StreamData,
};
use std::path::Path;

#[tokio::test]
async fn tar_files_from_large_content_obeys_live_budget() {
    for content in [vec![b'x'; 20_000], vec![0xff; 4000]] {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_live_intermediate_bytes(8192))
            .build();
        bash.fs()
            .write_file(Path::new("/list"), &content)
            .await
            .unwrap();
        let result = bash.exec("tar -cf /out.tar -T /list").await;
        assert!(
            matches!(
                result,
                Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                    ExecutionBudgetExceeded::LiveBytes { .. }
                )))
            ),
            "expected budget rejection"
        );
        assert_eq!(
            bash.exec("echo recovered").await.unwrap().stdout,
            "recovered\n"
        );
    }
}

#[tokio::test]
async fn tar_files_from_dense_lines_obey_work_budget() {
    for content in [
        "x\n".repeat(2000).into_bytes(),
        "\n".repeat(2000).into_bytes(),
        "x".repeat(20_000).into_bytes(),
        vec![0xff; 128],
    ] {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_work_units(100))
            .build();
        bash.fs()
            .write_file(Path::new("/list"), &content)
            .await
            .unwrap();
        let result = bash.exec("tar -cf /out.tar -T /list").await;
        assert!(
            matches!(
                result,
                Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                    ExecutionBudgetExceeded::WorkUnits { .. }
                )))
            ),
            "expected budget rejection"
        );
        assert_eq!(
            bash.exec("echo recovered").await.unwrap().stdout,
            "recovered\n"
        );
    }
}

#[tokio::test]
async fn tar_files_from_stdin_and_repeated_lists_obey_live_budget() {
    for (script, content) in [
        ("tar -cf /out.tar -T -".to_string(), "x".repeat(20_000)),
        (
            format!("tar -cf /out.tar {}", "-T /list ".repeat(40)),
            format!("{}\n", "x".repeat(256)),
        ),
    ] {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_live_intermediate_bytes(8192))
            .build();
        bash.fs()
            .write_file(Path::new("/list"), content.as_bytes())
            .await
            .unwrap();
        let result = bash
            .exec_with_options(&script, ExecOptions::new().stdin(StreamData::from(content)))
            .await;
        assert!(
            matches!(
                result,
                Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                    ExecutionBudgetExceeded::LiveBytes { .. }
                )))
            ),
            "expected budget rejection"
        );
        assert!(!bash.fs().exists(Path::new("/out.tar")).await.unwrap());
        assert_eq!(
            bash.exec("echo recovered").await.unwrap().stdout,
            "recovered\n"
        );
    }
}

#[tokio::test]
async fn tar_files_from_content_boundary_and_empty_lists() {
    for (bytes, succeeds) in [(8000, true), (8192, false), (8193, false)] {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_live_intermediate_bytes(8192))
            .build();
        bash.fs()
            .write_file(Path::new("/list"), &vec![b'\n'; bytes])
            .await
            .unwrap();
        let result = bash.exec("tar -cf /out.tar -T /list").await;
        if succeeds {
            let result = result.unwrap();
            assert_eq!(result.exit_code, 2);
            assert!(result.stderr.contains("empty archive"));
        } else {
            assert!(
                matches!(
                    result,
                    Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                        ExecutionBudgetExceeded::LiveBytes { .. }
                    )))
                ),
                "expected budget rejection"
            );
        }
    }
}

#[tokio::test]
async fn tar_files_from_preserves_order_duplicates_crlf_and_lossy_names() {
    let mut bash = Bash::new();
    for name in ["a", "b", "\u{fffd}"] {
        bash.fs()
            .write_file(Path::new(&format!("/{name}")), b"data")
            .await
            .unwrap();
    }
    bash.fs()
        .write_file(Path::new("/list"), b"a\r\n\nb\na\n\xff")
        .await
        .unwrap();
    let result = bash
        .exec("cd /; tar -cf /out.tar -T /list; tar -tf /out.tar")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "a\nb\na\n\u{fffd}\n");
}

#[tokio::test]
async fn tar_files_from_matches_real_bash_tar() {
    let script = "printf a > a; printf b > b; printf 'a\\n\\nb\\na' > list; cp list list2; tar -cf out.tar -T list --files-from=list2; tar -tf out.tar; printf 'b\\n' | tar -cf stdin.tar -T -; tar -tf stdin.tar";
    let mut bash = Bash::new();
    let actual = bash.exec(script).await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let host_script = if std::process::Command::new("gtar")
        .arg("--version")
        .output()
        .is_ok()
    {
        script
            .replace("; tar ", "; gtar ")
            .replace("| tar ", "| gtar ")
    } else {
        script.to_string()
    };
    let expected = std::process::Command::new("bash")
        .args(["-c", &host_script])
        .env("COPYFILE_DISABLE", "1")
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        expected.status.success(),
        "{}",
        String::from_utf8_lossy(&expected.stderr)
    );
    assert_eq!(actual.exit_code, 0, "{}", actual.stderr);
    assert_eq!(actual.stdout.as_bytes(), expected.stdout);
}

#[tokio::test]
async fn tar_files_from_repeated_reads_share_input_budget() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_aggregate_input_bytes(100))
        .build();
    bash.fs()
        .write_file(Path::new("/list"), "a\n".repeat(20).as_bytes())
        .await
        .unwrap();
    let result = bash.exec("tar -cf /out.tar -T /list -T /list").await;
    assert!(matches!(
        result,
        Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
            ExecutionBudgetExceeded::InputBytes { .. }
        )))
    ));
    assert!(!bash.fs().exists(Path::new("/out.tar")).await.unwrap());
    assert_eq!(
        bash.exec("echo recovered").await.unwrap().stdout,
        "recovered\n"
    );
}
