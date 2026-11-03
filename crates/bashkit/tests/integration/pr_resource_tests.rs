//! Pagination must admit repeated headers before allocating them.
use bashkit::{Bash, ExecutionLimits};
use std::path::Path;

#[tokio::test]
async fn pr_repeated_headers_respect_live_memory_and_recover() {
    for mode in ["", "-m"] {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_live_intermediate_bytes(131_072))
            .build();
        bash.fs()
            .write_file(Path::new("/lines"), &vec![b'\n'; 256])
            .await
            .unwrap();
        let plain = bash.exec(&format!("pr {mode} -t /lines")).await.unwrap();
        assert_eq!(plain.stdout.len(), 256);
        let script = format!("pr {mode} -l 11 -D DATE -h '{}' /lines", "H".repeat(1024));
        let result = bash.exec(&script).await;
        assert!(
            result.is_err(),
            "unmetered pagination succeeded: {result:?}"
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("live intermediate")
        );
        assert_eq!(
            bash.exec("echo recovered").await.unwrap().stdout,
            "recovered\n"
        );
    }
}

#[tokio::test]
async fn pr_output_amplification_fails_and_session_recovers() {
    for mode in ["", "-m", "-F", "-d", "+2:4096", "-o 7"] {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_live_intermediate_bytes(64 * 1024 * 1024))
            .build();
        bash.fs()
            .write_file(Path::new("/lines"), &vec![b'\n'; 4096])
            .await
            .unwrap();
        let script = format!("pr {mode} -l 11 -D DATE -h '{}' /lines", "H".repeat(4096));
        let error = bash.exec(&script).await.unwrap_err().to_string();
        assert!(
            error.contains("pr: output exceeds 16777216 bytes"),
            "{mode}: {error}"
        );
        assert_eq!(
            bash.exec("echo recovered").await.unwrap().stdout,
            "recovered\n"
        );
    }
}

#[tokio::test]
async fn pr_multiple_files_share_output_cap() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(64 * 1024 * 1024))
        .build();
    bash.fs()
        .write_file(Path::new("/lines"), &vec![b'\n'; 2048])
        .await
        .unwrap();
    let args = format!("-l 11 -D DATE -h '{}'", "H".repeat(4096));
    assert_eq!(
        bash.exec(&format!("pr {args} /lines"))
            .await
            .unwrap()
            .exit_code,
        0
    );
    assert!(
        bash.exec(&format!("pr {args} /lines /lines"))
            .await
            .unwrap_err()
            .to_string()
            .contains("output exceeds")
    );
}

#[tokio::test]
async fn pr_line_metadata_and_work_are_budgeted() {
    for limits in [
        ExecutionLimits::new().max_live_intermediate_bytes(4096),
        ExecutionLimits::new().max_work_units(900),
        ExecutionLimits::new().max_aggregate_input_bytes(128),
    ] {
        let mut bash = Bash::builder().limits(limits).build();
        bash.fs()
            .write_file(Path::new("/lines"), &vec![b'\n'; 256])
            .await
            .unwrap();
        assert!(bash.exec("pr -l 11 +257 /lines").await.is_err());
        assert_eq!(
            bash.exec("echo recovered").await.unwrap().stdout,
            "recovered\n"
        );
    }
}

#[tokio::test]
async fn pr_finite_pagination_matches_real_bash_and_gnu_pr() {
    use std::process::Command;
    let program = ["gpr", "pr"].into_iter().find(|program| {
        Command::new(program)
            .arg("--version")
            .output()
            .is_ok_and(|out| {
                out.status.success()
                    && String::from_utf8_lossy(&out.stdout).contains("GNU coreutils")
            })
    });
    let Some(program) = program else {
        assert!(
            !(cfg!(target_os = "linux") && std::env::var_os("CI").is_some()),
            "GNU pr required in Linux CI"
        );
        eprintln!("skip: GNU pr unavailable");
        return;
    };
    let header = "é".repeat(128);
    for mode in [
        "", "-m", "-F", "-d", "+2:3", "-o 3", "-2", "-2 -a", "-2 -J -n",
    ] {
        let script =
            format!("printf 'a\nb\nc\nd\n' | {program} {mode} -l 11 -D DATE -h '{header}'");
        let expected = Command::new("bash")
            .args(["-c", &script])
            .env("LC_ALL", "C.UTF-8")
            .output()
            .unwrap();
        assert!(
            expected.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        let mut bash = Bash::new();
        let actual = bash.exec(&script.replace(program, "pr")).await.unwrap();
        assert_eq!(actual.exit_code, 0);
        assert_eq!(actual.stdout.as_bytes(), expected.stdout, "{mode}");
    }
}
