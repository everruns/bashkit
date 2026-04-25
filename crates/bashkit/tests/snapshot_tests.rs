//! Tests for VFS snapshot/restore and shell state snapshot/restore

use bashkit::{
    Bash, ExecutionLimits, FileSystem, InMemoryFs, MemoryLimits, SessionLimits, Snapshot,
    SnapshotOptions,
};
use std::path::Path;
use std::sync::Arc;

// ==================== VFS snapshot/restore ====================

#[tokio::test]
async fn vfs_snapshot_restores_file_content() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(Path::new("/tmp/test.txt"), b"original")
        .await
        .unwrap();

    let snapshot = fs.snapshot();

    // Modify
    fs.write_file(Path::new("/tmp/test.txt"), b"modified")
        .await
        .unwrap();

    // Restore
    fs.restore(&snapshot);

    let content = fs.read_file(Path::new("/tmp/test.txt")).await.unwrap();
    assert_eq!(content, b"original");
}

#[tokio::test]
async fn vfs_snapshot_removes_new_files() {
    let fs = Arc::new(InMemoryFs::new());
    let snapshot = fs.snapshot();

    // Create new file
    fs.write_file(Path::new("/tmp/new.txt"), b"new file")
        .await
        .unwrap();
    assert!(fs.exists(Path::new("/tmp/new.txt")).await.unwrap());

    // Restore
    fs.restore(&snapshot);
    assert!(!fs.exists(Path::new("/tmp/new.txt")).await.unwrap());
}

#[tokio::test]
async fn vfs_snapshot_restores_deleted_files() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(Path::new("/tmp/keep.txt"), b"keep me")
        .await
        .unwrap();

    let snapshot = fs.snapshot();

    // Delete
    fs.remove(Path::new("/tmp/keep.txt"), false).await.unwrap();
    assert!(!fs.exists(Path::new("/tmp/keep.txt")).await.unwrap());

    // Restore
    fs.restore(&snapshot);
    let content = fs.read_file(Path::new("/tmp/keep.txt")).await.unwrap();
    assert_eq!(content, b"keep me");
}

#[tokio::test]
async fn vfs_snapshot_preserves_directories() {
    let fs = Arc::new(InMemoryFs::new());
    fs.mkdir(Path::new("/data"), false).await.unwrap();
    fs.mkdir(Path::new("/data/sub"), false).await.unwrap();
    fs.write_file(Path::new("/data/sub/file.txt"), b"content")
        .await
        .unwrap();

    let snapshot = fs.snapshot();

    fs.remove(Path::new("/data"), true).await.unwrap();
    assert!(!fs.exists(Path::new("/data")).await.unwrap());

    fs.restore(&snapshot);
    assert!(fs.exists(Path::new("/data/sub")).await.unwrap());
    let content = fs.read_file(Path::new("/data/sub/file.txt")).await.unwrap();
    assert_eq!(content, b"content");
}

#[tokio::test]
async fn vfs_snapshot_serialization_roundtrip() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(Path::new("/tmp/data.txt"), b"serialize me")
        .await
        .unwrap();

    let snapshot = fs.snapshot();
    let json = serde_json::to_string(&snapshot).unwrap();
    let restored: bashkit::VfsSnapshot = serde_json::from_str(&json).unwrap();

    let fs2 = Arc::new(InMemoryFs::new());
    fs2.restore(&restored);

    let content = fs2.read_file(Path::new("/tmp/data.txt")).await.unwrap();
    assert_eq!(content, b"serialize me");
}

// ==================== Shell state snapshot/restore ====================

#[tokio::test]
async fn shell_state_restores_variables() {
    let mut bash = Bash::new();
    bash.exec("x=42; y=hello").await.unwrap();

    let state = bash.shell_state();

    bash.exec("x=99; y=world").await.unwrap();
    bash.restore_shell_state(&state);

    let result = bash.exec("echo $x $y").await.unwrap();
    assert_eq!(result.stdout, "42 hello\n");
}

#[tokio::test]
async fn shell_state_restores_cwd() {
    let mut bash = Bash::new();
    bash.exec("mkdir -p /data && cd /data").await.unwrap();

    let state = bash.shell_state();

    bash.exec("cd /tmp").await.unwrap();
    bash.restore_shell_state(&state);

    let result = bash.exec("pwd").await.unwrap();
    assert_eq!(result.stdout, "/data\n");
}

#[tokio::test]
async fn shell_state_restores_aliases() {
    let mut bash = Bash::new();
    bash.exec("alias ll='ls -la'").await.unwrap();

    let state = bash.shell_state();

    bash.exec("unalias ll 2>/dev/null; alias ll='ls'")
        .await
        .unwrap();
    bash.restore_shell_state(&state);

    // Verify alias is restored by checking alias command
    let result = bash.exec("alias ll").await.unwrap();
    assert!(result.stdout.contains("ls -la"));
}

#[tokio::test]
async fn shell_state_serialization_roundtrip() {
    let mut bash = Bash::new();
    bash.exec("x=42").await.unwrap();

    let state = bash.shell_state();
    let json = serde_json::to_string(&state).unwrap();
    let restored: bashkit::ShellState = serde_json::from_str(&json).unwrap();

    let mut bash2 = Bash::new();
    bash2.restore_shell_state(&restored);

    let result = bash2.exec("echo $x").await.unwrap();
    assert_eq!(result.stdout, "42\n");
}

// ==================== Combined VFS + shell state ====================

#[tokio::test]
async fn combined_snapshot_restore_multi_turn() {
    let fs = Arc::new(InMemoryFs::new());
    let mut bash = Bash::builder().fs(fs.clone()).build();

    // Turn 1: Set up files and variables
    bash.exec("echo 'config' > /tmp/config.txt && count=1")
        .await
        .unwrap();

    let vfs_snap = fs.snapshot();
    let shell_snap = bash.shell_state();

    // Turn 2: Make changes
    bash.exec("echo 'modified' > /tmp/config.txt && count=5 && echo 'new' > /tmp/new.txt")
        .await
        .unwrap();

    // Rollback to turn 1
    fs.restore(&vfs_snap);
    bash.restore_shell_state(&shell_snap);

    let result = bash
        .exec("cat /tmp/config.txt && echo $count")
        .await
        .unwrap();
    assert_eq!(result.stdout, "config\n1\n");

    // New file should be gone
    let result = bash
        .exec("test -f /tmp/new.txt && echo exists || echo gone")
        .await
        .unwrap();
    assert_eq!(result.stdout, "gone\n");
}

// ==================== Shell options snapshot/restore ====================

#[tokio::test]
async fn shell_options_survive_snapshot_roundtrip() {
    let mut bash = Bash::new();

    // Set options via `set` builtin. Options live in SHOPT_* variables which
    // are included in the variables snapshot (no more split brain with a
    // separate ShellOptions struct).
    bash.exec("set -e; set -o pipefail").await.unwrap();

    let state = bash.shell_state();

    // Options should be present in snapshotted variables
    assert_eq!(
        state.variables.get("SHOPT_e").map(|s| s.as_str()),
        Some("1")
    );
    assert_eq!(
        state.variables.get("SHOPT_pipefail").map(|s| s.as_str()),
        Some("1")
    );

    // Serialize → deserialize to prove options survive JSON roundtrip
    let json = serde_json::to_string(&state).unwrap();
    let restored: bashkit::ShellState = serde_json::from_str(&json).unwrap();
    assert_eq!(
        restored.variables.get("SHOPT_e").map(|s| s.as_str()),
        Some("1")
    );
    assert_eq!(
        restored.variables.get("SHOPT_pipefail").map(|s| s.as_str()),
        Some("1")
    );

    // Restore into a fresh interpreter and verify options are active
    let mut bash2 = Bash::new();
    bash2.restore_shell_state(&restored);

    // `set` options (SHOPT_e, SHOPT_pipefail) are transient — they are
    // cleared by reset_transient_state() between exec() calls (TM-ISO-023).
    // Verify the snapshot restored them correctly before the next exec().
    let state2 = bash2.shell_state();
    assert_eq!(
        state2.variables.get("SHOPT_e").map(|s| s.as_str()),
        Some("1"),
        "errexit should survive snapshot/restore roundtrip"
    );
    assert_eq!(
        state2.variables.get("SHOPT_pipefail").map(|s| s.as_str()),
        Some("1"),
        "pipefail should survive snapshot/restore roundtrip"
    );
}

// ==================== Byte-level snapshot / from_snapshot ====================

#[tokio::test]
async fn snapshot_to_bytes_and_restore() {
    let mut bash = Bash::new();
    bash.exec("x=42; mkdir /tmp/work; echo 'data' > /tmp/work/file.txt")
        .await
        .unwrap();

    let bytes = bash.snapshot().unwrap();
    assert!(!bytes.is_empty());

    let mut bash2 = Bash::from_snapshot(&bytes).unwrap();

    // Verify shell state
    let r = bash2.exec("echo $x").await.unwrap();
    assert_eq!(r.stdout.trim(), "42");

    // Verify VFS contents
    let r = bash2.exec("cat /tmp/work/file.txt").await.unwrap();
    assert_eq!(r.stdout.trim(), "data");
}

#[tokio::test]
async fn snapshot_preserves_arrays() {
    let mut bash = Bash::new();
    bash.exec("arr=(one two three); declare -A map=([k1]=v1 [k2]=v2)")
        .await
        .unwrap();

    let bytes = bash.snapshot().unwrap();
    let mut bash2 = Bash::from_snapshot(&bytes).unwrap();

    let r = bash2.exec("echo ${arr[1]}").await.unwrap();
    assert_eq!(r.stdout.trim(), "two");

    let r = bash2.exec("echo ${map[k2]}").await.unwrap();
    assert_eq!(r.stdout.trim(), "v2");
}

#[tokio::test]
async fn snapshot_preserves_env() {
    let mut bash = Bash::new();
    bash.exec("export MY_VAR=hello").await.unwrap();

    let bytes = bash.snapshot().unwrap();
    let mut bash2 = Bash::from_snapshot(&bytes).unwrap();

    let r = bash2.exec("echo $MY_VAR").await.unwrap();
    assert_eq!(r.stdout.trim(), "hello");
}

#[tokio::test]
async fn snapshot_preserves_cwd() {
    let mut bash = Bash::new();
    bash.exec("mkdir -p /project && cd /project").await.unwrap();

    let bytes = bash.snapshot().unwrap();
    let mut bash2 = Bash::from_snapshot(&bytes).unwrap();

    let r = bash2.exec("pwd").await.unwrap();
    assert_eq!(r.stdout.trim(), "/project");
}

#[tokio::test]
async fn snapshot_preserves_functions() {
    let mut bash = Bash::new();
    bash.exec("greet() { echo \"hi $1\"; }").await.unwrap();

    let bytes = bash.snapshot().unwrap();
    let mut bash2 = Bash::from_snapshot(&bytes).unwrap();

    let r = bash2.exec("greet world").await.unwrap();
    assert_eq!(r.stdout.trim(), "hi world");
}

#[tokio::test]
async fn snapshot_restores_functions_from_source_when_ast_missing() {
    let mut bash = Bash::new();
    bash.exec("greet() { echo \"hi $1\"; }").await.unwrap();

    let bytes = bash.snapshot().unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&bytes[32..]).unwrap();
    json["shell"]["functions"]["greet"] = serde_json::json!({
        "source": "greet() { echo \"hi $1\"; }"
    });

    let rewritten: Snapshot = serde_json::from_value(json).unwrap();
    let bytes = rewritten.to_bytes().unwrap();
    let mut restored = Bash::from_snapshot(&bytes).unwrap();

    let result = restored
        .exec("type greet >/dev/null 2>&1; echo $?; greet world")
        .await
        .unwrap();
    assert_eq!(result.stdout, "0\nhi world\n");
}

#[tokio::test]
async fn snapshot_restores_legacy_function_shape_without_wrapper() {
    let mut bash = Bash::new();
    bash.exec("greet() { echo \"hi $1\"; }").await.unwrap();

    let bytes = bash.snapshot().unwrap();
    let parsed = Snapshot::from_bytes(&bytes).unwrap();
    let legacy_func = serde_json::to_value(parsed.shell.functions.get("greet").unwrap()).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&bytes[32..]).unwrap();
    json["shell"]["functions"]["greet"] = legacy_func;

    let rewritten: Snapshot = serde_json::from_value(json).unwrap();
    let bytes = rewritten.to_bytes().unwrap();
    let mut restored = Bash::from_snapshot(&bytes).unwrap();

    let result = restored
        .exec("type greet >/dev/null 2>&1; echo $?; greet world")
        .await
        .unwrap();
    assert_eq!(result.stdout, "0\nhi world\n");
}

#[tokio::test]
async fn snapshot_without_functions_skips_function_restore() {
    let mut bash = Bash::new();
    bash.exec("greet() { echo \"hi $1\"; }; answer=42")
        .await
        .unwrap();

    let bytes = bash
        .snapshot_with_options(SnapshotOptions {
            exclude_filesystem: true,
            exclude_functions: true,
        })
        .unwrap();
    let snap = Snapshot::from_bytes(&bytes).unwrap();
    assert!(snap.shell.functions.is_empty());

    let mut restored = Bash::from_snapshot(&bytes).unwrap();
    let result = restored
        .exec("echo $answer; type greet >/dev/null 2>&1; echo $?")
        .await
        .unwrap();
    assert_eq!(result.stdout, "42\n1\n");
}

#[tokio::test]
async fn snapshot_restore_enforces_function_limits() {
    let mut src = Bash::new();
    src.exec("a() { echo a; }; b() { echo b; }").await.unwrap();
    let bytes = src.snapshot().unwrap();

    let limits = MemoryLimits::new().max_function_count(1);
    let mut restored = Bash::builder().memory_limits(limits).build();
    restored.restore_snapshot(&bytes).unwrap();

    let result = restored
        .exec("type a >/dev/null 2>&1; echo $?; type b >/dev/null 2>&1; echo $?")
        .await
        .unwrap();
    assert_eq!(result.stdout, "0\n1\n");
}

#[tokio::test]
async fn snapshot_restore_enforces_parser_limits() {
    let mut src = Bash::new();
    let mut deep_body = String::from("deep() { ");
    for _ in 0..20 {
        deep_body.push_str("if true; then ");
    }
    deep_body.push_str("echo ok; ");
    for _ in 0..20 {
        deep_body.push_str("fi; ");
    }
    deep_body.push('}');
    src.exec(&format!("{deep_body}; shallow() {{ echo ok; }}"))
        .await
        .unwrap();
    let bytes = src.snapshot().unwrap();

    let limits = ExecutionLimits::new().max_ast_depth(5);
    let mut restored = Bash::builder().limits(limits).build();
    restored.restore_snapshot(&bytes).unwrap();

    let result = restored
        .exec("type shallow >/dev/null 2>&1; echo $?; type deep >/dev/null 2>&1; echo $?")
        .await
        .unwrap();
    assert_eq!(result.stdout, "0\n1\n");
}

#[tokio::test]
async fn snapshot_restore_into_existing_instance() {
    let mut bash = Bash::new();
    bash.exec("x=42; echo 'data' > /tmp/saved.txt")
        .await
        .unwrap();

    let bytes = bash.snapshot().unwrap();

    // Make changes
    bash.exec("x=99; echo 'changed' > /tmp/saved.txt")
        .await
        .unwrap();

    // Restore into same instance
    bash.restore_snapshot(&bytes).unwrap();

    let r = bash.exec("echo $x").await.unwrap();
    assert_eq!(r.stdout.trim(), "42");

    let r = bash.exec("cat /tmp/saved.txt").await.unwrap();
    assert_eq!(r.stdout.trim(), "data");
}

#[tokio::test]
async fn snapshot_without_filesystem_preserves_shell_only() {
    let mut bash = Bash::new();
    bash.exec("x=42; greet() { echo \"hi $1\"; }; echo 'saved' > /tmp/state.txt")
        .await
        .unwrap();

    let bytes = bash
        .snapshot_with_options(SnapshotOptions {
            exclude_filesystem: true,
            exclude_functions: false,
        })
        .unwrap();

    bash.exec("x=99; echo 'changed' > /tmp/state.txt")
        .await
        .unwrap();

    bash.restore_snapshot(&bytes).unwrap();

    let r = bash.exec("echo $x").await.unwrap();
    assert_eq!(r.stdout.trim(), "42");

    let r = bash.exec("greet world").await.unwrap();
    assert_eq!(r.stdout.trim(), "hi world");

    let r = bash.exec("cat /tmp/state.txt").await.unwrap();
    assert_eq!(r.stdout.trim(), "changed");
}

#[tokio::test]
async fn snapshot_struct_serialization() {
    let mut bash = Bash::new();
    bash.exec("greeting='hello world'").await.unwrap();

    let bytes = bash.snapshot().unwrap();
    let snap = Snapshot::from_bytes(&bytes).unwrap();

    assert_eq!(snap.version, 1);
    assert_eq!(
        snap.shell.variables.get("greeting").map(|s| s.as_str()),
        Some("hello world")
    );

    // Re-serialize and verify roundtrip
    let bytes2 = snap.to_bytes().unwrap();
    let snap2 = Snapshot::from_bytes(&bytes2).unwrap();
    assert_eq!(
        snap2.shell.variables.get("greeting"),
        snap.shell.variables.get("greeting")
    );
}

#[tokio::test]
async fn snapshot_invalid_data_returns_error() {
    let result = Bash::from_snapshot(b"not valid json");
    assert!(result.is_err());
}

#[tokio::test]
async fn snapshot_session_counters_transferred() {
    let mut bash = Bash::new();
    // Run some commands to increment session counters
    bash.exec("echo 1; echo 2; echo 3").await.unwrap();
    bash.exec("echo 4").await.unwrap();

    let bytes = bash.snapshot().unwrap();
    let snap = Snapshot::from_bytes(&bytes).unwrap();

    // Session counters should be > 0
    assert!(snap.session_commands > 0);
    assert!(snap.session_exec_calls > 0);
}

#[tokio::test]
async fn snapshot_restore_does_not_reset_session_exec_limit_with_tampered_counter() {
    let session_limits = SessionLimits::new().max_exec_calls(2);
    let mut bash = Bash::builder().session_limits(session_limits).build();
    bash.exec("echo first").await.unwrap();
    let bytes = bash.snapshot().unwrap();

    let mut tampered_json: serde_json::Value = serde_json::from_slice(&bytes[32..]).unwrap();
    tampered_json["session_exec_calls"] = serde_json::json!(0);
    let tampered_snapshot: Snapshot = serde_json::from_value(tampered_json).unwrap();
    let tampered_bytes = tampered_snapshot.to_bytes().unwrap();

    bash.restore_snapshot(&tampered_bytes).unwrap();
    bash.exec("echo second").await.unwrap();
    let third = bash.exec("echo third").await;
    assert!(
        third.is_err(),
        "session exec-call budget must remain monotonic across restore"
    );
}

#[tokio::test]
async fn snapshot_restore_rejects_tampered_shell_state_that_exceeds_memory_limits() {
    let mut src = Bash::new();
    src.exec("x=ok").await.unwrap();
    let bytes = src.snapshot().unwrap();

    let mut tampered_json: serde_json::Value = serde_json::from_slice(&bytes[32..]).unwrap();
    let oversized_vars = serde_json::json!({
        "a": "1",
        "b": "2",
        "c": "3"
    });
    tampered_json["shell"]["variables"] = oversized_vars;

    let tampered_snapshot: Snapshot = serde_json::from_value(tampered_json).unwrap();
    let tampered_bytes = tampered_snapshot.to_bytes().unwrap();

    let limits = MemoryLimits::new().max_variable_count(2);
    let mut restored = Bash::builder().memory_limits(limits).build();
    let result = restored.restore_snapshot(&tampered_bytes);
    assert!(
        result.is_err(),
        "restore must reject shell state above configured memory limits"
    );
}

// ==================== Integrity verification (Issue #977) ====================

#[tokio::test]
async fn snapshot_tampered_bytes_rejected() {
    let mut bash = Bash::new();
    bash.exec("x=42").await.unwrap();

    let mut bytes = bash.snapshot().unwrap();

    // Tamper with a byte in the JSON payload (after the 32-byte digest)
    if bytes.len() > 40 {
        bytes[40] ^= 0xFF;
    }

    let result = Bash::from_snapshot(&bytes);
    assert!(result.is_err());
    let err_msg = result.err().expect("should be error").to_string();
    assert!(
        err_msg.contains("integrity"),
        "Error should mention integrity: {}",
        err_msg
    );
}

#[tokio::test]
async fn snapshot_truncated_rejected() {
    let result = Bash::from_snapshot(&[0u8; 10]);
    assert!(result.is_err());
}

#[tokio::test]
async fn snapshot_modified_digest_rejected() {
    let mut bash = Bash::new();
    bash.exec("x=42").await.unwrap();

    let mut bytes = bash.snapshot().unwrap();

    // Modify the digest (first 32 bytes)
    bytes[0] ^= 0xFF;

    let result = Bash::from_snapshot(&bytes);
    assert!(result.is_err());
}

// ==================== Limits preserved after restore (Issue #978) ====================

#[tokio::test]
async fn restore_snapshot_preserves_limits() {
    use bashkit::ExecutionLimits;

    let limits = ExecutionLimits::new().max_commands(5);

    // Create a bash instance with strict command limit
    let mut bash = Bash::builder().limits(limits.clone()).build();
    bash.exec("x=42").await.unwrap();
    let bytes = bash.snapshot().unwrap();

    // Create a new instance with same limits, then restore snapshot state
    let mut restored = Bash::builder().limits(limits).build();
    restored.restore_snapshot(&bytes).unwrap();

    // Verify state was restored (simple command within limit)
    let r = restored.exec("echo $x").await.unwrap();
    assert_eq!(r.stdout.trim(), "42");

    // Verify limits are still enforced — many commands should hit the limit
    let r = restored
        .exec("echo 1; echo 2; echo 3; echo 4; echo 5; echo 6; echo 7; echo 8; echo 9; echo 10")
        .await;
    // Should hit the command limit and return an error
    assert!(r.is_err(), "Should hit max_commands limit after restore");
}

// ==================== Keyed snapshot integrity (Issue #1167) ====================

#[tokio::test]
async fn keyed_snapshot_roundtrip() {
    let key = b"my-secret-key-for-hmac";
    let mut bash = Bash::new();
    bash.exec("MY_VAR=hello").await.unwrap();
    let bytes = bash.snapshot_to_bytes_keyed(key).unwrap();

    let mut restored = Bash::new();
    restored.restore_snapshot_keyed(&bytes, key).unwrap();
    let r = restored.exec("echo $MY_VAR").await.unwrap();
    assert_eq!(r.stdout.trim(), "hello");
}

#[tokio::test]
async fn keyed_snapshot_wrong_key_rejected() {
    let key = b"correct-key";
    let wrong_key = b"wrong-key";
    let mut bash = Bash::new();
    bash.exec("x=42").await.unwrap();
    let bytes = bash.snapshot_to_bytes_keyed(key).unwrap();

    let result = Bash::from_snapshot_keyed(&bytes, wrong_key);
    assert!(result.is_err());
    let err = result.err().unwrap().to_string();
    assert!(
        err.contains("HMAC mismatch"),
        "Expected HMAC error: {}",
        err
    );
}

#[tokio::test]
async fn keyed_snapshot_tampered_rejected() {
    let key = b"secret";
    let mut bash = Bash::new();
    bash.exec("x=42").await.unwrap();
    let mut bytes = bash.snapshot_to_bytes_keyed(key).unwrap();

    // Tamper with payload
    if bytes.len() > 40 {
        bytes[40] ^= 0xFF;
    }

    let result = Bash::from_snapshot_keyed(&bytes, key);
    assert!(result.is_err());
}

/// Regression for #1421: HMAC verification must not short-circuit on a
/// matching prefix. A forged digest that agrees with the real digest in
/// every byte except the last must still be rejected — this exercises
/// the `Mac::verify_slice` constant-time path that replaced raw `==`.
#[tokio::test]
async fn keyed_snapshot_matching_prefix_digest_rejected() {
    let key = b"secret-key";
    let mut bash = Bash::new();
    bash.exec("x=1").await.unwrap();
    let mut bytes = bash.snapshot_to_bytes_keyed(key).unwrap();

    // Flip only the final byte of the 32-byte HMAC digest. The first 31
    // bytes still match the real digest; a short-circuiting compare could
    // leak that position via timing. Verification must still reject it.
    assert!(bytes.len() >= 32, "snapshot must contain a 32-byte digest");
    bytes[31] ^= 0xFF;

    let err = match Bash::from_snapshot_keyed(&bytes, key) {
        Ok(_) => panic!("expected verification to fail"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("HMAC mismatch"), "Expected HMAC error: {err}");
}

/// Regression for #1421: a digest that differs only in its first byte
/// must also be rejected. Symmetrical to the matching-prefix case — the
/// verifier should not depend on byte position.
#[tokio::test]
async fn keyed_snapshot_matching_suffix_digest_rejected() {
    let key = b"secret-key";
    let mut bash = Bash::new();
    bash.exec("x=1").await.unwrap();
    let mut bytes = bash.snapshot_to_bytes_keyed(key).unwrap();

    assert!(bytes.len() >= 32, "snapshot must contain a 32-byte digest");
    bytes[0] ^= 0xFF;

    let err = match Bash::from_snapshot_keyed(&bytes, key) {
        Ok(_) => panic!("expected verification to fail"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("HMAC mismatch"), "Expected HMAC error: {err}");
}
