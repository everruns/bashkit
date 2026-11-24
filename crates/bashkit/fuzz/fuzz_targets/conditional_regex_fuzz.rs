//! Compare cold, repeated and switched conditional-regex searches (TM-DOS-023).
//! Input is a UTF-8 pattern and haystack separated by a newline. Both are
//! assigned as quoted data, so mutations cannot inject shell syntax.
#![no_main]

use libfuzzer_sys::fuzz_target;

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    if input.len() > 512 || input.contains('\0') || bashkit::testing::input_echo_would_trip(input) {
        return;
    }
    // Exercise searches even when the initial corpus has no newline yet.
    let (pattern, haystack) = input.split_once('\n').unwrap_or((input, "id=123 é😀 ab"));
    let setup = format!("re={}; value={};", quote(pattern), quote(haystack));
    let query = r#"[[ $value =~ $re ]]; printf '%s:%s\n' "$?" "${#BASH_REMATCH[@]}"; printf '<%s>' "${BASH_REMATCH[@]}";"#;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let mut bash = bashkit::Bash::builder()
            .limits(
                bashkit::ExecutionLimits::new()
                    .max_commands(100)
                    .timeout(std::time::Duration::from_secs(2)),
            )
            .build();
        let cold_script = format!("{setup}{query}");
        let cold = bash.exec(&cold_script).await.unwrap();
        bashkit::testing::assert_fuzz_invariants(
            &cold,
            &cold_script,
            "conditional_regex_fuzz",
            &[],
        );
        // First two searches reuse scratch; the third follows a different
        // pattern with another capture layout. A new exec also resets state.
        let warm_script =
            format!("{setup}{query}{query} [[ sentinel =~ ^(s)(entinel)$ ]]; {query}");
        let warm = bash.exec(&warm_script).await.unwrap();
        bashkit::testing::assert_fuzz_invariants(
            &warm,
            &warm_script,
            "conditional_regex_fuzz",
            &[],
        );
        assert_eq!(warm.exit_code, cold.exit_code);
        assert_eq!(warm.stdout, cold.stdout.repeat(3));
        assert_eq!(warm.stderr, cold.stderr.repeat(3));
    });
});
