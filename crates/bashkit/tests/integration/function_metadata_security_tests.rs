//! Function source filenames share the function budget and lifecycle.
//! Probes use at most a few KiB, never a host-exhaustion workload.

use bashkit::{Bash, MemoryLimits};

fn bounded(bytes: usize) -> Bash {
    Bash::builder()
        .memory_limits(MemoryLimits::new().max_function_body_bytes(bytes))
        .build()
}

#[tokio::test]
async fn padded_source_filename_hits_function_budget() {
    let mut bash = bounded(256);
    bash.exec("echo 'f() { :; }' > /defs").await.unwrap();
    let path = format!("{}defs", "/".repeat(512));
    let error = bash
        .exec(&format!("source '{path}'"))
        .await
        .expect_err("source metadata must be budgeted before retaining a function");
    assert!(
        error.to_string().contains("function body byte limit"),
        "{error}"
    );
    assert_eq!(bash.exec("declare -F f").await.unwrap().exit_code, 1);
}

#[tokio::test]
async fn source_filename_bytes_aggregate_across_functions() {
    let mut bash = bounded(256);
    bash.exec("echo 'a() { :; }; b() { :; }; c() { :; }; d() { :; }' > /defs")
        .await
        .unwrap();
    let path = format!("{}defs", "/".repeat(96));
    let error = bash
        .exec(&format!(". '{path}'"))
        .await
        .expect_err("each retained function must charge its metadata");
    assert!(
        error.to_string().contains("function body byte limit"),
        "{error}"
    );
}

#[tokio::test]
async fn rejected_redefinition_keeps_original_function_and_filename() {
    let mut bash = bounded(256);
    bash.exec("echo 'f() { echo ${BASH_SOURCE[0]}; }' > /parent; source /parent; echo 'f() { :; }' > /defs").await.unwrap();
    let path = format!("{}defs", "/".repeat(512));
    assert!(bash.exec(&format!("source '{path}'")).await.is_err());
    assert_eq!(bash.exec("f").await.unwrap().stdout, "/parent\n");
}

#[tokio::test]
async fn unset_releases_function_bytes_and_metadata() {
    for unset in ["unset -f f", "unset f"] {
        let mut bash = bounded(100);
        bash.exec("echo 'f() { echo ${BASH_SOURCE[0]}; }' > /defs")
            .await
            .unwrap();
        let path = format!("{}defs", "/".repeat(32));
        for _ in 0..8 {
            let result = bash
                .exec(&format!("source '{path}'; f; {unset}"))
                .await
                .unwrap();
            assert_eq!(result.stdout, format!("{path}\n"));
        }
        assert_eq!(bash.exec("declare -F f").await.unwrap().exit_code, 1);
    }
}

#[tokio::test]
async fn function_filename_rolls_back_with_subshell_state() {
    for child in [
        "(source /child)",
        "x=$(source /child)",
        "source /child | cat",
        "source /child & wait",
        "chmod +x /child; /child",
    ] {
        let mut bash = bounded(256);
        bash.exec("echo 'f() { echo ${BASH_SOURCE[0]}; }' > /parent; echo 'f() { echo ${BASH_SOURCE[0]}; }; g() { :; }' > /child; source /parent").await.unwrap();
        let result = bash
            .exec(&format!("{child}; f; declare -F g"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "/parent\n", "{child}");
        assert_eq!(result.exit_code, 1, "{child}");
    }
}

#[tokio::test]
async fn redefinition_replaces_metadata_charge() {
    let mut bash = bounded(100);
    bash.exec("echo 'f() { echo ${BASH_SOURCE[0]}; }' > /defs")
        .await
        .unwrap();
    for slashes in [32, 1, 32, 1, 32] {
        let path = format!("{}defs", "/".repeat(slashes));
        let result = bash.exec(&format!("source '{path}'; f")).await.unwrap();
        assert_eq!(result.stdout, format!("{path}\n"));
    }
}

#[tokio::test]
async fn shell_restore_discards_previous_function_metadata() {
    let mut bash = bounded(256);
    bash.exec("f() { echo ${BASH_SOURCE[0]}; }").await.unwrap();
    let state = bash.shell_state();
    bash.exec("echo 'f() { echo ${BASH_SOURCE[0]}; }' > /defs; source /defs")
        .await
        .unwrap();
    bash.restore_shell_state(&state);
    assert_eq!(bash.exec("f").await.unwrap().stdout, "\n");
}

#[tokio::test]
async fn function_metadata_budget_boundary() {
    let definition = "f() { :; }";
    let path = "////defs";
    // One extra byte is the metadata map's function-name key.
    for (budget, accepted) in [
        (definition.len() + path.len() + 1, true),
        (definition.len() + path.len(), false),
    ] {
        let mut bash = bounded(budget);
        bash.exec(&format!("echo '{definition}' > /defs"))
            .await
            .unwrap();
        assert_eq!(
            bash.exec(&format!("source '{path}'")).await.is_ok(),
            accepted
        );
    }
}

#[tokio::test]
async fn source_filename_lifecycle_matches_real_bash() {
    let dir = tempfile::tempdir().unwrap();
    // Bash versions label functions defined in `-c` differently. Source the
    // replacement too, so the oracle checks the same filename contract.
    let script = r#"
echo 'f() { echo "${BASH_SOURCE[0]}"; }' > defs
echo 'f() { echo "${BASH_SOURCE[0]}"; }' > child
source ././defs
(source ./child)
f
x=$(source ./child)
f
source ./child | cat
f
source ./child & wait
f
chmod +x child
./child
f
unset -f f
echo 'f() { echo "${BASH_SOURCE[0]}"; }' > replacement
source ./replacement
f
"#;
    let oracle = std::process::Command::new("bash")
        .args(["--noprofile", "--norc", "-c", script])
        .current_dir(dir.path())
        .env_clear()
        .output()
        .unwrap();
    assert!(oracle.status.success());
    let result = bounded(1024).exec(script).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.as_bytes(), oracle.stdout);
    assert_eq!(result.stderr.as_bytes(), oracle.stderr);
}
