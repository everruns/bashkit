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

/// Run 272: libFuzzer spliced the leak detector's own host canary into the
/// expression, and the arithmetic diagnostic echoed it straight back.
///
/// The canary bytes reach the fuzzer through the detector's own
/// `stderr.contains(FUZZ_HOST_CANARY)` comparison: libFuzzer recovered a
/// 32-byte `PersAutoDict` entry for it (`DE:
/// "BASHKIT_FUZZ_HOST_CANARY_47a83bc"`) and crossed the remaining bytes in
/// from the corpus. No builtin read host env -- the canary is in the output
/// only because the fuzzer put it in the script. Any target that echoes its
/// input verbatim can be driven into this, so the detector has to tell
/// "script explains it" from a real TM-INF-013 read.
#[tokio::test]
async fn run_272_canary_comes_from_the_input() {
    // Rebuilt from the canary constant so the two cannot drift apart.
    let input = format!(
        "**HOST_CARY:ARY_47a83bcf_DO_NOT_LHi&{}EAKa",
        bashkit::testing::FUZZ_HOST_CANARY
    );
    assert_eq!(input.len(), 85, "crash input is 85 bytes: {input}");

    // The target's gate admits it, so it really does reach exec: every byte
    // is one `bashkit_fuzz::is_arithmetic_expression` accepts outside its
    // grouping arms. (That crate is its own workspace, so mirror the set
    // rather than depend on it.)
    assert!(
        input
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_ \t+-*/%<>=!&|^~?:,#".contains(&b)),
        "input must pass the arithmetic gate: {input}"
    );

    // Pre-filter layer: the target skips the input before executing it.
    assert!(bashkit::testing::input_echo_would_trip(&input));

    // Echo layer: the canary does reach stderr, as a bounded verbatim echo.
    let mut bash = fuzz_bash();
    let r = bash
        .exec(&format!("echo $(({input}))"))
        .await
        .expect("exec");
    let err = r.stderr.to_string();
    assert!(
        err.contains(bashkit::testing::FUZZ_HOST_CANARY),
        "expected the echo to carry the canary: {err}"
    );
    assert!(err.len() <= 1024, "stderr is {} bytes:\n{err}", err.len());

    // Detector layer: the full invariant check tolerates it, because the
    // script it ran accounts for every canary byte in the output.
    fuzz_arith(&input, "run_272_canary_echo").await;
}

/// The genuine TM-INF-013 regression is still caught: a canary in stderr that
/// the script cannot account for must still fail the invariant check.
#[test]
#[should_panic(expected = "FUZZ canary leaked into stderr")]
fn canary_not_explained_by_the_script_is_still_a_leak() {
    let result = bashkit::ExecResult {
        stderr: format!("env: {}\n", bashkit::testing::FUZZ_HOST_CANARY).into(),
        ..Default::default()
    };
    // The script never mentions the canary, so the only source is host env.
    bashkit::testing::assert_fuzz_invariants(&result, "echo hi", "canary_real_leak", &[]);
}

/// Same for stdout.
#[test]
#[should_panic(expected = "FUZZ canary leaked into stdout")]
fn canary_in_stdout_not_explained_by_the_script_is_still_a_leak() {
    let result = bashkit::ExecResult {
        stdout: bashkit::testing::FUZZ_HOST_CANARY.into(),
        ..Default::default()
    };
    bashkit::testing::assert_fuzz_invariants(&result, "echo hi", "canary_real_leak_stdout", &[]);
}

/// Run 295: the banned shape straddles the join between the echoed
/// expression and the diagnostic's own punctuation.
///
/// `evaluate_arithmetic` renders `{diag_echo(expr.trim())}: {msg}`, so an
/// expression ending in `Tok:` picks up the separator's colon and the line
/// reads `bash: line 1: wZTok:: syntax error in expression (error token is
/// ":")`. The input carries only `Tok:`, which is not banned -- `Tok::`
/// exists nowhere in the script and nowhere in bashkit's own text; it is
/// spelled by the input's trailing colon sitting next to the template's.
/// Checking the raw input therefore admitted it and the detector tripped on
/// a faithful echo, so the gate renders the slot before matching.
#[tokio::test]
async fn run_295_banned_shape_straddles_the_separator() {
    let input = "wZTok:";
    // The input on its own carries no banned shape.
    assert!(!input.contains("Tok::"), "{input}");
    // The rendered diagnostic does, so the gate must skip it.
    assert!(bashkit::testing::input_echo_would_trip(input));

    let mut bash = fuzz_bash();
    let r = bash
        .exec(&format!("echo $(({input}))"))
        .await
        .expect("exec");
    let err = r.stderr.to_string();
    // The shape is the echo plus the separator, not a formatted `Tok` enum.
    assert!(
        err.contains("wZTok:: syntax error in expression"),
        "expected the verbatim echo and separator: {err}"
    );
    assert!(err.len() <= 1024, "stderr is {} bytes:\n{err}", err.len());
}

/// The widened gate is still narrow: an expression that ends one character
/// short of the shape cannot form it against the separator, so it keeps
/// running under the strict detector.
#[tokio::test]
async fn near_miss_at_the_separator_is_still_checked() {
    let input = "wZTo:";
    assert!(!bashkit::testing::input_echo_would_trip(input));
    fuzz_arith(input, "near_miss_separator").await;
}
