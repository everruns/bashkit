// Scaffold tests for the arithmetic_fuzz target.
//
// The target wraps its input in `echo $((...))` and asserts the fuzz
// invariants: no panic, and stderr that neither leaks Debug shapes/host paths
// nor runs past `bashkit::testing::MAX_STDERR_BYTES`.
//
// An arithmetic error names the expression and then the unparsed rest as the
// "error token". Both come from the script, so an expression whose first
// rejected character sits near the front is echoed back roughly twice --
// nightly fuzz run 270 turned 507 bytes of input into 1,076 bytes of stderr.
// `Interpreter::MAX_ARITHMETIC_DIAG_ECHO` bounds each fragment (L-ARITH-002).

use bashkit::testing::{fuzz_exec, fuzz_init};
use bashkit::{Bash, ExecutionLimits};

/// The limits `fuzz_targets/arithmetic_fuzz.rs` builds.
fn fuzz_bash() -> Bash {
    fuzz_init();
    Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(100)
                .max_function_depth(10)
                .max_subst_depth(5)
                .max_stdout_bytes(4096)
                .max_stderr_bytes(4096)
                .timeout(std::time::Duration::from_millis(100)),
        )
        .build()
}

async fn fuzz_arith(expr: &str, ctx: &str) {
    let mut bash = fuzz_bash();
    fuzz_exec(&mut bash, &format!("echo $(({expr}))"), ctx, &[]).await;
}

/// The exact crash input from nightly fuzz run 270.
const RUN_270_CRASH: &str = "+---+~~~~~~~~~~~#~~~~~~~~ech~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~#~~~~~~~~ech~~\
     ~~~~~~~~~~~~~~~~~~~~~u~~~~~~~~~~|~~_LOWER_~~~~~~~~~~~~~~~~~~~~#~~~~~~~~e\
     ch~~~~~~~~~~~~~~~~~~~~~~~u~~~~~~~~~~|~~_LOWER_~~~~~~~~~~~~~~~~~~~~~~~~~~\
     ~~~~u~~~~~~~~~~~~~~~~~~~~~u~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~u~~~~~~~~~~\
     ~~~~~~~~--~~~u~~~~~~~~~~|~~_LOWER_~~~~~~~~~~~~~~~~~~~~#~~~~~~~~ech~~~~~~\
     ~~~~~~~~~~~~~~~~~u~~~~~~~~~~|~~_LOWER_~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~u~~~\
     ~~~~~~~~~~~~~~~~~~u~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~u~~~~~~~~~~~~~~~~~~\
     ---";

#[tokio::test]
async fn run_270_crash_input_is_bounded() {
    // The literal is line-continued above; check it still reassembles to the
    // 507 bytes libFuzzer reported, so a reflow cannot quietly shrink it.
    assert_eq!(RUN_270_CRASH.len(), 507);
    fuzz_arith(RUN_270_CRASH, "run_270_crash_input").await;
}

#[tokio::test]
async fn long_rejected_operator_near_front() {
    // Reduced shape of the same bug: one rejected byte, a long tail.
    fuzz_arith(&format!("~#{}", "~".repeat(500)), "long_rejected_operator").await;
}

#[tokio::test]
async fn long_number_token() {
    fuzz_arith(&format!("1{}x", "9".repeat(600)), "long_number_token").await;
}

#[tokio::test]
async fn long_division_by_zero_token() {
    fuzz_arith(&format!("1/(0{})", "+0".repeat(400)), "long_division_token").await;
}

#[tokio::test]
async fn long_unary_run_hits_the_depth_limit() {
    fuzz_arith(&"~".repeat(500), "long_unary_run").await;
}

#[tokio::test]
async fn deeply_grouped_expression() {
    let depth = 200;
    fuzz_arith(
        &format!("{}1{}", "(".repeat(depth), ")".repeat(depth)),
        "deeply_grouped",
    )
    .await;
}

#[tokio::test]
async fn valid_expression_still_evaluates() {
    let mut bash = fuzz_bash();
    let r = bash.exec("echo $((2 + 3 * 4))").await.unwrap();
    assert_eq!(r.stdout, "14\n");
}

/// Run 271: the diagnostic echoes the script's own text, which happened to
/// contain the banned parser-token shape `Tok::`.
///
/// The tokenizer's `Tok` enum is never formatted -- this is the expression
/// coming back verbatim, exactly as real bash echoes it. The target now skips
/// such inputs at the input layer. Assert that, then assert the diagnostic is
/// still a faithful echo and still bounded, so the skip is hiding a false
/// positive rather than a real leak.
#[tokio::test]
async fn run_271_banned_shape_comes_from_the_input() {
    let input = ":A>>=::TTA:::Tok::::";
    assert!(bashkit::testing::input_echo_would_trip(input));

    let mut bash = fuzz_bash();
    let r = bash
        .exec(&format!("echo $(({input}))"))
        .await
        .expect("exec");
    let err = r.stderr.to_string();
    // The echo is the user's own text, not a Debug shape: it appears inside
    // the quoted error token, and only because the input put it there.
    assert!(
        err.contains(&format!("(error token is \"{input}\")")),
        "{err}"
    );
    assert!(err.len() <= 1024, "stderr is {} bytes:\n{err}", err.len());
}

/// A banned shape bashkit itself emitted would still be caught: the same
/// expression without the `Tok::` text produces a diagnostic with no banned
/// substring at all, so the filter above is not blanket-suppressing this path.
#[tokio::test]
async fn same_shape_without_the_banned_text_is_still_checked() {
    let input = ":A>>=::TTA:::X::::";
    assert!(!bashkit::testing::input_echo_would_trip(input));
    fuzz_arith(input, "no_banned_text").await;
}
