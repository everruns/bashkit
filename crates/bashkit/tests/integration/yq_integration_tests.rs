#![cfg(feature = "jq")]

use std::io::Write;
use std::process::{Command, Stdio};

use bashkit::testing::{fuzz_exec, fuzz_init};
use bashkit::{Bash, ExecutionLimits};

#[tokio::test]
async fn inplace_update_is_atomic_and_suppresses_stdout() {
    let mut bash = Bash::new();
    let result = bash
        .exec("printf 'name: old\\ncount: 1\\n' > /tmp/data.yml; chmod 600 /tmp/data.yml; yq -i '.name = \"new\" | .count += 1' /tmp/data.yml; cat /tmp/data.yml; stat -c 'mode:%a' /tmp/data.yml")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    // yq's JSON-value boundary sorts mapping keys deterministically (L-YQ-002).
    assert_eq!(result.stdout, "count: 2\nname: new\nmode:600\n");
}

#[tokio::test]
async fn inplace_failure_preserves_original_file() {
    let mut bash = Bash::new();
    let result = bash
        .exec("printf 'name: original\\n' > /tmp/data.yml; yq -i '.[[[[' /tmp/data.yml 2>/dev/null || true; cat /tmp/data.yml")
        .await
        .unwrap();

    assert_eq!(result.stdout, "name: original\n");
}

#[tokio::test]
async fn inplace_exit_status_failure_preserves_file_and_suppresses_output() {
    let mut bash = Bash::new();
    let result = bash
        .exec("printf 'name: original\\n' > /tmp/data.yml; yq -ei '.missing' /tmp/data.yml")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stdout.is_empty());

    let contents = bash.exec("cat /tmp/data.yml").await.unwrap();
    assert_eq!(contents.stdout, "name: original\n");
}

#[tokio::test]
async fn file_input_and_exit_status_cover_positive_and_negative_results() {
    let mut bash = Bash::new();
    let result = bash
        .exec("printf 'items: [1, 2, 3]\\n' > /tmp/data.yml; yq -e '.items[] | select(. > 2)' /tmp/data.yml; yq -e '.missing' /tmp/data.yml >/dev/null; printf 'status:%s\\n' $?")
        .await
        .unwrap();

    assert_eq!(result.stdout, "3\nstatus:1\n", "stderr={}", result.stderr);
}

#[tokio::test]
async fn deep_yaml_is_rejected_without_panicking_or_leaking_internals() {
    fuzz_init();
    let mut yaml = String::new();
    for depth in 0..110 {
        yaml.push_str(&"  ".repeat(depth));
        yaml.push_str("level:\n");
    }
    yaml.push_str(&"  ".repeat(110));
    yaml.push_str("value\n");
    let script = format!("yq '.' <<'YAML'\n{yaml}YAML");

    let mut bash = fuzz_bash(4096);
    fuzz_exec(
        &mut bash,
        &script,
        "yq_deep_yaml",
        &["serde_yaml_ng::", "Mapping {", "TaggedValue {"],
    )
    .await;
}

#[tokio::test]
async fn yaml_document_count_is_bounded() {
    let input = "---\n1\n".repeat(4097);
    let script = format!("yq '.' <<'YAML'\n{input}YAML");
    let mut bash = Bash::new();
    let result = bash.exec(&script).await.unwrap();

    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("document limit exceeded (4096)"));
}

#[tokio::test]
async fn yaml_tags_and_non_string_keys_fail_closed() {
    let mut bash = Bash::new();
    let tagged = bash
        .exec("printf 'value: !Ref thing\\n' | yq '.'")
        .await
        .unwrap();
    assert_eq!(tagged.exit_code, 1);
    assert!(tagged.stderr.contains("custom YAML tags are not supported"));

    let keyed = bash.exec("printf '1: value\\n' | yq '.'").await.unwrap();
    assert_eq!(keyed.exit_code, 1);
    assert!(keyed.stderr.contains("mapping keys must be strings"));
}

#[tokio::test]
async fn rendered_output_obeys_execution_limit() {
    let mut bash = fuzz_bash(64);
    let result = bash.exec("yq -n '[range(0; 100)]'").await.unwrap();

    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.contains("output limit exceeded"));
}

#[tokio::test]
async fn representative_expression_matches_mikefarah_yq_when_available() {
    let Ok(version) = Command::new("yq").arg("--version").output() else {
        return;
    };
    let version = String::from_utf8_lossy(&version.stdout);
    if !version.contains("mikefarah") {
        return;
    }

    let input = "items:\n  - name: apple\n    kind: fruit\n  - name: carrot\n    kind: vegetable\n";
    let args = [
        "-o=json",
        "-I=0",
        ".items | map(select(.kind == \"fruit\") | .name)",
    ];
    let mut child = Command::new("yq")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let real = child.wait_with_output().unwrap();
    assert!(real.status.success());

    let mut bash = Bash::new();
    let script = format!("yq -o=json -I=0 '{}' <<'YAML'\n{input}YAML", args[2]);
    let embedded = bash.exec(&script).await.unwrap();
    assert_eq!(embedded.exit_code, 0, "{}", embedded.stderr);
    assert_eq!(embedded.stdout.as_bytes(), real.stdout);
}

fn fuzz_bash(max_stdout: usize) -> Bash {
    Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(50)
                .max_stdout_bytes(max_stdout)
                .max_stderr_bytes(4096)
                .timeout(std::time::Duration::from_secs(2)),
        )
        .build()
}
