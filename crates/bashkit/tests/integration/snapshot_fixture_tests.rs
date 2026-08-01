//! Golden corpus: snapshots written by earlier builds must still restore.
//!
//! The version rules in `knowledge/foundations/snapshot-history.md` say a
//! reader accepts anything whose `min_reader` it satisfies. Rules alone do not
//! catch a regression — these checked-in bytes do. Fixtures are **never**
//! regenerated; a new format version adds a file
//! (`cargo run -p bashkit --example generate_snapshot_fixtures`).

use bashkit::{Bash, CheckoutPolicy};
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/snapshots")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("missing fixture {}: {e}", path.display()))
}

/// Every fixture encodes this same state, whatever version wrote it.
async fn assert_golden_state(bash: &mut Bash) {
    assert_eq!(
        bash.exec("echo $GOLDEN").await.unwrap().stdout.trim(),
        "fixture"
    );
    assert_eq!(
        bash.exec("echo $EXPORTED").await.unwrap().stdout.trim(),
        "yes"
    );
    assert_eq!(
        bash.exec("echo ${arr[1]}").await.unwrap().stdout.trim(),
        "beta"
    );
    assert_eq!(bash.exec("pwd").await.unwrap().stdout.trim(), "/golden");
    assert_eq!(
        bash.exec("greet world").await.unwrap().stdout.trim(),
        "hi world"
    );
    assert_eq!(
        bash.exec("cat /golden/text.txt").await.unwrap().stdout,
        "plain text\n"
    );
    assert_eq!(
        bash.exec("readlink /golden/link")
            .await
            .unwrap()
            .stdout
            .trim(),
        "/golden/text.txt"
    );
    assert_eq!(
        bash.exec("test -d /golden/nested && echo present")
            .await
            .unwrap()
            .stdout
            .trim(),
        "present"
    );
    // Binary content, byte for byte: NUL, 0xff, and 0xfe all survive.
    assert_eq!(
        bash.exec("od -An -tx1 /golden/blob.bin")
            .await
            .unwrap()
            .stdout
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        "00 01 ff fe 7f 80 00 41 42 0a 0d"
    );
}

/// Restored with `Force` on purpose.
///
/// A fixture's capability fingerprint names the builtin set of the build that
/// wrote it, so any later release that adds or removes a builtin would fail the
/// capability gate. These tests are about *format* compatibility; capability
/// policy has its own tests in `snapshot_history_tests`.
async fn restore_fixture(name: &str) -> Bash {
    let mut bash = Bash::new();
    bash.restore_snapshot_with_policy(&fixture(name), CheckoutPolicy::Force)
        .unwrap_or_else(|e| panic!("fixture {name} failed to restore: {e}"));
    bash
}

#[tokio::test]
async fn v1_json_fixture_still_restores() {
    let bytes = fixture("v1.snapshot");
    assert_eq!(bytes[32], b'{', "v1 fixture should be a JSON payload");
    let mut bash = restore_fixture("v1.snapshot").await;
    assert_golden_state(&mut bash).await;
}

#[tokio::test]
async fn v2_container_fixture_still_restores() {
    let bytes = fixture("v2.snapshot");
    assert_eq!(
        &bytes[32..38],
        b"BKSNAP",
        "v2 fixture should be a container"
    );
    let mut bash = restore_fixture("v2.snapshot").await;
    assert_golden_state(&mut bash).await;
}

#[tokio::test]
async fn fixtures_are_rejected_when_corrupted() {
    // The corpus also guards the failure path: a fixture that has been damaged
    // must fail loudly rather than restore partial state.
    for name in ["v1.snapshot", "v2.snapshot"] {
        let mut damaged = fixture(name);
        let at = damaged.len() / 2;
        damaged[at] ^= 0xff;
        let mut bash = Bash::new();
        assert!(
            bash.restore_snapshot_with_policy(&damaged, CheckoutPolicy::Force)
                .is_err(),
            "corrupted {name} restored anyway"
        );
    }
}

#[tokio::test]
async fn every_fixture_in_the_corpus_is_exercised() {
    // A new fixture added without a matching test would otherwise sit unused
    // and silently stop proving anything.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/snapshots");
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".snapshot"))
        .collect();
    found.sort();

    assert_eq!(
        found,
        vec!["v1.snapshot".to_string(), "v2.snapshot".to_string()],
        "corpus changed: add a restore test for each new fixture"
    );
}
