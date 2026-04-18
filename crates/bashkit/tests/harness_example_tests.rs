use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn write_executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn example_script() -> String {
    format!(
        "{}/../../examples/harness-openai-joke.sh",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn harness_example_installs_non_streaming_openai_override() {
    let temp = tempfile::tempdir().unwrap();
    let harness_dir = temp.path().join("harness");
    let work_dir = temp.path().join("work");
    let fake_bashkit = temp.path().join("fake-bashkit");
    let stdout_file = temp.path().join("stdout.txt");

    fs::create_dir_all(harness_dir.join("bin")).unwrap();
    fs::create_dir_all(&work_dir).unwrap();

    write_executable(
        &fake_bashkit,
        &format!(
            "#!/usr/bin/env bash\nset -euo pipefail\nprintf '%s\\n' \"${{FAKE_BASHKIT_STDOUT:-ok}}\"\nprintf '%s' \"${{FAKE_BASHKIT_STDOUT:-ok}}\" > \"{}\"\nexit \"${{FAKE_BASHKIT_EXIT:-0}}\"\n",
            stdout_file.display()
        ),
    );

    let output = Command::new("bash")
        .arg(example_script())
        .env("BASHKIT", &fake_bashkit)
        .env("HARNESS_DIR", &harness_dir)
        .env("WORK_DIR", &work_dir)
        .env("OPENAI_API_KEY", "dummy")
        .env("FAKE_BASHKIT_STDOUT", "ok")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let override_path = work_dir.join(".harness/providers/openai");
    assert!(override_path.exists());
    let override_body = fs::read_to_string(override_path).unwrap();
    assert!(override_body.contains("exec /harness/plugins/openai/providers/openai \"$@\""));
    assert!(
        !override_body.contains("--stream"),
        "override should suppress harness streaming autodetection"
    );
}

#[test]
fn harness_example_fails_when_bashkit_prints_error_output() {
    let temp = tempfile::tempdir().unwrap();
    let harness_dir = temp.path().join("harness");
    let work_dir = temp.path().join("work");
    let fake_bashkit = temp.path().join("fake-bashkit");

    fs::create_dir_all(harness_dir.join("bin")).unwrap();
    fs::create_dir_all(&work_dir).unwrap();

    write_executable(
        &fake_bashkit,
        "#!/usr/bin/env bash\nset -euo pipefail\nprintf '%s\\n' 'error: hook pipeline failed'\nexit 0\n",
    );

    let output = Command::new("bash")
        .arg(example_script())
        .env("BASHKIT", &fake_bashkit)
        .env("HARNESS_DIR", &harness_dir)
        .env("WORK_DIR", &work_dir)
        .env("OPENAI_API_KEY", "dummy")
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "script should fail on error-prefixed stdout"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("error: hook pipeline failed"),
        "stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}
