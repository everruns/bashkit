use bashkit::Bash;

#[tokio::test]
#[cfg_attr(not(unix), ignore = "compares against host Unix bash")]
async fn mixed_functions_and_loops_match_bash() {
    for source in [
        // The original tool benchmark's JSON/substitution/regex shape, using
        // a shell fixture so exactly the same script runs in both engines.
        r#"call_tool() { printf '{"id":%s,"ok":true}\n' "$1"; }
run() {
    local start=$1 count=$2 id reply
    local end=$((start + count))
    for ((id=start;id<end;id++)); do
        reply=$(call_tool "$id" "hello from the sandbox") || return
        [[ $reply =~ \"id\":([0-9]+) ]] || return 3
        ((BASH_REMATCH[1] == id)) || return 4
    done
    printf '%s\n' "$id"
}
run 1 4; run 10 2"#,
        "f() { local i; for ((i=0;i<4;i++)); do printf '%s' \"$i\"; if ((i==2)); then return 7; fi; done; }; f; printf ':%s\\n' \"$?\"",
        "f() { for ((i=0;i<2;i++)); do printf old; f() { printf new; }; done; }; f; f",
        "set -e; f() { for ((i=0;i<3;i++)); do printf x; false; printf still; done; }; f || printf caught",
        "f() { local i; for ((i=0;i<3;i++)); do if ((i==1)); then declare -i i; fi; printf '%s' \"$i\"; done; }; f",
        "f() { local i=0; while ((i<4)); do ((i++)); if ((i==2)); then continue; fi; printf '%s' \"$i\"; done; until ((i==6)); do ((i++)); printf '%s' \"$i\"; done; }; f",
        "f() { for ((i=0;i<3;i++)); do for j in 1 2 3; do printf '%s%s' \"$i\" \"$j\"; break 2; done; done; }; f",
    ] {
        let expected = std::process::Command::new("bash")
            .env_clear()
            .args(["--noprofile", "--norc", "-c", source])
            .output()
            .unwrap();
        let mut bash = Bash::new();
        let actual = bash.exec(source).await.unwrap();
        assert_eq!(
            actual.exit_code,
            expected.status.code().unwrap(),
            "{source}"
        );
        assert_eq!(actual.stdout.as_bytes(), expected.stdout, "{source}");
        assert_eq!(actual.stderr.as_bytes(), expected.stderr, "{source}");
    }
}

#[tokio::test]
async fn mixed_function_capture_limits_preserve_substitution_and_redirect_consumers() {
    let mut bash = Bash::builder()
        .limits(
            bashkit::ExecutionLimits::new()
                .max_stdout_bytes(7)
                .max_stderr_bytes(5),
        )
        .build();
    let result = bash
        .exec(
            r#"
f() { local i; for ((i=0;i<20;i++)); do printf é; printf err >&2; done; }
f
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        result.stdout.as_bytes(),
        &[0xc3, 0xa9, 0xc3, 0xa9, 0xc3, 0xa9, 0xc3]
    );
    assert_eq!(result.stderr, "errer");
    assert!(result.stdout_truncated && result.stderr_truncated);
    let result = bash
        .exec(
            r#"
x=$(f 2>/dev/null)
f >/out 2>/err
printf '%s:%s:%s' "${#x}" "$(wc -c </out)" "$(wc -c </err)"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "20:40:6");
    assert!(result.stdout_truncated);
    assert!(result.stderr.is_empty());
}
