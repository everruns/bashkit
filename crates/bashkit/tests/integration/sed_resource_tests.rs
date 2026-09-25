//! TM-DOS-112: sed's output sinks grow inside the builtin, so they are charged
//! to the shared live-intermediate budget before allocating.
//!
//! The stdout *capture* cap is deliberately not a sed sink limit: sed output is
//! routinely piped into another command or redirected to a file, neither of
//! which is captured stdout. Bounding the sinks by that cap truncated ordinary
//! transformations, so these tests pin both halves of the contract — the
//! amplification is refused, and below-budget output is byte-for-byte intact.

use bashkit::{Bash, ExecutionLimits};

/// `r FILE` re-reads the whole file after every input line, so output grows
/// quadratically inside the sed engine. The live-intermediate budget must refuse
/// it rather than letting the allocation abort the host.
///
/// The refusal can surface either as sed's own `sed: <error>` result or, when a
/// downstream stage of the pipeline reaches the shared budget first, as a
/// top-level resource-limit error. Which layer trips first is an implementation
/// detail; what this pins is that the amplification is always refused, with a
/// bounded diagnostic and a nonzero status, and never aborts the process.
#[tokio::test]
async fn repeated_read_file_stops_at_live_intermediate_budget() {
    let limits = ExecutionLimits::new().max_live_intermediate_bytes(1 << 20);
    let mut bash = Bash::builder().limits(limits).build();

    // 2000 lines × 32 B = 64 KB of input; `r` re-emits all of it per line,
    // so the sink would need ~128 MB against a 1 MB budget.
    let script = "for i in $(seq 1 2000); do printf 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\\n'; \
                  done > /in.txt; sed 'r /in.txt' /in.txt | wc -c";

    match bash.exec(script).await {
        Ok(result) => {
            assert_ne!(result.exit_code, 0, "stdout: {}", result.stdout);
            assert!(result.stderr.contains("sed:"), "stderr: {}", result.stderr);
            assert!(result.stderr.len() < 1024, "diagnostic must stay bounded");
        }
        Err(error) => {
            let rendered = error.to_string();
            assert!(
                rendered.contains("live intermediate bytes"),
                "unexpected error: {rendered}"
            );
        }
    }
}

#[tokio::test]
async fn read_stdin_preserves_output_below_limit() {
    let limits = ExecutionLimits::new().max_live_intermediate_bytes(64 * 1024);
    let mut bash = Bash::builder().limits(limits).build();

    let result = bash
        .exec("printf 'a\\nb\\n' | sed 'r /dev/stdin'")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a\na\nb\nb\na\nb\n");
    assert!(result.stderr.is_empty());
}

/// Regression for #2455: sed output larger than `max_stdout_bytes` is an
/// ordinary transformation when it is piped onward, not a resource failure.
/// The stdout cap governs what the interpreter *captures*, not sed's sink.
#[tokio::test]
async fn output_over_stdout_cap_still_flows_through_a_pipe() {
    let limits = ExecutionLimits::new()
        .max_stdout_bytes(4096)
        .max_live_intermediate_bytes(32_000_000);
    let mut bash = Bash::builder().limits(limits).build();

    // ~65 KB out of sed — far over the 4 KB capture cap, far under the budget.
    let result = bash
        .exec("seq 1 2000 | sed 's/$/-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx/' | wc -c")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), "70893");
    assert!(result.stderr.is_empty(), "stderr: {}", result.stderr);
}

/// Same shape, but the output lands in a VFS file. The stdout capture cap must
/// not bound a file write either.
#[tokio::test]
async fn output_over_stdout_cap_still_writes_a_file() {
    let limits = ExecutionLimits::new()
        .max_stdout_bytes(4096)
        .max_live_intermediate_bytes(32_000_000);
    let mut bash = Bash::builder().limits(limits).build();

    let result = bash
        .exec(
            "seq 1 2000 | sed 's/$/-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx/' > /out.txt; \
             wc -c < /out.txt",
        )
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), "70893");
}
