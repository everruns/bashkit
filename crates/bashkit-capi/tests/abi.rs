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
