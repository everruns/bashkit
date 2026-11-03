//! Ordinary join rows exercise configured budgets through the public API.
use bashkit::{Bash, Error, ExecutionLimits, LimitExceeded};
use std::path::Path;

async fn two_tables(limits: ExecutionLimits) -> Bash {
    let bash = Bash::builder().limits(limits).build();
    bash.fs()
        .write_file(Path::new("/left"), b"a 1\na 2\n")
        .await
        .unwrap();
    bash.fs()
        .write_file(Path::new("/right"), b"a x\na y\n")
        .await
        .unwrap();
    bash
}

#[tokio::test]
async fn join_ordinary_rows_charge_generated_work() {
    let mut bash = two_tables(ExecutionLimits::new().max_work_units(70)).await;
    let result = bash.exec("join /left /right").await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
        ),
        "{result:?}"
    );
    assert!(result.unwrap_err().to_string().contains("work units"));
    assert_eq!(
        bash.exec("echo recovered").await.unwrap().stdout,
        "recovered\n"
    );
}

#[tokio::test]
async fn join_ordinary_cartesian_rows_survive_pipe_and_redirect() {
    let mut bash = two_tables(ExecutionLimits::new().max_stdout_bytes(8)).await;
    let expected = b"a 1 x\na 1 y\na 2 x\na 2 y\n";
    assert_eq!(
        bash.exec("join /left /right | wc -c")
            .await
            .unwrap()
            .stdout
            .trim(),
        "24"
    );
    assert_eq!(
        bash.exec("join /left /right > /joined")
            .await
            .unwrap()
            .exit_code,
        0
    );
    assert_eq!(
        bash.fs().read_file(Path::new("/joined")).await.unwrap(),
        expected
    );
}

#[tokio::test]
async fn join_ordinary_input_and_memory_limits_recover() {
    for limits in [
        ExecutionLimits::new().max_aggregate_input_bytes(7),
        ExecutionLimits::new().max_live_intermediate_bytes(512),
    ] {
        let mut bash = two_tables(limits).await;
        let result = bash.exec("join /left /right").await;
        assert!(
            matches!(
                result,
                Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
            ),
            "{result:?}"
        );
        assert_eq!(bash.exec("echo x").await.unwrap().stdout, "x\n");
    }
}

#[tokio::test]
async fn join_ordinary_options_match_gnu() {
    let program = if Path::new("/opt/homebrew/bin/gjoin").is_file() {
        "/opt/homebrew/bin/gjoin"
    } else {
        "join"
    };
    let version = std::process::Command::new(program)
        .arg("--version")
        .output()
        .unwrap();
    let is_gnu = String::from_utf8_lossy(&version.stdout).contains("GNU coreutils");
    #[cfg(target_os = "linux")]
    assert!(is_gnu, "GNU join is required by Linux CI");
    if !is_gnu {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let left = dir.path().join("left");
    let right = dir.path().join("right");
    for (a, b, flags) in [
        ("a 1\na 2\n", "a x\na y\n", ""),
        ("a 1\nb 2\n", "a x\nc y\n", "-a 1 -a 2 -e NA -o 0,1.2,2.2"),
        ("a 1\nb 2\n", "a x\nc y\n", "-v 1"),
        ("id name\na 1\n", "id value\na x\n", "--header -o auto"),
        ("A:1\nB:2\n", "a:x\nb:y\n", "-i -t : -o 1.2,0,2.2"),
    ] {
        std::fs::write(&left, a).unwrap();
        std::fs::write(&right, b).unwrap();
        let oracle = std::process::Command::new(program)
            .args(flags.split_whitespace())
            .arg(&left)
            .arg(&right)
            .output()
            .unwrap();
        assert!(
            oracle.status.success(),
            "{}",
            String::from_utf8_lossy(&oracle.stderr)
        );
        let mut bash = Bash::new();
        bash.fs()
            .write_file(Path::new("/left"), a.as_bytes())
            .await
            .unwrap();
        bash.fs()
            .write_file(Path::new("/right"), b.as_bytes())
            .await
            .unwrap();
        let actual = bash
            .exec(&format!("join {flags} /left /right"))
            .await
            .unwrap();
        assert_eq!(actual.exit_code, 0);
        assert_eq!(actual.stdout.as_bytes(), oracle.stdout, "flags={flags}");
        assert_eq!(actual.stderr.as_bytes(), oracle.stderr, "flags={flags}");
    }
}
