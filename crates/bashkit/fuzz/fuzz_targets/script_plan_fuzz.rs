//! Bounded, structured compiler/AST differential fuzzing. All operators and
//! loop/branch forms are in the native subset, so random bytes reach lowering
//! and execution instead of mostly producing shell parse errors.
#![no_main]

use bashkit::{Bash, ExecOptions, ExecutionExtensions, ExperimentalPlanBenchmarkControl as Mode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut script = format!(
        "a={}; b={}; for ((i=0;i<{};i++)); do if ((i&1)); then\n",
        data.first().copied().unwrap_or(0) as i8,
        data.get(1).copied().unwrap_or(1) as i8,
        data.get(2).copied().unwrap_or(2) % 8 + 1,
    );
    // Independent of the loop counter; no input can turn this into an
    // unbounded loop. Small decimal operands avoid Bash's octal-variable gap.
    for byte in data.iter().take(32).copied().chain(std::iter::once(1)) {
        let op = ["+", "-", "*", "^", "|", "&", "<<", ">>"][usize::from(byte & 7)];
        let name = if byte & 8 == 0 { "a" } else { "b" };
        script.push_str(&format!("(({name}={name}{op}{}));\n", (byte >> 4) + 1));
    }
    script.push_str("else ((a+=b)); fi; done; printf '%s %s %s %s [%s]\\n' \"$a\" \"$b\" \"$i\" \"$?\" \"${PIPESTATUS[*]}\"");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        bashkit::testing::fuzz_init();
        let mut results = Vec::new();
        for mode in [Mode::Auto, Mode::InterpreterOnly] {
            let mut bash = Bash::new();
            let options = ExecOptions::new().extensions(ExecutionExtensions::new().with(mode));
            let result = bash.exec_with_options(&script, options).await.unwrap();
            bashkit::testing::assert_fuzz_invariants(&result, &script, "script_plan_fuzz", &[]);
            results.push((
                result.stdout,
                result.stderr,
                result.exit_code,
                result.control_flow,
            ));
            // Request-local plans/selectors must not corrupt subsequent use.
            let next = bash.exec("printf reusable").await.unwrap();
            assert_eq!(next.stdout, "reusable");
            assert_eq!(next.exit_code, 0);
        }
        assert_eq!(results[0], results[1], "compiler/AST mismatch: {script}");
    });
});
