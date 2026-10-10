//! Quote-adjacent backticks: Bash parity and permission/budget invariants.

use bashkit::hooks::{HookAction, ToolEvent};
use bashkit::{Bash, CommandContext, ExecutionLimits};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn backticks_after_quotes_match_bash() {
    let cases = [
        r#"printf '[%s]\n' 'x'`printf '1 2'`; echo tail"#,
        r#"printf '[%s]\n' "x"`printf '1 2'`"z"; echo tail"#,
        r#"printf '[%s]\n' ''`printf ''`; echo tail"#,
        r#"printf '[%s]\n' 'x'`printf ''`'y'"#,
        r#"printf '[%s]\n' 'x'`printf 'A\n\n'`'y'"#,
        r#"printf '[%s]\n' 'x'`printf A`'y'`printf B`'z'"#,
        r#"printf '[%s]\n' 'λ='`printf 猫`'終'"#,
        r#"v=outside; printf '[%s]\n' '$v'`printf Y`"#,
        r#"v='a b'; printf '[%s]\n' "$v"`printf '1 2'`"#,
        r#"IFS=:; printf '[%s]\n' 'x'`printf '1:2'`"#,
        r#"printf '[%s]\n' '`literal`'`printf ok`"#,
        r#"printf '[%s]\n' 'x'\`'y'"#,
        r#"printf '[%s]\n' 'x'`printf '%s' \`printf Y\``"#,
        "printf '[%s]\\n' 'x'`printf '\\nY'`\necho tail",
        r#"printf '[%s]\n' '$()*'`printf ''`"#,
        r#"printf '[%s]\n' $'*\x1f'`printf ''`"#,
        r#"v=''; printf '<%s>\n' ''$v "$v"$v"#,
        r#"f() { printf '[%s]\n' 'x'`printf Y`; }; f"#,
    ];
    let host_available = std::process::Command::new("bash")
        .arg("--version")
        .output()
        .is_ok();
    for script in cases {
        if !host_available {
            eprintln!("skip: Bash is not installed");
            return;
        }
        // All oracle scripts use shell builtins and variables only: no host I/O.
        let host = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-c", script])
            .env_clear()
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        let actual = Bash::new().exec(script).await.unwrap();
        assert_eq!(
            actual.exit_code,
            host.status.code().unwrap_or(-1),
            "{script}: {}",
            actual.stderr
        );
        assert_eq!(actual.stdout.as_bytes(), host.stdout, "{script}");
        assert!(actual.stderr.is_empty(), "{script}: {}", actual.stderr);
        assert!(host.stderr.is_empty(), "{script}: {:?}", host.stderr);
    }
}

#[tokio::test]
async fn backticks_after_quotes_execute_once_and_glob_only_unquoted_output() {
    let mut bash = Bash::new();
    let result = bash.exec(r#"touch /tmp/p-a /tmp/p-b; printf '[%s]\n' '/tmp/p-'`printf '*'`; printf '[%s]\n' '/tmp/p-*'`printf ''`; echo 'x'`echo ran >> /tmp/calls; printf Y`; cat /tmp/calls"#).await.unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(
        result.stdout,
        "[/tmp/p-a]\n[/tmp/p-b]\n[/tmp/p-*]\nxY\nran\n"
    );
    assert!(result.stderr.is_empty());
}

#[test]
fn backticks_after_quotes_are_visible_to_analysis() {
    for script in [
        r#"echo 'x'`rm /tmp/sentinel`"#,
        r#"echo "x"`rm /tmp/sentinel`"#,
    ] {
        let a = Bash::new().analyze(script).unwrap();
        assert!(a.has_command_substitution, "{script}");
        let rm = a
            .commands
            .iter()
            .find(|c| c.name.as_deref() == Some("rm"))
            .expect("substitution must be reported");
        assert_eq!(rm.context, CommandContext::Substitution);
        let echo = a
            .commands
            .iter()
            .find(|c| c.name.as_deref() == Some("echo"))
            .unwrap();
        assert_eq!(echo.args, vec![None]);
    }
    let a = Bash::new().analyze(r#"'ec'`printf ho` hello"#).unwrap();
    assert!(a.has_dynamic_commands);
    assert!(a.is_opaque());
}

#[tokio::test]
async fn backticks_after_quotes_still_obey_dispatch_veto() {
    let dispatched = Arc::new(Mutex::new(Vec::new()));
    let sink = dispatched.clone();
    let mut bash = Bash::builder()
        .before_tool(Box::new(move |event: ToolEvent| {
            sink.lock().unwrap().push(event.name.clone());
            if event.name == "rm" {
                HookAction::Cancel("rm denied".into())
            } else {
                HookAction::Continue(event)
            }
        }))
        .build();
    bash.exec("echo kept > /tmp/sentinel").await.unwrap();
    let a = bash.analyze(r#"echo 'x'`rm /tmp/sentinel`"#).unwrap();
    assert!(a.command_names().contains(&"rm"));
    let _ = bash.exec(r#"echo 'x'`rm /tmp/sentinel`"#).await.unwrap();
    assert!(dispatched.lock().unwrap().contains(&"rm".to_string()));
    let result = bash.exec("cat /tmp/sentinel").await.unwrap();
    assert_eq!(result.stdout, "kept\n");
}

#[test]
fn unterminated_backticks_after_quotes_fail_analysis() {
    for script in [
        "echo 'x'`printf Y",
        "echo \"x\"`printf Y",
        "echo 'x'`printf Y`'unclosed",
    ] {
        assert!(Bash::new().analyze(script).is_err(), "{script}");
    }
}

#[tokio::test]
async fn backticks_after_quotes_cannot_refresh_execution_budget() {
    let limits = ExecutionLimits::new()
        .max_commands(100)
        .max_work_units(6)
        .max_aggregate_input_bytes(1_000);
    let mut bash = Bash::builder().limits(limits).build();
    let result = bash.exec(r#"echo 'x'`echo $(echo $(echo nested))`"#).await;
    assert!(result.is_err(), "budget must abort execution: {result:?}");
    assert!(result.unwrap_err().to_string().contains("budget"));
}

#[tokio::test]
async fn empty_substitutions_do_not_activate_quoted_globs() {
    let mut bash = Bash::new();
    bash.exec("touch /tmp/p-a /tmp/p-b").await.unwrap();
    for word in [
        r#"'/tmp/p-*'`printf ''`"#,
        r#""/tmp/p-*"`printf ''`"#,
        r#"'/tmp/p-*'$(printf '')"#,
        r#""/tmp/p-*"$(printf '')"#,
        r#""$prefix"$empty"#,
    ] {
        let script = format!("prefix='/tmp/p-*'; empty=; printf '[%s]\\n' {word}");
        let r = bash.exec(&script).await.unwrap();
        assert_eq!(r.exit_code, 0, "{script}: {}", r.stderr);
        assert_eq!(r.stdout, "[/tmp/p-*]\n", "{script}");
        assert!(r.stderr.is_empty());
    }
}

#[tokio::test]
async fn mixed_quote_patterns_keep_unquoted_expansions_active() {
    for script in [
        r#"v='*'; case aa in 'a'$v) echo yes;; *) echo no;; esac"#,
        r#"v='*'; case '*abc' in '*'$v) echo yes;; *) echo no;; esac"#,
        r#"v='*'; case '*abc' in '*'`printf '%s' "$v"`) echo yes;; *) echo no;; esac"#,
        r#"v='*'; case '*abc' in "$v"$(printf '*')) echo yes;; *) echo no;; esac"#,
    ] {
        let r = Bash::new().exec(script).await.unwrap();
        assert_eq!(r.stdout, "yes\n", "{script}");
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
    }
}

#[tokio::test]
async fn invalid_backtick_body_after_quotes_fails_at_execution_only() {
    let r = Bash::new()
        .exec(r#"v='x'`fi`; printf '[%s] %s\n' "$v" "$?"; echo tail"#)
        .await
        .unwrap();
    assert_eq!(r.stdout, "[x] 2\ntail\n");
    assert_eq!(r.exit_code, 0);
    assert!(r.stderr.contains("syntax error"), "{}", r.stderr);
}
