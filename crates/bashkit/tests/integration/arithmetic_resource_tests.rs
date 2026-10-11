// THREAT[TM-DOS-026]: textual expansion and subscript evaluation share limits.
use bashkit::Bash;

fn nested_array_expansion(levels: usize, length: bool) -> String {
    let mut expr = "0".to_string();
    for _ in 0..levels {
        expr = if length {
            format!("${{#a[{expr}]}}")
        } else {
            format!("${{a[{expr}]}}")
        };
    }
    expr
}

#[tokio::test]
async fn arithmetic_dollar_subscripts_stop_at_recursion_limit() {
    // Bounded input exercises the guard without probing a host crash threshold.
    for length in [false, true] {
        let mut bash = Bash::new();
        let expr = nested_array_expansion(51, length);
        let result = bash.exec(&format!("a=(0); let '{expr}'")).await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(
            result
                .stderr
                .contains("expression recursion level exceeded"),
            "{}",
            result.stderr
        );
        let recovery = bash.exec("echo recovered").await.unwrap();
        assert_eq!(recovery.stdout, "recovered\n");
        assert_eq!(recovery.exit_code, 0);
        assert!(recovery.stderr.is_empty());
    }
}

#[tokio::test]
async fn arithmetic_dollar_siblings_share_expansion_fuel() {
    let mut bash = Bash::new();
    let term = nested_array_expansion(10, false);
    // Source and individual terms fit. Aggregate recursive work exceeds the fuel.
    let expr = format!("{}+1", vec![term; 30].join("+"));
    assert!(expr.len() < 8192);
    let result = bash.exec(&format!("a=(0); let '{expr}'")).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(
        result
            .stderr
            .contains("expression recursion level exceeded")
    );
}

#[tokio::test]
async fn arithmetic_dollar_subscripts_match_bash_within_limits() {
    let expr = nested_array_expansion(4, false);
    // NOTE: subscript stays portable on purpose: macOS ships bash 3.2 with no
    // negative-index support while Linux CI has bash 5.x, so `${a[-1]}` would
    // make this differential assertion environment-dependent. `i-0` still
    // exercises arithmetic inside the subscript on every bash.
    let script = format!(
        "a=(0 7); n=123; i=1; echo $(({expr}+${{a[i]}})); \
         echo $((${{#a[1]}}+${{n%3}})); echo $((${{a[i-0]}}))"
    );
    let expected = std::process::Command::new("bash")
        .args(["--noprofile", "--norc", "-c", &script])
        .output()
        .unwrap();
    assert!(expected.status.success());
    let mut bash = Bash::new();
    let result = bash.exec(&script).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stderr.is_empty(), "{}", result.stderr);
    assert_eq!(result.stdout.as_bytes(), expected.stdout);
}

#[tokio::test]
async fn arithmetic_quoted_let_keeps_shallow_dollar_expansion() {
    let mut bash = Bash::new();
    let expr = nested_array_expansion(4, false);
    let result = bash
        .exec(&format!("a=(0 7); let 'x={expr}+${{a[1]}}'; echo \"$x\""))
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "7\n");
    assert!(result.stderr.is_empty());
}

#[tokio::test]
async fn arithmetic_nameref_and_assoc_keys_cannot_restart_the_budget() {
    let expr = nested_array_expansion(51, false);
    for usage in [
        "$r",
        "${r}",
        "${#r}",
        "${m[$r]}",
        "${m[k$r]}",
        "${m[k${r}]}",
    ] {
        let mut bash = Bash::new();
        let result = bash
            .exec(&format!(
                "a=(0); declare -A m; declare -n r='a[{expr}]'; let '{usage}'"
            ))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 1, "{usage}");
        assert!(
            result
                .stderr
                .contains("expression recursion level exceeded"),
            "{usage}: {}",
            result.stderr
        );
    }
}

#[tokio::test]
async fn nested_arithmetic_dollars_share_fuel_with_subscripts() {
    let term = format!("$(({}))", nested_array_expansion(10, false));
    let expr = format!("{}+1", vec![term; 30].join("+"));
    let mut bash = Bash::new();
    let result = bash.exec(&format!("a=(0); let '{expr}'")).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(
        result
            .stderr
            .contains("expression recursion level exceeded")
    );
}

#[tokio::test]
async fn dollar_subscript_writes_remain_read_only() {
    let mut bash = Bash::new();
    let result = bash
        .exec("a=(0 7); i=0; let 'x=${a[i=1]}'; echo \"$i $x\"")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "0 7\n");
    assert!(result.stderr.is_empty());
}

#[tokio::test]
async fn arithmetic_array_keys_obey_live_budget_and_recover() {
    use bashkit::{Error, ExecutionLimits, LimitExceeded};
    for reference in ["$x", "${x}"] {
        for expr in ["c[KEY]=1", "c[KEY]+=1", "c[KEY]++", "--c[KEY]", "c[KEY]"] {
            for entry in ["((EXPR))", "let 'EXPR'", "echo $((EXPR))"] {
                let limits = ExecutionLimits::new().max_live_intermediate_bytes(512);
                let mut bash = Bash::builder().limits(limits).build();
                bash.exec(&format!("declare -A c; x={}", "A".repeat(64)))
                    .await
                    .unwrap();
                let expr = expr.replace("KEY", &reference.repeat(32));
                let script = entry.replace("EXPR", &expr);
                let result = bash.exec(&script).await;
                assert!(
                    matches!(
                        result,
                        Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
                    ),
                    "{script}: {result:?}"
                );
                let recovery = bash.exec("echo recovered; echo ${#c[@]}").await.unwrap();
                assert_eq!(recovery.stdout, "recovered\n0\n");
                assert_eq!(recovery.exit_code, 0);
            }
        }
    }
}

#[tokio::test]
async fn arithmetic_array_key_scanner_obeys_budgets() {
    use bashkit::{Error, ExecutionLimits, LimitExceeded};
    for expression in ["${c[k$x$x$x$x]}", "${c[k${x}${x}${x}${x}]}", "$r"] {
        for limits in [
            ExecutionLimits::new().max_live_intermediate_bytes(512),
            ExecutionLimits::new().max_work_units(400),
        ] {
            let mut bash = Bash::builder()
                .limits(limits)
                .env("x", "A".repeat(256))
                .build();
            bash.exec("declare -A c; declare -n r='c[k$x$x$x$x]'")
                .await
                .unwrap();
            let result = bash.exec(&format!("let '{expression}'")).await;
            assert!(
                matches!(
                    result,
                    Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
                ),
                "{expression}: {result:?}"
            );
            assert_eq!(
                bash.exec("echo recovered").await.unwrap().stdout,
                "recovered\n"
            );
        }
    }
}

#[tokio::test]
async fn arithmetic_array_key_size_boundary() {
    for size in [65_535, 65_536] {
        let mut bash = Bash::builder().env("x", "a".repeat(size)).build();
        bash.exec("declare -A c").await.unwrap();
        // The literal prefix makes the scanner's output exactly 64 KiB or
        // one byte over. No huge arithmetic source or host-exhaustion probe.
        let result = bash.exec("let '1+${c[k$x]:-0}'").await.unwrap();
        if size == 65_535 {
            assert_eq!(result.exit_code, 0, "{}", result.stderr);
            assert!(result.stderr.is_empty());
        } else {
            assert_eq!(result.exit_code, 1);
            assert!(result.stderr.contains("expression too long"));
        }
        assert_eq!(
            bash.exec("echo recovered").await.unwrap().stdout,
            "recovered\n"
        );
    }
    let mut bash = Bash::builder().env("x", "a".repeat(65_537)).build();
    // Inspecting length does not materialize that value as an expression/key.
    let result = bash.exec("echo $((${#x}+1))").await.unwrap();
    assert_eq!(result.stdout, "65538\n");
}

#[tokio::test]
async fn arithmetic_array_keys_match_modern_bash() {
    let version = std::process::Command::new("bash")
        .args(["--noprofile", "--norc", "-c", "echo ${BASH_VERSINFO[0]}"])
        .output()
        .unwrap();
    if String::from_utf8_lossy(&version.stdout)
        .trim()
        .parse::<u32>()
        .unwrap()
        < 4
    {
        // Associative arrays require Bash >= 4; Linux CI supplies Bash 5.x.
        return;
    }
    for script in [
        "declare -A c; x='a b'; ((c[$x$x]=3)); ((c[$x$x]+=2)); ((c[$x$x]++)); let 'c[$x$x]--'; echo $((c[$x$x])); echo ${c['a ba b']}",
        "declare -A c; x=é; ((c[${x}${x}]=7)); echo $((c[${x}${x}])); let 'c[éé]+=1'; echo ${c[éé]}",
        "declare -A c; c[abcabc]=7; x=abc; echo $((1+${c[k${x}${x}]:-2})); echo $((1+${c[$x$x]})); declare -n r='c[$x$x]'; echo $(($r))",
        "declare -A c; a=(a b); c[k'a b']=8; echo $((1+${c[k${a[@]}]}))",
    ] {
        let expected = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-c", script])
            .output()
            .unwrap();
        assert!(
            expected.status.success(),
            "{script}: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        let mut bash = Bash::new();
        let result = bash.exec(script).await.unwrap();
        assert_eq!(
            result.exit_code,
            expected.status.code().unwrap(),
            "{script}: {}",
            result.stderr
        );
        assert_eq!(result.stdout.as_bytes(), expected.stdout, "{script}");
        assert!(result.stderr.is_empty(), "{script}: {}", result.stderr);
    }
}
