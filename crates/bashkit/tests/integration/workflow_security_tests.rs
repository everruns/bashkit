//! Security regression tests for repository workflows.
//!
//! These tests keep high-impact CI credential boundaries explicit. The
//! coreutils drift workflow intentionally executes code from uutils/coreutils,
//! so that work must stay in a read-only job with persisted checkout
//! credentials disabled.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate must live under repo/crates/bashkit")
        .to_path_buf()
}

fn workflow(name: &str) -> String {
    let path = repo_root().join(".github/workflows").join(name);
    fs::read_to_string(path).expect("read workflow")
}

fn section_between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let start_idx = text.find(start).expect("section start");
    let rest = &text[start_idx..];
    let end_idx = rest.find(end).expect("section end");
    &rest[..end_idx]
}

#[test]
fn coreutils_drift_executes_external_code_only_in_read_only_job() {
    let wf = workflow("coreutils-args-drift.yml");
    let regenerate = section_between(&wf, "  regenerate:", "  open-pr:");

    assert!(
        regenerate.contains("permissions:\n      contents: read"),
        "uutils checkout/build/test job must not have repository write permissions"
    );
    assert!(
        !regenerate.contains("contents: write") && !regenerate.contains("pull-requests: write"),
        "external-code job must not request write scopes"
    );
    assert!(
        regenerate.matches("persist-credentials: false").count() >= 2,
        "both bashkit and uutils checkouts must disable persisted credentials"
    );
    assert!(
        regenerate.contains("cargo build --release --bin coreutils"),
        "test must cover the job that executes the external uutils build"
    );
    assert!(
        regenerate.contains("BASHKIT_RUN_COREUTILS_DIFF: '1'"),
        "test must cover the job that executes the external uutils binary"
    );
}

#[test]
fn coreutils_drift_write_job_does_not_execute_uutils_or_third_party_pr_action() {
    let wf = workflow("coreutils-args-drift.yml");
    let open_pr = wf
        .split_once("  open-pr:")
        .map(|(_, section)| section)
        .expect("open-pr job");

    assert!(
        open_pr.contains("permissions:\n      contents: write\n      pull-requests: write"),
        "PR publication job owns the minimal write scopes"
    );
    assert!(
        !open_pr.contains("repository: uutils/coreutils")
            && !open_pr.contains("working-directory: uutils")
            && !open_pr.contains("cargo build")
            && !open_pr.contains("BASHKIT_RUN_COREUTILS_DIFF"),
        "write-scoped job must not checkout, build, or execute uutils"
    );
    assert!(
        !wf.contains("peter-evans/create-pull-request"),
        "write-scoped PR creation must not depend on a third-party action"
    );
    assert!(
        open_pr.contains("gh pr create") || open_pr.contains("gh pr edit"),
        "write-scoped job should publish with GitHub CLI"
    );
}
