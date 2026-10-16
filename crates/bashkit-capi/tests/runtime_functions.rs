use bashkit_capi::*;
use std::{
    ffi::c_void,
    ptr,
    sync::{
        Mutex,
        atomic::{AtomicPtr, AtomicU32, Ordering},
    },
    thread,
    time::Duration,
};

fn bytes(s: &[u8]) -> BashkitBytes {
    BashkitBytes {
        ptr: s.as_ptr(),
        len: s.len(),
    }
}
unsafe fn view(v: BashkitBytes) -> Vec<u8> {
    if v.len == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(v.ptr, v.len) }.to_vec()
    }
}
struct Host {
    mode: u8,
    requests: Mutex<Vec<(thread::ThreadId, serde_json::Value)>>,
    pending: Mutex<Vec<usize>>,
    workers: Mutex<Vec<thread::JoinHandle<()>>>,
    bash: AtomicPtr<Bashkit>,
    reentry: AtomicU32,
    response: AtomicU32,
}
impl Host {
    fn new(mode: u8) -> Self {
        Self {
            mode,
            requests: Mutex::new(Vec::new()),
            pending: Mutex::new(Vec::new()),
            workers: Mutex::new(Vec::new()),
            bash: AtomicPtr::new(ptr::null_mut()),
            reentry: AtomicU32::new(99),
            response: AtomicU32::new(99),
        }
    }
    fn join(&self) {
        for worker in self.workers.lock().unwrap().drain(..) {
            worker.join().unwrap();
        }
    }
}
unsafe extern "C" fn handler(data: *mut c_void, call: *mut BashkitRuntimeCall) {
    unsafe {
        let host = &*data.cast::<Host>();
        let request = serde_json::from_slice(&view(bashkit_runtime_call_request(call))).unwrap();
        host.requests
            .lock()
            .unwrap()
            .push((thread::current().id(), request));
        match host.mode {
            1 => {
                // Transfer ownership to an asynchronous foreign worker.
                let owned = call as usize;
                host.workers.lock().unwrap().push(thread::spawn(move || {
                    thread::sleep(Duration::from_millis(5));
                    let call = owned as *mut BashkitRuntimeCall;
                    bashkit_runtime_call_respond_json(call, bytes(b"42"));
                    bashkit_runtime_call_free(call);
                }));
            }
            2 => host.pending.lock().unwrap().push(call as usize),
            3 => {
                bashkit_runtime_call_fail(call);
                bashkit_runtime_call_free(call);
            }
            4 => bashkit_runtime_call_free(call),
            5 => {
                let mut result = ptr::null_mut();
                let mut error = ptr::null_mut();
                let status = bashkit_execute_streaming(
                    host.bash.load(Ordering::SeqCst),
                    bytes(b"echo forbidden"),
                    0,
                    None,
                    ptr::null_mut(),
                    &mut result,
                    &mut error,
                );
                host.reentry.store(status as u32, Ordering::SeqCst);
                bashkit_result_free(result);
                bashkit_error_free(error);
                bashkit_runtime_call_respond_json(call, bytes(b"42"));
                bashkit_runtime_call_free(call);
            }
            _ => {
                let mut result = ptr::null_mut();
                let mut error = ptr::null_mut();
                let status = bashkit_execute(
                    host.bash.load(Ordering::SeqCst),
                    bytes(b"echo forbidden"),
                    &mut result,
                    &mut error,
                );
                host.reentry.store(status as u32, Ordering::SeqCst);
                bashkit_error_free(error);
                let invalid = bashkit_runtime_call_respond_json(call, bytes(b"invalid JSON"));
                host.response.store(invalid as u32, Ordering::SeqCst);
                bashkit_runtime_call_respond_json(call, bytes(b"42"));
                bashkit_runtime_call_free(call);
            }
        }
    }
}
unsafe fn create(host: &Host, timeout: u64) -> *mut Bashkit {
    let config = format!(
        r#"{{"schema_version":1,"python":true,"typescript":true,"external_functions":["lookup"],"typescript_external_functions":["lookup"],"limits":{{"timeout_ms":{timeout}}}}}"#
    );
    let mut bash = ptr::null_mut();
    let mut error = ptr::null_mut();
    assert_eq!(
        unsafe {
            bashkit_create_with_runtime_handler_v1(
                bytes(config.as_bytes()),
                Some(handler),
                ptr::from_ref(host).cast_mut().cast(),
                &mut bash,
                &mut error,
            )
        },
        BashkitStatus::Ok
    );
    host.bash.store(bash, Ordering::SeqCst);
    bash
}
unsafe fn execute(bash: *mut Bashkit, script: &str) -> (BashkitStatus, i32, Vec<u8>) {
    unsafe {
        let mut result = ptr::null_mut();
        let mut error = ptr::null_mut();
        let status = bashkit_execute(bash, bytes(script.as_bytes()), &mut result, &mut error);
        let exit = bashkit_result_exit_code(result);
        let stdout = view(bashkit_result_stdout(result));
        bashkit_result_free(result);
        bashkit_error_free(error);
        (status, exit, stdout)
    }
}

#[test]
fn c_runtime_callbacks_run_on_caller_thread_and_reject_reentry() {
    unsafe {
        let host = Host::new(0);
        let bash = create(&host, 5000);
        for script in [
            "python -c 'print(lookup(21, multiplier=2))'",
            "ts -c 'await lookup(21, 2)'",
        ] {
            assert_eq!(
                execute(bash, script),
                (BashkitStatus::Ok, 0, b"42\n".to_vec())
            );
        }
        let requests = host.requests.lock().unwrap();
        assert!(requests.iter().all(|(id, _)| *id == thread::current().id()));
        assert_eq!(requests[0].1["kwargs"]["multiplier"], 2);
        assert_eq!(requests[1].1["args"], serde_json::json!([21, 2]));
        assert_eq!(
            host.reentry.load(Ordering::SeqCst),
            BashkitStatus::Busy as u32
        );
        assert_eq!(
            host.response.load(Ordering::SeqCst),
            BashkitStatus::InvalidArgument as u32
        );
        bashkit_free(bash);
    }
}

#[test]
fn c_runtime_callbacks_accept_replies_from_other_threads() {
    let workers: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(|| unsafe {
                let host = Host::new(1);
                let bash = create(&host, 5000);
                for _ in 0..5 {
                    assert_eq!(
                        execute(bash, "python -c 'print(lookup())'; ts -c 'await lookup()'"),
                        (BashkitStatus::Ok, 0, b"42\n42\n".to_vec())
                    );
                }
                host.join();
                bashkit_free(bash);
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
}

#[test]
fn c_runtime_calls_are_revoked_after_deadline_and_handle_can_be_reused() {
    unsafe {
        for command in ["python -c 'lookup()'", "ts -c 'await lookup()'"] {
            let host = Host::new(2);
            let bash = create(&host, 200);
            let (status, exit, _) = execute(bash, command);
            assert!(status != BashkitStatus::Ok || exit != 0);
            let call = host
                .pending
                .lock()
                .unwrap()
                .pop()
                .expect("handler must start") as *mut BashkitRuntimeCall;
            assert_eq!(bashkit_runtime_call_cancelled(call), 1);
            assert_eq!(
                bashkit_runtime_call_respond_json(call, bytes(b"42")),
                BashkitStatus::Cancelled
            );
            bashkit_runtime_call_free(call);
            assert_eq!(
                execute(bash, "echo reusable"),
                (BashkitStatus::Ok, 0, b"reusable\n".to_vec())
            );
            bashkit_free(bash);
        }
    }
}

#[test]
fn c_runtime_host_failures_and_abandoned_calls_fail_scripts() {
    unsafe {
        for mode in [3, 4] {
            let host = Host::new(mode);
            let bash = create(&host, 5000);
            for script in ["python -c 'lookup()'", "ts -c 'await lookup()'"] {
                let (status, exit, _) = execute(bash, script);
                assert!(status != BashkitStatus::Ok || exit != 0);
            }
            bashkit_free(bash);
        }
    }
}

#[test]
fn c_runtime_cancellation_revokes_pending_calls_from_another_thread() {
    unsafe {
        let host = Host::new(2);
        let bash = create(&host, 5000);
        let owned = bash as usize;
        thread::scope(|scope| {
            let execution = scope.spawn(|| execute(owned as *mut Bashkit, "python -c 'lookup()'"));
            for _ in 0..1000 {
                if !host.pending.lock().unwrap().is_empty() {
                    break;
                }
                thread::sleep(Duration::from_millis(1));
            }
            let call = host
                .pending
                .lock()
                .unwrap()
                .pop()
                .expect("host callback started") as *mut BashkitRuntimeCall;
            assert_eq!(bashkit_cancel(bash), BashkitStatus::Ok);
            let (status, exit, _) = execution.join().unwrap();
            assert!(status != BashkitStatus::Ok || exit != 0);
            assert_eq!(bashkit_runtime_call_cancelled(call), 1);
            assert_eq!(
                bashkit_runtime_call_respond_json(call, bytes(b"42")),
                BashkitStatus::Cancelled
            );
            bashkit_runtime_call_free(call);
        });
        assert_eq!(bashkit_clear_cancel(bash), BashkitStatus::Ok);
        assert_eq!(
            execute(bash, "echo reusable"),
            (BashkitStatus::Ok, 0, b"reusable\n".to_vec())
        );
        bashkit_free(bash);
    }
}

#[test]
fn c_runtime_construction_and_json_boundaries_fail_closed() {
    unsafe {
        let host = Host::new(0);
        for config in [
            r#"{"schema_version":1,"external_functions":["lookup"]}"#,
            r#"{"schema_version":1,"python":true,"external_functions":["lookup"]}"#,
        ] {
            let mut bash = ptr::null_mut();
            let mut error = ptr::null_mut();
            assert_eq!(
                bashkit_create_json(bytes(config.as_bytes()), &mut bash, &mut error),
                BashkitStatus::InvalidConfig
            );
            assert!(bash.is_null());
            bashkit_error_free(error);
        }
        let bash = create(&host, 5000);
        assert_eq!(execute(bash, "python -c 'lookup(bytes([1]))'").1, 1);
        assert_eq!(execute(bash, "ts -c 'await lookup(() => 42)'").1, 1);
        assert!(host.requests.lock().unwrap().is_empty());
        bashkit_free(bash);
        assert_eq!(bashkit_runtime_call_cancelled(ptr::null()), 1);
        assert_eq!(bashkit_runtime_call_request(ptr::null()).len, 0);
        bashkit_runtime_call_free(ptr::null_mut());
        assert_eq!(
            bashkit_runtime_call_respond_json(
                ptr::null_mut(),
                BashkitBytes {
                    ptr: std::ptr::dangling::<u8>(),
                    len: BASHKIT_MAX_RUNTIME_VALUE_BYTES + 1
                }
            ),
            BashkitStatus::InvalidArgument
        );
    }
}

#[test]
fn c_runtime_idle_handle_migrates_between_caller_threads() {
    unsafe {
        let host = Host::new(0);
        let bash = create(&host, 5000);
        for _ in 0..5 {
            let owned = bash as usize;
            let id = thread::spawn(move || {
                assert_eq!(
                    execute(
                        owned as *mut Bashkit,
                        "python -c 'print(lookup())'; ts -c 'await lookup()'"
                    ),
                    (BashkitStatus::Ok, 0, b"42\n42\n".to_vec())
                );
                thread::current().id()
            })
            .join()
            .unwrap();
            assert_eq!(host.requests.lock().unwrap().last().unwrap().0, id);
        }
        bashkit_free(bash);
    }
}

#[test]
fn c_runtime_owned_call_remains_safe_after_handle_is_freed() {
    unsafe {
        let host = Host::new(2);
        let bash = create(&host, 100);
        let (status, exit, _) = execute(bash, "ts -c 'await lookup()'");
        assert!(status != BashkitStatus::Ok || exit != 0);
        let call = host.pending.lock().unwrap().pop().unwrap() as *mut BashkitRuntimeCall;
        bashkit_free(bash);
        assert!(!view(bashkit_runtime_call_request(call)).is_empty());
        assert_eq!(bashkit_runtime_call_cancelled(call), 1);
        assert_eq!(
            bashkit_runtime_call_respond_json(call, bytes(b"42")),
            BashkitStatus::Cancelled
        );
        bashkit_runtime_call_free(call);
    }
}

#[test]
fn c_runtime_flags_are_opt_in_and_missing_dispatchers_are_rejected() {
    unsafe {
        let mut bash = std::ptr::dangling_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_with_runtime_handler_v1(
                bytes(br#"{"schema_version":1}"#),
                None,
                ptr::null_mut(),
                &mut bash,
                &mut error
            ),
            BashkitStatus::InvalidArgument
        );
        assert!(bash.is_null());
        bashkit_error_free(error);
        error = ptr::null_mut();
        assert_eq!(
            bashkit_create_json(
                bytes(br#"{"schema_version":1,"python":true,"typescript":true}"#),
                &mut bash,
                &mut error
            ),
            BashkitStatus::Ok
        );
        assert_eq!(
            execute(bash, "python -c 'print(21*2)'; ts -c '21*2'"),
            (BashkitStatus::Ok, 0, b"42\n42\n".to_vec())
        );
        bashkit_free(bash);
        assert_eq!(
            bashkit_create_default(&mut bash, &mut error),
            BashkitStatus::Ok
        );
        assert_eq!(execute(bash, "python -c 'print(42)'").1, 127);
        assert_eq!(execute(bash, "ts -c '42'").1, 127);
        bashkit_free(bash);
    }
}

#[test]
fn c_runtime_callbacks_preserve_hardened_python_memory_limit() {
    unsafe {
        let host = Host::new(0);
        let mut bash = ptr::null_mut();
        let mut error = ptr::null_mut();
        let config = br#"{"schema_version":1,"profile":"hardened","python":true,"external_functions":["lookup"]}"#;
        assert_eq!(
            bashkit_create_with_runtime_handler_v1(
                bytes(config),
                Some(handler),
                ptr::from_ref(&host).cast_mut().cast(),
                &mut bash,
                &mut error
            ),
            BashkitStatus::Ok
        );
        host.bash.store(bash, Ordering::SeqCst);
        let (status, exit, _) = execute(bash, "python -c 'x=[0]*2500000; print(lookup())'");
        assert!(
            status != BashkitStatus::Ok || exit != 0,
            "hardened VM must reject a list larger than 16 MB"
        );
        assert!(host.requests.lock().unwrap().is_empty());
        bashkit_free(bash);
    }
}

struct LiveOutput {
    bytes: Vec<u8>,
    caller: thread::ThreadId,
    other: *mut Bashkit,
    reentry: Option<BashkitStatus>,
}
unsafe extern "C" fn collect_live(data: *mut c_void, stream: u32, chunk: BashkitBytes) -> u32 {
    unsafe {
        let output = &mut *data.cast::<LiveOutput>();
        if stream != 1 || thread::current().id() != output.caller {
            return 1;
        }
        output.bytes.extend(view(chunk));
        let mut result = ptr::null_mut();
        let mut error = ptr::null_mut();
        output.reentry = Some(bashkit_execute_streaming(
            output.other,
            bytes(b"echo forbidden"),
            0,
            None,
            ptr::null_mut(),
            &mut result,
            &mut error,
        ));
        bashkit_result_free(result);
        bashkit_error_free(error);
        0
    }
}

#[test]
fn c_runtime_callbacks_work_with_live_output_and_capture() {
    unsafe {
        let mut other = ptr::null_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            bashkit_create_default(&mut other, &mut error),
            BashkitStatus::Ok
        );
        for mode in [0, 1, 5] {
            let host = Host::new(mode);
            let bash = create(&host, 5000);
            for capture in [0, 2, 64] {
                for script in [
                    "python -c 'print(lookup())' | cat",
                    "ts -c 'await lookup()' | cat",
                ] {
                    let mut output = LiveOutput {
                        bytes: Vec::new(),
                        caller: thread::current().id(),
                        other,
                        reentry: None,
                    };
                    let mut result = ptr::null_mut();
                    assert_eq!(
                        bashkit_execute_streaming(
                            bash,
                            bytes(script.as_bytes()),
                            capture,
                            Some(collect_live),
                            ptr::from_mut(&mut output).cast(),
                            &mut result,
                            &mut error,
                        ),
                        BashkitStatus::Ok
                    );
                    assert_eq!(bashkit_result_exit_code(result), 0);
                    assert_eq!(output.bytes, b"42\n");
                    assert_eq!(output.reentry, Some(BashkitStatus::Busy));
                    assert_eq!(
                        view(bashkit_result_stdout(result)),
                        &b"42\n"[..capture.min(3)]
                    );
                    assert_eq!(
                        bashkit_result_flags(result) & BASHKIT_RESULT_STDOUT_TRUNCATED != 0,
                        capture == 2
                    );
                    bashkit_result_free(result);
                }
            }
            if mode == 5 {
                assert_eq!(
                    host.reentry.load(Ordering::SeqCst),
                    BashkitStatus::Busy as u32
                );
            }
            assert!(
                host.requests
                    .lock()
                    .unwrap()
                    .iter()
                    .all(|(id, _)| *id == thread::current().id())
            );
            host.join();
            bashkit_free(bash);
        }
        assert_eq!(
            execute(other, "echo reusable"),
            (BashkitStatus::Ok, 0, b"reusable\n".to_vec())
        );
        bashkit_free(other);
    }
}
