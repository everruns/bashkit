use std::time::Duration;

use bashkit::{Bash, ExecOptions, ExecResult, ExecutionEvent, ExecutionLimits};

#[tokio::test]
async fn host_call_suspends_and_resumes_the_live_execution() {
    let bash = Bash::builder().host_call_builtin("lookup").build();
    let mut execution = bash.start_execution(
        "export REGION=us-east; cd /tmp; printf 'before\\n'; lookup alice; printf 'after\\n'",
    );

    let request = match execution.next_event().await.unwrap() {
        ExecutionEvent::HostCall(request) => request,
        ExecutionEvent::Complete(_) => panic!("execution completed before its host call"),
    };
    assert_eq!(request.command(), "lookup");
    assert_eq!(request.args(), &["alice"]);
    assert_eq!(request.cwd().to_string_lossy(), "/tmp");
    assert_eq!(
        request.env().get("REGION").map(String::as_str),
        Some("us-east")
    );
    assert_eq!(request.stdin(), None);

    execution
        .resume(request.id(), ExecResult::ok("found: alice\n".to_string()))
        .unwrap();

    let result = match execution.next_event().await.unwrap() {
        ExecutionEvent::Complete(result) => result,
        ExecutionEvent::HostCall(_) => panic!("unexpected second host call"),
    };
    assert_eq!(result.stdout, "before\nfound: alice\nafter\n");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn host_call_receives_pipeline_stdin_and_propagates_failure() {
    let bash = Bash::builder().host_call_builtin("remote").build();
    let mut execution = bash.start_execution("printf payload | remote value");

    let request = match execution.next_event().await.unwrap() {
        ExecutionEvent::HostCall(request) => request,
        ExecutionEvent::Complete(_) => panic!("execution completed before its host call"),
    };
    assert_eq!(request.args(), &["value"]);
    assert_eq!(request.stdin().unwrap(), "payload");
    execution
        .resume(
            request.id(),
            ExecResult::err("remote: unavailable\n".to_string(), 23),
        )
        .unwrap();

    let result = match execution.next_event().await.unwrap() {
        ExecutionEvent::Complete(result) => result,
        ExecutionEvent::HostCall(_) => panic!("unexpected second host call"),
    };
    assert_eq!(result.stderr, "remote: unavailable\n");
    assert_eq!(result.exit_code, 23);
}

#[tokio::test]
async fn resume_rejects_an_unknown_request_without_losing_the_pending_call() {
    let bash = Bash::builder().host_call_builtin("lookup").build();
    let mut execution = bash.start_execution("lookup alice");

    let request = match execution.next_event().await.unwrap() {
        ExecutionEvent::HostCall(request) => request,
        ExecutionEvent::Complete(_) => panic!("execution completed before its host call"),
    };
    execution
        .resume(request.id(), ExecResult::ok("ok\n".to_string()))
        .unwrap();
    let error = execution
        .resume(request.id(), ExecResult::ok(String::new()))
        .unwrap_err();
    assert!(error.to_string().contains("unknown host-call request"));
    let result = match execution.next_event().await.unwrap() {
        ExecutionEvent::Complete(result) => result,
        ExecutionEvent::HostCall(_) => panic!("unexpected second host call"),
    };
    assert_eq!(result.stdout, "ok\n");
}

#[tokio::test]
async fn ordinary_exec_fails_host_call_builtin_without_a_driver() {
    let mut bash = Bash::builder().host_call_builtin("lookup").build();
    let result = bash.exec("lookup alice").await.unwrap();

    assert_eq!(result.exit_code, 1);
    assert_eq!(
        result.stderr,
        "lookup: host-call builtin requires Bash::start_execution\n"
    );
}

#[tokio::test]
async fn execution_can_resume_multiple_host_calls() {
    let bash = Bash::builder().host_call_builtin("lookup").build();
    let mut execution = bash.start_execution("lookup one; lookup two");

    for expected in ["one", "two"] {
        let request = match execution.next_event().await.unwrap() {
            ExecutionEvent::HostCall(request) => request,
            ExecutionEvent::Complete(_) => panic!("execution completed before all host calls"),
        };
        assert_eq!(request.args(), &[expected]);
        execution
            .resume(request.id(), ExecResult::ok(format!("{expected}!\n")))
            .unwrap();
    }

    let result = match execution.next_event().await.unwrap() {
        ExecutionEvent::Complete(result) => result,
        ExecutionEvent::HostCall(_) => panic!("unexpected third host call"),
    };
    assert_eq!(result.stdout, "one!\ntwo!\n");
}

#[tokio::test(start_paused = true)]
async fn suspended_host_call_remains_inside_the_execution_timeout() {
    let bash = Bash::builder()
        .host_call_builtin("lookup")
        .limits(ExecutionLimits::new().timeout(Duration::from_secs(2)))
        .build();
    let mut execution =
        bash.start_execution_with_options("lookup alice", ExecOptions::new().stdin("unused"));

    assert!(matches!(
        execution.next_event().await.unwrap(),
        ExecutionEvent::HostCall(_)
    ));
    tokio::time::advance(Duration::from_secs(3)).await;
    let error = execution.next_event().await.unwrap_err();
    assert!(error.to_string().contains("timeout"));
    let _bash = execution.into_bash().unwrap();
}

#[tokio::test]
async fn completed_execution_returns_the_reusable_bash_session() {
    let bash = Bash::builder().host_call_builtin("lookup").build();
    let mut execution = bash.start_execution("lookup alice");
    let request = match execution.next_event().await.unwrap() {
        ExecutionEvent::HostCall(request) => request,
        ExecutionEvent::Complete(_) => panic!("execution completed before its host call"),
    };
    execution = match execution.into_bash() {
        Ok(_) => panic!("active execution returned its Bash session"),
        Err(execution) => execution,
    };
    execution
        .resume(request.id(), ExecResult::ok("ok\n"))
        .unwrap();
    assert!(matches!(
        execution.next_event().await.unwrap(),
        ExecutionEvent::Complete(_)
    ));

    let mut bash = execution.into_bash().unwrap();
    let result = bash.exec("echo reused").await.unwrap();
    assert_eq!(result.stdout, "reused\n");
}
