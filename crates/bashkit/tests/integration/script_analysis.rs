//! End-to-end behavior of `Bash::analyze`, focused on what a host permission
//! gate depends on: evasion attempts must surface as *unknown*, never as
//! *safe*, and analysis must agree with what execution actually dispatches.
//!
//! Unit-level coverage of each AST node kind lives in `src/analysis.rs`.

use bashkit::hooks::{HookAction, ToolEvent};
use bashkit::{Bash, CommandContext, ScriptAnalysis};
use std::sync::{Arc, Mutex};

fn analyze(script: &str) -> ScriptAnalysis {
    Bash::new().analyze(script).expect("script should parse")
}

/// Minimal host gate: allow only allowlisted commands in a fully transparent
/// script. Mirrors the pattern documented in `docs/script-analysis.md`.
fn allowed(script: &str, allowlist: &[&str]) -> bool {
    let Ok(analysis) = Bash::new().analyze(script) else {
        return false;
    };
    if analysis.is_opaque() {
        return false;
    }
    analysis.commands.iter().all(|c| {
        c.is_assignment_only() || c.name.as_deref().is_some_and(|n| allowlist.contains(&n))
    })
}

const READ_ONLY: &[&str] = &["cat", "grep", "ls", "echo", "wc", "head"];

#[test]
fn allows_a_transparent_read_only_script() {
    assert!(allowed("cat notes.txt | grep -i todo | wc -l", READ_ONLY));
    assert!(allowed("N=3 ; ls | head -n 3", READ_ONLY));
}

#[test]
fn rejects_a_command_outside_the_allowlist() {
    assert!(!allowed("rm -rf /data", READ_ONLY));
    assert!(!allowed("cat a && rm -rf /data", READ_ONLY));
}

#[test]
fn rejects_commands_hidden_in_a_substitution() {
    assert!(!allowed("echo $(rm -rf /data)", READ_ONLY));
    assert!(!allowed("echo `rm -rf /data`", READ_ONLY));
    assert!(!allowed("cat <(rm -rf /data)", READ_ONLY));
    assert!(!allowed("time -f \"$(rm -rf /data)\" echo safe", READ_ONLY));
    assert!(!allowed("time -o \"$(rm -rf /data)\" echo safe", READ_ONLY));
}

#[test]
fn rejects_commands_hidden_in_a_loop_or_branch() {
    assert!(!allowed("if true; then rm -rf /data; fi", READ_ONLY));
    assert!(!allowed("for f in a b; do rm $f; done", READ_ONLY));
    assert!(!allowed("while read l; do rm $l; done", READ_ONLY));
    assert!(!allowed("case x in a) rm y ;; esac", READ_ONLY));
}

#[test]
fn rejects_commands_hidden_in_a_function_body() {
    assert!(!allowed("cleanup() { rm -rf /data; }; cleanup", READ_ONLY));
}

#[test]
fn rejects_dynamic_dispatch() {
    for script in [
        "$cmd -rf /data",
        "c=rm; $c -rf /data",
        "$(echo rm) -rf /data",
        "${cmds[0]} -rf /data",
    ] {
        assert!(!allowed(script, READ_ONLY), "{script} must not be allowed");
    }
}

#[test]
fn rejects_eval_and_source() {
    assert!(!allowed("eval \"$payload\"", READ_ONLY));
    assert!(!allowed("source /tmp/setup.sh", READ_ONLY));
    assert!(!allowed(". /tmp/setup.sh", READ_ONLY));
}

#[test]
fn rejects_shell_re_entry() {
    // `bash -c` hands a whole script back to the interpreter; the payload's
    // commands are invisible to this analysis even when it is a literal.
    assert!(!allowed("bash -c 'rm -rf /data'", READ_ONLY));
    assert!(!allowed("sh -c \"$payload\"", READ_ONLY));
    assert!(analyze("bash -c 'cat x'").has_interpreter_reentry);
    // A script file hides its commands just as well as inline `-c` text.
    assert!(!allowed("bash /tmp/setup.sh", READ_ONLY));
    assert!(analyze("bash /tmp/setup.sh").has_interpreter_reentry);
}

#[test]
fn rejects_unparseable_scripts() {
    assert!(!allowed("if true; then", READ_ONLY));
    assert!(!allowed("echo 'unterminated", READ_ONLY));
}

#[test]
fn analysis_covers_every_command_the_interpreter_dispatches() {
    // The `before_tool` hook sees what actually ran; for a fully static script
    // the two must agree.
    let dispatched = Arc::new(Mutex::new(Vec::new()));
    let sink = dispatched.clone();

    let mut bash = Bash::builder()
        .before_tool(Box::new(move |event: ToolEvent| {
            sink.lock().expect("lock").push(event.name.clone());
            HookAction::Continue(event)
        }))
        .build();

    let script = "echo one > /tmp/a; cat /tmp/a | grep one; echo $(basename /x/y)";
    let analysis = bash.analyze(script).expect("parses");

    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(bash.exec(script))
        .expect("exec");
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);

    let ran = dispatched.lock().expect("lock").clone();
    for name in &ran {
        assert!(
            analysis.command_names().contains(&name.as_str()),
            "`{name}` ran but analysis did not report it (reported {:?})",
            analysis.command_names(),
        );
    }
    assert!(ran.contains(&"basename".to_string()));
}

#[test]
fn hook_catches_what_analysis_cannot_resolve() {
    let mut bash = Bash::builder()
        .before_tool(Box::new(|event: ToolEvent| {
            if event.name == "rm" {
                return HookAction::Cancel("rm is not permitted".into());
            }
            HookAction::Continue(event)
        }))
        .build();

    let script = "c=rm; $c -rf /data";
    let analysis = bash.analyze(script).expect("parses");
    assert!(analysis.has_dynamic_commands, "name is not static");
    assert!(analysis.is_opaque());

    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(bash.exec(script))
        .expect("exec");
    assert_ne!(result.exit_code, 0, "hook must block the resolved command");
}

#[test]
fn analyze_does_not_execute_or_mutate_state() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let mut bash = Bash::new();

    let analysis = bash
        .analyze("mkdir -p /tmp/analyzed && echo marker > /tmp/analyzed/f")
        .expect("parses");
    assert_eq!(analysis.command_names(), ["mkdir", "echo"]);

    // Nothing ran: the path does not exist and the shell has no new variables.
    let result = rt
        .block_on(bash.exec("test -e /tmp/analyzed; echo $?"))
        .expect("exec");
    assert_eq!(result.stdout.trim(), "1");

    let analysis = bash.analyze("FOO=bar").expect("parses");
    assert!(analysis.commands[0].is_assignment_only());
    let result = rt.block_on(bash.exec("echo \"[$FOO]\"")).expect("exec");
    assert_eq!(result.stdout.trim(), "[]");
}

#[test]
fn analyze_honors_instance_parser_limits() {
    let strict = Bash::builder()
        .limits(bashkit::ExecutionLimits::default().max_parser_operations(3))
        .build();
    assert!(strict.analyze("echo a; echo b; echo c").is_err());

    let relaxed = Bash::new();
    assert!(relaxed.analyze("echo a; echo b; echo c").is_ok());
}

#[test]
fn analyze_rejects_oversized_input_like_exec() {
    let bash = Bash::builder()
        .limits(bashkit::ExecutionLimits::default().max_input_bytes(16))
        .build();
    assert!(bash.analyze("echo hi").is_ok());
    assert!(bash.analyze(&format!("echo {}", "x".repeat(100))).is_err());
}

#[test]
fn permission_keys_from_a_custom_builtin_cli() {
    // The eidos-style case: gate on `<tool> <resource> <action>`, not on the
    // tool name alone.
    fn key(script: &str) -> Option<String> {
        let analysis = Bash::new().analyze(script).ok()?;
        let command = analysis.commands_named("mydata").next()?;
        let resource = command.args.first()?.as_deref()?;
        let action = command.args.get(1)?.as_deref()?;
        match action {
            "list" | "get" | "query" => None,
            _ => Some(format!("mydata:{resource}:{action}")),
        }
    }

    assert_eq!(key("mydata record query 42"), None);
    assert_eq!(
        key("mydata record delete 42").as_deref(),
        Some("mydata:record:delete")
    );
    assert_eq!(
        key("cat x | mydata doc write 7").as_deref(),
        Some("mydata:doc:write")
    );
    // A computed action is not a key — the host must fall back to prompting.
    assert_eq!(key("mydata record $action"), None);
    assert!(
        Bash::new()
            .analyze("mydata record $action")
            .expect("parses")
            .commands[0]
            .literal_args()
            .is_none()
    );
}

#[test]
fn contexts_separate_what_runs_now_from_what_may_run_later() {
    let analysis = analyze("helper() { curl http://x; }\nls -la");
    let direct: Vec<_> = analysis
        .commands
        .iter()
        .filter(|c| c.context == CommandContext::Direct)
        .filter_map(|c| c.name.as_deref())
        .collect();
    assert_eq!(direct, ["ls"]);
    assert_eq!(analysis.functions, ["helper"]);
    assert!(analysis.command_names().contains(&"curl"));
}

#[test]
fn large_script_truncates_and_stays_opaque() {
    let script = "echo x;".repeat(6000);
    let analysis = analyze(&script);
    assert!(analysis.truncated);
    assert!(analysis.is_opaque());
    assert!(
        !allowed(&script, READ_ONLY),
        "truncated must not be allowed"
    );
}

#[test]
fn analysis_round_trips_through_json() {
    let analysis = analyze("cat a > b");
    let json = serde_json::to_string(&analysis).expect("serialize");
    let back: ScriptAnalysis = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, analysis);
}

/// A command substitution whose body is a syntax error must not be dropped:
/// dropping it splices the surrounding literals into a command name that
/// appears nowhere in the source (`a$(|)b` -> `ab`), which would then be shown
/// to a host permission gate. Real bash rejects the script outright, so
/// `analyze` must fail rather than invent a name.
#[test]
fn malformed_syntax_never_invents_a_command_name() {
    for script in [
        "a$(|)b",
        "a$(&&)b",
        "x$(;)y",
        "echo a$(|)b",
        "<<\u{1c}<~ \u{1c}& \u{1c}{{",
    ] {
        match Bash::new().analyze(script) {
            Err(_) => {}
            Ok(analysis) => {
                for command in &analysis.commands {
                    if let Some(name) = command.name.as_deref() {
                        assert!(
                            script.contains(name),
                            "`{script}` produced command name `{name}`, absent from the source"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn malformed_literal_boundaries_are_rejected_by_analysis() {
    for script in [
        "\u{3}\0J",
        "$\0$",
        "x\u{1}y",
        "x\u{2}y",
        "\r\u{1e}e",
        "J\u{1f}J\u{1f}",
        "0\u{1f}\u{8}",
        "z'A",
        "z\"A",
        "z$'A",
        "z$\"A",
    ] {
        assert!(
            Bash::new().analyze(script).is_err(),
            "malformed input unexpectedly analyzed: {script:?}"
        );
    }
}

#[test]
fn valid_quote_and_escape_removal_can_join_command_name_spans() {
    for (script, expected) in [("u\"3\"", "u3"), ("'#'g", "#g"), (r"!\[[", "![[")] {
        let analysis = Bash::new().analyze(script).expect("valid script");
        assert_eq!(analysis.command_names(), [expected]);
        assert!(!script.contains(expected));
    }
}
