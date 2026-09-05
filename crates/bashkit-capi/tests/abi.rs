use bashkit_capi::*;
use std::ptr;

fn bytes(value: &[u8]) -> BashkitBytes {
    BashkitBytes {
        ptr: value.as_ptr(),
        len: value.len(),
    }
}

unsafe fn borrowed(value: BashkitBytes) -> Vec<u8> {
    if value.len == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(value.ptr, value.len) }.to_vec()
    }
}

unsafe fn error_message(error: *mut BashkitError) -> String {
    let message = unsafe { borrowed(bashkit_error_message(error)) };
    unsafe { bashkit_error_free(error) };
    String::from_utf8(message).unwrap()
}

#[test]
fn creates_executes_and_preserves_shell_exit_status() {
    unsafe {
        assert_eq!(bashkit_abi_version(), BASHKIT_ABI_VERSION_1);
        assert!(!borrowed(bashkit_version()).is_empty());
        let capabilities: serde_json::Value =
            serde_json::from_slice(&borrowed(bashkit_capabilities_json())).unwrap();
        assert_eq!(capabilities["abi"], 1);

        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_default(&mut bash, &mut error),
            BashkitStatus::Ok
        );
        assert!(!bash.is_null());
        assert!(error.is_null());

        let mut result = ptr::null_mut();
        let script = b"printf 'hello'; printf 'warning' >&2; exit 7";
        assert_eq!(
            bashkit_execute(bash, bytes(script), &mut result, &mut error),
            BashkitStatus::Ok
        );
        assert_eq!(bashkit_result_exit_code(result), 7);
        assert_eq!(borrowed(bashkit_result_stdout(result)), b"hello");
        assert_eq!(borrowed(bashkit_result_stderr(result)), b"warning");

        bashkit_result_free(result);
        bashkit_free(bash);
    }
}

#[test]
fn execute_returns_exact_binary_stdout() {
    unsafe {
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_default(&mut bash, &mut error),
            BashkitStatus::Ok
        );
        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(
                bash,
                bytes(b"printf 'AAH//g==' | base64 -d | cat"),
                &mut result,
                &mut error,
            ),
            BashkitStatus::Ok
        );
        assert_eq!(
            borrowed(bashkit_result_stdout(result)),
            [0x00, 0x01, 0xff, 0xfe]
        );
        bashkit_result_free(result);
        bashkit_free(bash);
    }
}

#[test]
fn configures_environment_files_limits_and_final_environment() {
    unsafe {
        let config = br#"{
            "schema_version": 1,
            "cwd": "/workspace",
            "env": {"GREETING": "hello"},
            "files": {"/workspace/name.txt": "bashkit\n"},
            "limits": {"max_output_bytes": 128},
            "capture_final_env": true
        }"#;
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_json(bytes(config), &mut bash, &mut error),
            BashkitStatus::Ok
        );

        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(
                bash,
                bytes(b"printf '%s ' \"$GREETING\"; cat name.txt; export DONE=yes"),
                &mut result,
                &mut error,
            ),
            BashkitStatus::Ok
        );
        assert_eq!(borrowed(bashkit_result_stdout(result)), b"hello bashkit\n");

        let final_env = borrowed(bashkit_result_final_env_json(result));
        let final_env: serde_json::Value = serde_json::from_slice(&final_env).unwrap();
        assert_eq!(final_env["DONE"], "yes");

        bashkit_result_free(result);
        bashkit_free(bash);
    }
}

#[test]
fn reads_and_writes_binary_vfs_content() {
    unsafe {
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_default(&mut bash, &mut error),
            BashkitStatus::Ok
        );
        assert_eq!(
            bashkit_mkdir(bash, bytes(b"/data"), 0, &mut error),
            BashkitStatus::Ok
        );
        assert_eq!(
            bashkit_write_file(
                bash,
                bytes(b"/data/blob.bin"),
                bytes(b"\0\x01bashkit"),
                &mut error,
            ),
            BashkitStatus::Ok
        );

        let mut buffer = ptr::null_mut();
        assert_eq!(
            bashkit_read_file(bash, bytes(b"/data/blob.bin"), &mut buffer, &mut error),
            BashkitStatus::Ok
        );
        assert_eq!(borrowed(bashkit_buffer_bytes(buffer)), b"\0\x01bashkit");

        bashkit_buffer_free(buffer);

        assert_eq!(
            bashkit_remove(bash, bytes(b"/data/blob.bin"), 0, &mut error),
            BashkitStatus::Ok
        );
        buffer = ptr::null_mut();
        assert_eq!(
            bashkit_read_file(bash, bytes(b"/data/blob.bin"), &mut buffer, &mut error),
            BashkitStatus::IoError
        );
        assert!(buffer.is_null());
        assert_eq!(bashkit_error_code(error), BashkitStatus::IoError as u32);
        bashkit_error_free(error);

        bashkit_free(bash);
    }
}

#[test]
fn rejects_bad_inputs_with_owned_errors() {
    unsafe {
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_default(&mut bash, &mut error),
            BashkitStatus::Ok
        );

        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(bash, bytes(&[0xff]), &mut result, &mut error,),
            BashkitStatus::InvalidUtf8
        );
        assert!(result.is_null());
        assert_eq!(error_message(error), "script must be valid UTF-8");

        error = ptr::null_mut();
        assert_eq!(
            bashkit_execute(bash, bytes(b":"), ptr::null_mut(), &mut error),
            BashkitStatus::InvalidArgument
        );
        assert_eq!(error_message(error), "out_result must not be NULL");

        bashkit_free(bash);
    }
}

#[test]
fn rejects_oversized_script_before_utf8_validation() {
    unsafe {
        let config = br#"{"schema_version":1,"limits":{"max_input_bytes":1}}"#;
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_json(bytes(config), &mut bash, &mut error),
            BashkitStatus::Ok
        );

        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(bash, bytes(&[b':', 0xff]), &mut result, &mut error),
            BashkitStatus::ExecutionError
        );
        assert!(result.is_null());
        assert!(error_message(error).contains("input too large"));
        bashkit_free(bash);
    }
}

#[test]
fn rejects_unknown_config_fields_and_versions() {
    unsafe {
        for (config, expected) in [
            (
                br#"{"schema_version":1,"surprise":true}"#.as_slice(),
                "invalid configuration",
            ),
            (
                br#"{"schema_version":2}"#.as_slice(),
                "unsupported configuration schema version 2",
            ),
            (
                br#"{"schema_version":1,"profile":"permissive"}"#.as_slice(),
                "unknown variant",
            ),
        ] {
            let mut bash = ptr::null_mut();
            let mut error = ptr::null_mut();
            assert_eq!(
                bashkit_create_json(bytes(config), &mut bash, &mut error),
                BashkitStatus::InvalidConfig
            );
            assert!(bash.is_null());
            assert!(error_message(error).contains(expected));
        }
    }
}

#[test]
fn rejects_oversized_config_before_reading_its_pointer() {
    unsafe {
        let config = BashkitBytes {
            ptr: ptr::null(),
            len: BASHKIT_MAX_CONFIG_BYTES + 1,
        };
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_json(config, &mut bash, &mut error),
            BashkitStatus::InvalidConfig
        );
        assert!(bash.is_null());
        assert_eq!(error_message(error), "configuration exceeds 10000000 bytes");
    }
}

#[test]
fn reports_output_truncation_flags() {
    unsafe {
        let config = br#"{
            "schema_version": 1,
            "limits": {"max_output_bytes": 3}
        }"#;
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_json(bytes(config), &mut bash, &mut error),
            BashkitStatus::Ok
        );

        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(
                bash,
                bytes(b"printf '12345'; printf 'abcde' >&2"),
                &mut result,
                &mut error,
            ),
            BashkitStatus::Ok
        );
        assert_eq!(borrowed(bashkit_result_stdout(result)), b"123");
        assert_eq!(borrowed(bashkit_result_stderr(result)), b"abc");
        assert_eq!(
            bashkit_result_flags(result),
            BASHKIT_RESULT_STDOUT_TRUNCATED | BASHKIT_RESULT_STDERR_TRUNCATED
        );

        bashkit_result_free(result);
        bashkit_free(bash);
    }
}

#[test]
fn configured_deadline_aborts_execution() {
    unsafe {
        let config = br#"{"schema_version":1,"limits":{"timeout_ms":10}}"#;
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_json(bytes(config), &mut bash, &mut error),
            BashkitStatus::Ok
        );

        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(bash, bytes(b"sleep 10"), &mut result, &mut error),
            BashkitStatus::ExecutionError
        );
        assert!(result.is_null());
        let message = error_message(error);
        assert!(
            message.contains("execution timeout"),
            "unexpected error: {message}"
        );
        bashkit_free(bash);
    }
}

#[test]
fn cancellation_aborts_running_execution_and_stays_sticky_until_cleared() {
    unsafe {
        let capabilities: serde_json::Value =
            serde_json::from_slice(&borrowed(bashkit_capabilities_json())).unwrap();
        assert!(
            capabilities["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|feature| feature == "cancellation")
        );

        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_default(&mut bash, &mut error),
            BashkitStatus::Ok
        );

        // Cancellation lands at command boundaries, so the script must reach
        // one quickly without tripping the profile's command/iteration caps:
        // a loop of 1-second sleeps gives a boundary every second while
        // burning almost no budget. (A pending single sleep is NOT
        // interruptible; only the profile deadline ends it.)
        // Raw pointers are not Send and edition-2021 closures would capture the
        // inner field of any wrapper anyway, so cross the thread as a usize.
        let handle = bash as usize;

        let observed = std::sync::Arc::new(std::sync::Mutex::new(None::<BashkitStatus>));
        let writer = observed.clone();
        let worker = std::thread::spawn(move || {
            let bash = handle as *mut Bashkit;
            // No inner `unsafe` block: the closure literal is lexically nested
            // under the test's `unsafe` block, which covers the body.
            let mut result = ptr::null_mut();
            let mut thread_error = ptr::null_mut();
            let status = bashkit_execute(
                bash,
                bytes(b"while true; do sleep 1; done"),
                &mut result,
                &mut thread_error,
            );
            assert!(result.is_null());
            *writer.lock().unwrap() = Some(status);
            if !thread_error.is_null() {
                bashkit_error_free(thread_error);
            }
        });

        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(bashkit_cancel(bash), BashkitStatus::Ok);
        worker.join().unwrap();
        assert_eq!(
            observed.lock().unwrap().take(),
            Some(BashkitStatus::Cancelled)
        );

        // Sticky: the next execute aborts immediately until the flag is cleared.
        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(bash, bytes(b"echo blocked"), &mut result, &mut error),
            BashkitStatus::Cancelled
        );
        assert!(result.is_null());
        bashkit_error_free(error);

        // clear_cancel restores normal execution without losing shell state.
        assert_eq!(bashkit_clear_cancel(bash), BashkitStatus::Ok);
        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(bash, bytes(b"echo resumed"), &mut result, &mut error),
            BashkitStatus::Ok
        );
        assert_eq!(borrowed(bashkit_result_stdout(result)), b"resumed\n");
        bashkit_result_free(result);

        bashkit_free(bash);
    }
}

#[test]
fn cancel_rejects_null_handle_without_touching_state() {
    unsafe {
        assert_eq!(
            bashkit_cancel(ptr::null_mut()),
            BashkitStatus::InvalidArgument
        );
        assert_eq!(
            bashkit_clear_cancel(ptr::null_mut()),
            BashkitStatus::InvalidArgument
        );
    }
}

#[test]
fn null_destructors_and_accessors_are_safe() {
    unsafe {
        bashkit_free(ptr::null_mut());
        bashkit_result_free(ptr::null_mut());
        bashkit_buffer_free(ptr::null_mut());
        bashkit_error_free(ptr::null_mut());
        assert_eq!(bashkit_result_exit_code(ptr::null()), 0);
        assert_eq!(bashkit_result_flags(ptr::null()), 0);
        assert_eq!(bashkit_result_stdout(ptr::null()).len, 0);
        assert_eq!(bashkit_buffer_bytes(ptr::null()).len, 0);
        assert_eq!(bashkit_error_message(ptr::null()).len, 0);
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bashkit-capi-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn config_mounts_expose_host_dir_read_only() {
    unsafe {
        let host = temp_dir("mount-ro");
        std::fs::write(host.join("note.txt"), b"host-bytes").unwrap();
        let config = serde_json::json!({
            "schema_version": 1,
            "allowed_mount_paths": [host.to_string_lossy()],
            "mounts": [{"path": "/data", "root": host.to_string_lossy()}],
        })
        .to_string();

        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_json(bytes(config.as_bytes()), &mut bash, &mut error),
            BashkitStatus::Ok
        );

        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(bash, bytes(b"cat /data/note.txt"), &mut result, &mut error),
            BashkitStatus::Ok
        );
        assert_eq!(borrowed(bashkit_result_stdout(result)), b"host-bytes");
        bashkit_result_free(result);

        // Read-only mount: writes through the mount never reach the host.
        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(
                bash,
                bytes(b"echo nope > /data/denied.txt"),
                &mut result,
                &mut error,
            ),
            BashkitStatus::Ok
        );
        bashkit_result_free(result);
        assert!(!host.join("denied.txt").exists());

        bashkit_free(bash);
        let _ = std::fs::remove_dir_all(&host);
    }
}

#[test]
fn runtime_mount_and_unmount_round_trip() {
    unsafe {
        let host = temp_dir("mount-rt");
        std::fs::write(host.join("f.txt"), b"rt").unwrap();
        let config = serde_json::json!({
            "schema_version": 1,
            "allowed_mount_paths": [host.to_string_lossy()],
        })
        .to_string();

        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_json(bytes(config.as_bytes()), &mut bash, &mut error),
            BashkitStatus::Ok
        );

        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(bash, bytes(b"cat /mnt/f.txt"), &mut result, &mut error),
            BashkitStatus::Ok
        );
        assert_ne!(bashkit_result_exit_code(result), 0);
        bashkit_result_free(result);

        assert_eq!(
            bashkit_mount(
                bash,
                bytes(b"/mnt"),
                bytes(host.to_string_lossy().as_bytes()),
                0,
                &mut error,
            ),
            BashkitStatus::Ok
        );
        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(bash, bytes(b"cat /mnt/f.txt"), &mut result, &mut error),
            BashkitStatus::Ok
        );
        assert_eq!(borrowed(bashkit_result_stdout(result)), b"rt");
        bashkit_result_free(result);

        assert_eq!(
            bashkit_unmount(bash, bytes(b"/mnt"), &mut error),
            BashkitStatus::Ok
        );
        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(bash, bytes(b"cat /mnt/f.txt"), &mut result, &mut error),
            BashkitStatus::Ok
        );
        assert_ne!(bashkit_result_exit_code(result), 0);
        bashkit_result_free(result);

        bashkit_free(bash);
        let _ = std::fs::remove_dir_all(&host);
    }
}

#[test]
fn mounts_require_allowlist_and_containment() {
    unsafe {
        // No allowed_mount_paths: config mount is rejected outright.
        let host = temp_dir("mount-denied");
        let config = serde_json::json!({
            "schema_version": 1,
            "mounts": [{"path": "/data", "root": host.to_string_lossy()}],
        })
        .to_string();
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_json(bytes(config.as_bytes()), &mut bash, &mut error),
            BashkitStatus::InvalidConfig
        );
        assert_eq!(
            bashkit_error_code(error),
            BashkitStatus::InvalidConfig as u32
        );
        bashkit_error_free(error);

        // Root outside every allowed prefix is rejected at mount time.
        let allowed = temp_dir("mount-allowed");
        let outside = temp_dir("mount-outside");
        let config = serde_json::json!({
            "schema_version": 1,
            "allowed_mount_paths": [allowed.to_string_lossy()],
        })
        .to_string();
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_json(bytes(config.as_bytes()), &mut bash, &mut error),
            BashkitStatus::Ok
        );
        assert_ne!(
            bashkit_mount(
                bash,
                bytes(b"/data"),
                bytes(outside.to_string_lossy().as_bytes()),
                0,
                &mut error,
            ),
            BashkitStatus::Ok
        );
        assert!(!error.is_null());
        bashkit_error_free(error);
        bashkit_free(bash);
        let _ = std::fs::remove_dir_all(&host);
        let _ = std::fs::remove_dir_all(&allowed);
        let _ = std::fs::remove_dir_all(&outside);
    }
}

// THREAT[TM-FS-013]: a broad allowlist entry (the home directory itself) is
// not consent to expose a credential directory under it.
#[test]
fn config_mounts_refuse_sensitive_paths_under_broad_allowlist() {
    unsafe {
        let home = temp_dir("mount-home");
        let ssh_dir = home.join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();
        std::fs::write(ssh_dir.join("id_rsa"), b"PRIVATE-KEY-BYTES").unwrap();

        let config = serde_json::json!({
            "schema_version": 1,
            "allowed_mount_paths": [home.to_string_lossy()],
            "mounts": [{"path": "/data", "root": ssh_dir.to_string_lossy()}],
        })
        .to_string();
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_json(bytes(config.as_bytes()), &mut bash, &mut error),
            BashkitStatus::InvalidConfig
        );
        assert!(bash.is_null());
        assert!(
            error_message(error).contains("sensitive host path"),
            "error must name the sensitive-path rule"
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}

#[test]
fn runtime_mount_refuses_sensitive_paths_under_broad_allowlist() {
    unsafe {
        let home = temp_dir("mount-home-rt");
        let ssh_dir = home.join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();
        std::fs::write(ssh_dir.join("id_rsa"), b"PRIVATE-KEY-BYTES").unwrap();

        let config = serde_json::json!({
            "schema_version": 1,
            "allowed_mount_paths": [home.to_string_lossy()],
        })
        .to_string();
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_json(bytes(config.as_bytes()), &mut bash, &mut error),
            BashkitStatus::Ok
        );

        assert_ne!(
            bashkit_mount(
                bash,
                bytes(b"/data"),
                bytes(ssh_dir.to_string_lossy().as_bytes()),
                0,
                &mut error,
            ),
            BashkitStatus::Ok
        );
        assert!(!error.is_null());
        bashkit_error_free(error);

        // The refused mount left nothing behind: the path stays unresolved.
        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(bash, bytes(b"cat /data/id_rsa"), &mut result, &mut error),
            BashkitStatus::Ok
        );
        assert_ne!(bashkit_result_exit_code(result), 0);
        bashkit_result_free(result);

        bashkit_free(bash);
        let _ = std::fs::remove_dir_all(&home);
    }
}

#[test]
fn sensitive_path_mounts_when_allowlisted_exactly() {
    unsafe {
        let home = temp_dir("mount-home-exact");
        let ssh_dir = home.join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();
        std::fs::write(ssh_dir.join("id_rsa"), b"PRIVATE-KEY-BYTES").unwrap();

        // Naming the sensitive root itself in the allowlist is explicit
        // consent: the mount is allowed for both config-time and runtime.
        let config = serde_json::json!({
            "schema_version": 1,
            "allowed_mount_paths": [ssh_dir.to_string_lossy()],
            "mounts": [{"path": "/data", "root": ssh_dir.to_string_lossy()}],
        })
        .to_string();
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_json(bytes(config.as_bytes()), &mut bash, &mut error),
            BashkitStatus::Ok
        );
        let mut result = ptr::null_mut();
        assert_eq!(
            bashkit_execute(bash, bytes(b"cat /data/id_rsa"), &mut result, &mut error),
            BashkitStatus::Ok
        );
        assert_eq!(
            borrowed(bashkit_result_stdout(result)),
            b"PRIVATE-KEY-BYTES"
        );
        bashkit_result_free(result);
        bashkit_free(bash);
        let _ = std::fs::remove_dir_all(&home);
    }
}
