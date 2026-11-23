//! Bashkit: one in-process `Bash` interpreter per session. The guest calls a registered host
//! builtin and receives its result through ordinary shell command substitution.

use std::time::Duration;

use bashkit::{
    Bash, Builtin, BuiltinContext, ExecResult, ExecutionEvent, ExecutionLimits, SessionLimits,
    async_trait,
};
use bench_common::{Info, run_owned, tick};
use tokio::runtime::{Builder, Runtime};

const GUEST: &str = r#"
run() {
    local start=$1 count=$2 id reply
    local end=$((start + count))
    for ((id = start; id < end; id++)); do
        __CALL_BODY__
    done
}
"#;

const MAX_CALLS: usize = 2_000_000;
const MAX_WORK_UNITS: u64 = 10_000_000_000;

#[derive(Clone, Copy)]
enum CallPath {
    Event,
    Direct,
}

#[derive(Clone, Copy)]
enum Reply {
    Json,
    Text,
    Variable,
    Empty,
}

impl Reply {
    fn call_body(self) -> &'static str {
        match self {
            Self::Json => {
                r#"reply=$(call_tool "$id" "hello from the sandbox") || return
        [[ $reply =~ \"id\":([0-9]+) ]] || return 3
        ((BASH_REMATCH[1] == id)) || return 4"#
            }
            Self::Text => {
                r#"reply=$(call_tool "$id" "hello from the sandbox") || return
        [[ $reply == "$id:22" ]] || return 4"#
            }
            Self::Variable => {
                r#"call_tool "$id" "hello from the sandbox" || return
        [[ $reply == "$id:22" ]] || return 4"#
            }
            Self::Empty => r#"call_tool "$id" "hello from the sandbox" || return"#,
        }
    }

    fn host_call(self, args: &[String]) -> ExecResult {
        tick();
        let [id, value] = args else {
            panic!("call_tool expects an ID and a string");
        };
        let length = value.len();
        ExecResult::ok(match self {
            Self::Json => format!("{{\"id\":{id},\"ok\":true,\"value\":{length}}}\n"),
            Self::Text | Self::Variable => format!("{id}:{length}\n"),
            Self::Empty => {
                let numeric_id: i64 = id.parse().expect("numeric call ID");
                std::hint::black_box((numeric_id, length));
                String::new()
            }
        })
    }
}

struct Lookup(Reply);

#[async_trait]
impl Builtin for Lookup {
    async fn execute(&self, context: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        if matches!(self.0, Reply::Variable) {
            tick();
            let [id, value] = context.args else {
                panic!("call_tool expects an ID and a string");
            };
            context
                .variables
                .insert("reply".to_string(), format!("{id}:{}", value.len()));
            return Ok(ExecResult::ok(""));
        }
        Ok(self.0.host_call(context.args))
    }
}

struct Session {
    bash: Option<Bash>,
}

fn exec(runtime: &Runtime, session: &mut Session, script: &str) -> ExecResult {
    runtime
        .block_on(session.bash.as_mut().expect("session").exec(script))
        .unwrap_or_else(|error| panic!("bashkit exec failed for {script:?}: {error}"))
}

fn exec_host_calls(
    runtime: &Runtime,
    session: &mut Session,
    script: String,
    reply: Reply,
) -> ExecResult {
    let bash = session.bash.take().expect("session");
    let mut execution = bash.start_execution(script);
    let result = runtime.block_on(async {
        loop {
            match execution.next_event().await.expect("next execution event") {
                ExecutionEvent::HostCall(request) => {
                    assert_eq!(request.command(), "call_tool");
                    execution
                        .resume(request.id(), reply.host_call(request.args()))
                        .expect("resume host call");
                }
                ExecutionEvent::Complete(result) => break result,
            }
        }
    });
    session.bash = Some(execution.into_bash().expect("completed Bash session"));
    result
}

fn new_session(runtime: &Runtime, call_path: CallPath, reply: Reply) -> Session {
    let limits = ExecutionLimits::new()
        .max_work_units(MAX_WORK_UNITS)
        .max_commands(MAX_CALLS)
        .max_loop_iterations(MAX_CALLS)
        .max_total_loop_iterations(MAX_CALLS)
        .timeout(Duration::from_secs(600));
    let session_limits = SessionLimits::new()
        .max_total_commands(MAX_CALLS as u64)
        .max_exec_calls(20);
    let builder = Bash::builder()
        .limits(limits)
        .session_limits(session_limits);
    let builder = match call_path {
        CallPath::Event => builder.host_call_builtin("call_tool"),
        CallPath::Direct => builder.builtin("call_tool", Box::new(Lookup(reply))),
    };
    let mut session = Session {
        bash: Some(builder.build()),
    };
    let result = exec(runtime, &mut session, "echo $((1 + 1))");
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "2\n");
    session
}

fn main() {
    let (key, call_path, reply) = match std::env::args().nth(1).as_deref() {
        None => ("bashkit", CallPath::Event, Reply::Json),
        Some("event-json") => ("event-json", CallPath::Event, Reply::Json),
        Some("event-text") => ("event-text", CallPath::Event, Reply::Text),
        Some("direct-json") => ("direct-json", CallPath::Direct, Reply::Json),
        Some("direct-text") => ("direct-text", CallPath::Direct, Reply::Text),
        Some("direct-variable") => ("direct-variable", CallPath::Direct, Reply::Variable),
        Some("direct-empty") => ("direct-empty", CallPath::Direct, Reply::Empty),
        Some(variant) => panic!("unknown Bashkit benchmark variant: {variant}"),
    };
    let guest = GUEST.replace("__CALL_BODY__", reply.call_body());
    let runtime = Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("build Tokio runtime");
    let info = Info {
        key,
        language: "Shell",
        crate_name: "bashkit",
        version: bashkit::VERSION,
        call_kind: match call_path {
            CallPath::Event => "async",
            CallPath::Direct => "sync",
        },
        limits_enabled: format!(
            "max_work_units={MAX_WORK_UNITS}, max_commands/max_loop_iterations/\
             max_total_loop_iterations={MAX_CALLS}, timeout=600s, session max_total_commands=\
             {MAX_CALLS}, session max_exec_calls=20; all other ExecutionLimits, MemoryLimits, \
             filesystem quotas and default-deny network policy remain at defaults"
        ),
        notes: vec![
            "Session = Bash::builder() with the normal virtual root filesystem, default features, raised \
             benchmark fuel limits, and one host builtin; then exec(\"echo $((1 + 1))\")."
                .into(),
            "The Tokio current-thread runtime is host infrastructure shared by every session and is created \
             before the measured phases."
                .into(),
            match call_path {
                CallPath::Event => "The builtin suspends execution through ExecutionHandle and the host resumes it.",
                CallPath::Direct => "The builtin calls the same Rust reply handler inline with no suspension or event bridge.",
            }.into(),
            match reply {
                Reply::Json => "The guest captures the JSON reply with $(...) and validates its ID with a regex and BASH_REMATCH.",
                Reply::Text => "The guest captures the plain-text ID:length reply with $(...) and checks it with string comparison.",
                Reply::Variable => "The builtin writes ID:length directly to a shell variable, which the guest checks with string comparison.",
                Reply::Empty => "The builtin reads the ID and string arguments and returns no payload; the guest performs no reply validation.",
            }.into(),
            "The loaded shell function runs once for calibration and once for all warm-up and measured calls. \
             Timing starts at the first measured host call and records batches of 1,000 calls."
                .into(),
        ],
    };
    run_owned(
        info,
        || new_session(&runtime, call_path, reply),
        |session| {
            let result = exec(&runtime, session, &guest);
            assert_eq!(result.exit_code, 0, "{}", result.stderr);
        },
        |session, start, count| {
            let script = format!("run {start} {count}");
            let result = match call_path {
                CallPath::Event => exec_host_calls(&runtime, session, script, reply),
                CallPath::Direct => exec(&runtime, session, &script),
            };
            assert_eq!(result.exit_code, 0, "{}", result.stderr);
            assert!(
                result.stdout.is_empty(),
                "unexpected stdout: {}",
                result.stdout
            );
        },
    );
}
