use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
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
fn harness_example_does_not_follow_preexisting_provider_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let harness_dir = temp.path().join("harness");
    let work_dir = temp.path().join("work");
    let providers_dir = work_dir.join(".harness/providers");
    let target_file = temp.path().join("target.txt");
    let fake_bashkit = temp.path().join("fake-bashkit");

    fs::create_dir_all(harness_dir.join("bin")).unwrap();
    fs::create_dir_all(&providers_dir).unwrap();
    fs::write(&target_file, "victim data").unwrap();
    let mut perms = fs::metadata(&target_file).unwrap().permissions();
    perms.set_mode(0o600);
    fs::set_permissions(&target_file, perms).unwrap();
    symlink(&target_file, providers_dir.join("openai")).unwrap();

    write_executable(
        &fake_bashkit,
        "#!/usr/bin/env bash\nset -euo pipefail\nprintf '%s\n' ok\n",
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
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(fs::read_to_string(&target_file).unwrap(), "victim data");
    assert_eq!(
        fs::metadata(&target_file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let override_path = providers_dir.join("openai");
    assert!(
        !fs::symlink_metadata(&override_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::read_to_string(override_path)
            .unwrap()
            .contains("exec /harness/plugins/openai/providers/openai \"$@\"")
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

#[test]
fn harness_example_skips_when_openai_quota_is_exhausted() {
    let temp = tempfile::tempdir().unwrap();
    let harness_dir = temp.path().join("harness");
    let work_dir = temp.path().join("work");
    let fake_bashkit = temp.path().join("fake-bashkit");

    fs::create_dir_all(harness_dir.join("bin")).unwrap();
    fs::create_dir_all(&work_dir).unwrap();

    write_executable(
        &fake_bashkit,
        "#!/usr/bin/env bash\nset -euo pipefail\ncat <<'EOF'\nsession: /work/.harness/sessions/20260423-030213-1/~}\nerror: openai API error: You exceeded your current quota, please check your plan and billing details.\nprovider: openai\nmodel: gpt-4o\nEOF\nexit 0\n",
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
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Skipping example"),
        "stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}
