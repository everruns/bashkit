#![cfg(feature = "jq")]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use bashkit::testing::{fuzz_exec, fuzz_init};
use bashkit::{Bash, Error, ExecutionLimits, FileSystem, InMemoryFs, LimitExceeded};

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
async fn yaml_aliases_are_resolved_and_lossy_numbers_fail_closed() {
    let mut bash = Bash::new();
    let alias = bash
        .exec("printf 'base: &base\n  one: 1\ncopy: *base\n' | yq -o=json -I=0 '.'")
        .await
        .unwrap();
    assert_eq!(alias.exit_code, 0, "{}", alias.stderr);
    assert_eq!(
        alias.stdout,
        "{\"base\":{\"one\":1},\"copy\":{\"one\":1}}\n"
    );

    let duplicate = bash
        .exec("printf 'key: 1\nkey: 2\n' | yq '.'")
        .await
        .unwrap();
    assert_eq!(duplicate.exit_code, 1);
    assert!(duplicate.stderr.contains("duplicate entry with key"));

    for value in [".nan", ".inf", "-.inf"] {
        let result = bash
            .exec(&format!("printf 'value: {value}\n' | yq '.'"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 1, "{value}: {}", result.stderr);
        assert!(
            result.stderr.contains("non-finite YAML number"),
            "{value}: {}",
            result.stderr
        );
    }
}

#[tokio::test]
async fn rendered_output_obeys_execution_limit() {
    let mut bash = fuzz_bash(64);
    let result = bash.exec("yq -n '[range(0; 100)]'").await.unwrap();

    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.contains("output limit exceeded"));
}

#[tokio::test]
async fn json_depth_and_document_count_are_bounded() {
    let deep = format!("{}0{}", "[".repeat(101), "]".repeat(101));
    let documents = "0\n".repeat(4097);
    let mut bash = Bash::new();

    let deep_result = bash
        .exec(&format!("printf '%s' '{deep}' | yq -p=json '.'"))
        .await
        .unwrap();
    assert_eq!(deep_result.exit_code, 1);
    assert!(deep_result.stderr.contains("nesting too deep"));

    let document_result = bash
        .exec(&format!("yq -p=json '.' <<'JSON'\n{documents}JSON"))
        .await
        .unwrap();
    assert_eq!(document_result.exit_code, 1);
    assert!(
        document_result
            .stderr
            .contains("document limit exceeded (4096)")
    );
}

#[tokio::test]
async fn aggregate_input_budget_covers_all_input_files() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(Path::new("/tmp/a.yml"), b"value: 1\n")
        .await
        .unwrap();
    fs.write_file(Path::new("/tmp/b.yml"), b"value: 2\n")
        .await
        .unwrap();
    let limits = ExecutionLimits::new()
        .max_commands(20)
        .max_work_units(10_000)
        .max_aggregate_input_bytes(15);
    let mut bash = Bash::builder().fs(fs).limits(limits).build();

    let result = bash.exec("yq '.value' /tmp/a.yml /tmp/b.yml").await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
        ),
        "expected aggregate input exhaustion, got {result:?}"
    );
}

#[tokio::test]
async fn aggregate_document_limit_covers_all_input_files() {
    let fs = Arc::new(InMemoryFs::new());
    let documents = "---\n0\n".repeat(2049);
    fs.write_file(Path::new("/tmp/a.yml"), documents.as_bytes())
        .await
        .unwrap();
    fs.write_file(Path::new("/tmp/b.yml"), documents.as_bytes())
        .await
        .unwrap();
    let mut bash = Bash::builder().fs(fs).build();

    let result = bash.exec("yq '.' /tmp/a.yml /tmp/b.yml").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("document limit exceeded (4096)"));
}

#[tokio::test]
async fn jq_generator_consumes_shared_work_budget_through_yq() {
    let limits = ExecutionLimits::new()
        .max_work_units(500)
        .max_aggregate_input_bytes(100_000);
    let mut bash = Bash::builder().limits(limits).build();

    let result = bash.exec("yq -n 'range(0; 10000)'").await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
        ),
        "expected shared work exhaustion, got {result:?}"
    );
}

#[tokio::test]
async fn argument_errors_are_deterministic_and_bounded() {
    for (script, expected) in [
        ("yq --input-format toml", "unsupported format 'toml'"),
        ("yq --output-format auto", "unsupported format 'auto'"),
        ("yq --indent=10", "indentation must be between 0 and 9"),
        ("yq -Z", "unknown option '-Z'"),
        ("yq eval-all '.'", "eval-all is not supported"),
        ("yq -i '.'", "requires exactly one input file"),
        ("yq -i '.' -", "requires exactly one input file"),
        (
            "yq -ni '.' /tmp/data.yml",
            "requires exactly one input file",
        ),
    ] {
        let mut bash = Bash::new();
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 2, "{script}: {}", result.stderr);
        assert!(result.stdout.is_empty(), "{script}");
        assert!(
            result.stderr.contains(expected),
            "{script}: {}",
            result.stderr
        );
        assert!(result.stderr.len() <= 1024, "{script}");
    }
}

#[tokio::test]
async fn file_stdin_and_json_stream_order_is_preserved() {
    let mut bash = Bash::new();
    let result = bash
        .exec("printf 'id: 1\n' > /tmp/a.yml; printf '{\"id\":2}\n' > /tmp/b.json; printf 'id: 3\n' | yq -o=json -I=0 '.id' /tmp/a.yml - /tmp/b.json")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "1\n3\n2\n");
}

#[derive(Clone, Copy)]
struct CompatibilityCase {
    name: &'static str,
    input: &'static str,
    args: &'static [&'static str],
    stdout: &'static str,
    exit_code: i32,
}

// Locked against mikefarah/yq v4.53.3; MIKEFARAH_YQ enables the live oracle.
const COMPATIBILITY_CASES: &[CompatibilityCase] = &[
    CompatibilityCase {
        name: "identity",
        input: "enabled: true\nname: bashkit\n",
        args: &["-o=json", "-I=0", "."],
        stdout: "{\"enabled\":true,\"name\":\"bashkit\"}\n",
        exit_code: 0,
    },
    CompatibilityCase {
        name: "nested-select",
        input: "items:\n  - kind: fruit\n    name: apple\n  - kind: vegetable\n    name: carrot\n",
        args: &[
            "-o=json",
            "-I=0",
            ".items[] | select(.kind == \"fruit\") | .name",
        ],
        stdout: "\"apple\"\n",
        exit_code: 0,
    },
    CompatibilityCase {
        name: "map",
        input: "values: [1, 2, 3]\n",
        args: &["-o=json", "-I=0", ".values | map(. * 2)"],
        stdout: "[2,4,6]\n",
        exit_code: 0,
    },
    CompatibilityCase {
        name: "assignment",
        input: "count: 1\n",
        args: &["-o=json", "-I=0", ".count += 1"],
        stdout: "{\"count\":2}\n",
        exit_code: 0,
    },
    CompatibilityCase {
        name: "construction",
        input: "items:\n  - name: a\n  - name: b\n",
        args: &["-o=json", "-I=0", "{\"names\": [.items[].name]}"],
        stdout: "{\"names\":[\"a\",\"b\"]}\n",
        exit_code: 0,
    },
    CompatibilityCase {
        name: "raw-output",
        input: "name: bashkit\n",
        args: &["-r", ".name"],
        stdout: "bashkit\n",
        exit_code: 0,
    },
    CompatibilityCase {
        name: "json-input",
        input: "{\"nested\":{\"ok\":true}}\n",
        args: &["-p=json", "-o=json", "-I=0", ".nested"],
        stdout: "{\"ok\":true}\n",
        exit_code: 0,
    },
    CompatibilityCase {
        name: "null-input",
        input: "",
        args: &["-n", "-o=json", "-I=0", "{\"generated\": true}"],
        stdout: "{\"generated\":true}\n",
        exit_code: 0,
    },
    CompatibilityCase {
        name: "exit-status-false",
        input: "enabled: false\n",
        args: &["-e", "-o=json", "-I=0", ".enabled"],
        stdout: "false\n",
        exit_code: 1,
    },
    CompatibilityCase {
        name: "exit-status-null",
        input: "enabled: true\n",
        args: &["-e", "-o=json", "-I=0", ".missing"],
        stdout: "null\n",
        exit_code: 1,
    },
];

#[tokio::test]
async fn locked_mikefarah_compatible_corpus_and_optional_differential() {
    let real_yq = mikefarah_yq();
    for case in COMPATIBILITY_CASES {
        let command = case
            .args
            .iter()
            .map(|arg| shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" ");
        let script = format!("yq {command} <<'YAML'\n{}YAML", case.input);
        let mut bash = Bash::new();
        let embedded = bash.exec(&script).await.unwrap();
        assert_eq!(
            embedded.exit_code, case.exit_code,
            "{}: {}",
            case.name, embedded.stderr
        );
        assert_eq!(embedded.stdout, case.stdout, "{}", case.name);

        let Some(real_yq) = &real_yq else {
            continue;
        };
        let mut child = Command::new(real_yq)
            .args(case.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(case.input.as_bytes())
            .unwrap();
        let real = child.wait_with_output().unwrap();
        assert_eq!(
            real.status.code(),
            Some(case.exit_code),
            "{}: {}",
            case.name,
            String::from_utf8_lossy(&real.stderr)
        );
        assert_eq!(real.stdout, case.stdout.as_bytes(), "{}", case.name);
    }
}

fn mikefarah_yq() -> Option<String> {
    let candidate = std::env::var("MIKEFARAH_YQ").unwrap_or_else(|_| "yq".to_string());
    let version = Command::new(&candidate).arg("--version").output().ok()?;
    String::from_utf8_lossy(&version.stdout)
        .contains("mikefarah")
        .then_some(candidate)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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
