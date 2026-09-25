//! TM-DOS-114: retained array contents must be metered against the variable
//! byte budget (#2462).
//!
//! `max_array_entries` counted entries but never their keys or values, so a
//! script could park a handful of multi-megabyte strings in an array and keep
//! the host memory it wanted:
//!
//! ```text
//! x=x; for i in {1..22}; do x="$x$x"; done   # 4 MiB
//! for i in {1..10}; do a[$i]=$x; done        # 40 MiB retained, exit 0
//! ```
//!
//! Array keys and values now charge the same `max_total_variable_bytes` budget
//! as scalars, are released on unset / replacement / scope pop, and report a
//! visible error instead of dropping the write.

use bashkit::{Bash, ExecutionLimits, MemoryLimits};

/// 64 KiB of headroom: enough for a handful of small entries, far below the
/// megabyte-scale values these tests park in arrays.
fn bounded() -> Bash {
    Bash::builder()
        .memory_limits(MemoryLimits::new().max_total_variable_bytes(64 * 1024))
        .limits(ExecutionLimits::new())
        .build()
}

/// A single-quoted literal of `len` bytes. Built in the test rather than by the
/// script: `printf 'a%.0s' {1..40000}` silently degrades to one byte once brace
/// expansion passes its item cap, which would make these tests assert nothing.
fn filler(len: usize) -> String {
    format!("'{}'", "a".repeat(len))
}

fn assert_budget_error(error: bashkit::Error) {
    let rendered = error.to_string();
    assert!(
        rendered.contains("variable byte limit"),
        "unexpected diagnostic: {rendered}"
    );
}

/// The reproducer from #2462: indexed element assignment.
#[tokio::test]
async fn indexed_element_assignment_over_budget_fails() {
    // 40 KB fits in the 64 KiB budget as a scalar; parking a second copy in an
    // array does not. Before the fix the array copy was free.
    let error = bounded()
        .exec(&format!("v={}; a[1]=$v; echo ${{#a[1]}}", filler(40_000)))
        .await
        .expect_err("over-budget array element must fail execution");
    assert_budget_error(error);
}

/// Many small entries must aggregate, not just be judged one at a time.
#[tokio::test]
async fn many_small_entries_aggregate_against_the_budget() {
    let error = bounded()
        .exec("v=$(printf 'a%.0s' {1..900}); for i in {1..200}; do a[$i]=$v; done; echo ok")
        .await
        .expect_err("aggregate array bytes must fail execution");
    assert_budget_error(error);
}

#[tokio::test]
async fn associative_element_assignment_over_budget_fails() {
    let error = bounded()
        .exec(&format!(
            "declare -A m; v={}; m[k]=$v; echo done",
            filler(40_000)
        ))
        .await
        .expect_err("over-budget assoc entry must fail execution");
    assert_budget_error(error);
}

/// `a[i]+=...` grows a value already in the array.
#[tokio::test]
async fn array_append_over_budget_fails() {
    let error = bounded()
        .exec("v=$(printf 'b%.0s' {1..2000}); a[0]=start; for i in {1..60}; do a[0]+=$v; done; echo ok")
        .await
        .expect_err("over-budget array append must fail execution");
    assert_budget_error(error);
}

/// Bulk replacement `a=(...)` must be metered too.
#[tokio::test]
async fn bulk_array_assignment_over_budget_fails() {
    let error = bounded()
        .exec("v=$(printf 'c%.0s' {1..9000}); a=($v $v $v $v $v $v $v $v $v $v); echo ok")
        .await
        .expect_err("over-budget bulk array assignment must fail execution");
    assert_budget_error(error);
}

/// Replacing a large array with a small one must give the bytes back, and
/// unsetting must release them, so a session is not permanently poisoned.
#[tokio::test]
async fn unset_and_replacement_release_array_bytes() {
    let mut bash = bounded();

    let filled = bash
        .exec("v=$(printf 'd%.0s' {1..8000}); for i in {1..6}; do a[$i]=$v; done; echo ${#a[6]}")
        .await
        .expect("6 x 8000 bytes fits inside 64 KiB");
    assert_eq!(filled.exit_code, 0, "stderr: {}", filled.stderr);
    assert_eq!(filled.stdout, "8000\n");

    // Releasing the whole array must return its bytes to the budget, so the
    // same workload succeeds a second time in the same session.
    let reused = bash
        .exec("unset a; v=$(printf 'e%.0s' {1..8000}); for i in {1..6}; do a[$i]=$v; done; echo ${#a[6]}")
        .await
        .expect("bytes released by unset must be reusable");
    assert_eq!(reused.exit_code, 0, "stderr: {}", reused.stderr);
    assert_eq!(reused.stdout, "8000\n");

    // Element-level unset must release too.
    let per_element = bash
        .exec("for i in {1..6}; do unset 'a[$i]'; done; v=$(printf 'f%.0s' {1..8000}); for i in {1..6}; do a[$i]=$v; done; echo ${#a[6]}")
        .await
        .expect("bytes released by element unset must be reusable");
    assert_eq!(per_element.exit_code, 0, "stderr: {}", per_element.stderr);
    assert_eq!(per_element.stdout, "8000\n");
}

/// Ordinary array workloads below the cap must be untouched.
#[tokio::test]
async fn arrays_below_the_budget_are_unaffected() {
    let mut bash = bounded();
    let result = bash
        .exec(
            "a=(one two three); a[5]=five; declare -A m; m[key]=value; m[other]=thing; \
             echo \"${a[0]} ${a[2]} ${a[5]} ${m[key]} ${m[other]} ${#a[@]}\"",
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "one three five value thing 4\n");
}

/// A function-local array must give its bytes back when the frame pops.
#[tokio::test]
async fn local_array_bytes_are_released_on_scope_pop() {
    let mut bash = bounded();
    let result = bash
        .exec(
            "v=$(printf 'g%.0s' {1..8000}); \
             f() { local -a inner; for i in {1..5}; do inner[$i]=$v; done; echo ${#inner[5]}; }; \
             for round in {1..6}; do f; done",
        )
        .await
        .expect("local array bytes must be released when each frame pops");
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "8000\n".repeat(6));
}

/// The default budget must still admit a realistic array workload.
#[tokio::test]
async fn default_limits_admit_a_realistic_array_workload() {
    let mut bash = Bash::new();
    let result = bash
        .exec("for i in {1..2000}; do a[$i]=\"row-$i\"; done; echo ${#a[@]} ${a[2000]}")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2000 row-2000\n");
}
