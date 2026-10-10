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
