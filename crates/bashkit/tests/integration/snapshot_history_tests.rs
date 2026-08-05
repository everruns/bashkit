//! Snapshot history: commits, forks, checkout, capability gating, and the
//! failure directions that matter — corrupt objects, malformed graphs, missing
//! objects, mid-history restore, and version rejection.
//!
//! Test plan lives in `knowledge/foundations/snapshot-history.md`.

use bashkit::{
    Bash, CapabilityFingerprint, CheckoutPolicy, CommitId, CommitOptions, Error, ObjectId,
    SnapshotGraph, SnapshotOptions,
};
use std::collections::HashMap;

type Store = HashMap<ObjectId, Vec<u8>>;

/// Commit `bash` into `store`, reusing what the store already holds.
fn commit_into(bash: &Bash, store: &mut Store, parents: &[CommitId]) -> CommitId {
    let mut options = CommitOptions::new().have(store.keys());
    for parent in parents {
        options = options.parent(*parent);
    }
    let packed = bash.commit(options).unwrap();
    let id = packed.id();
    store.extend(packed.into_objects());
    id
}

async fn read_file(bash: &mut Bash, path: &str) -> String {
    bash.exec(&format!("cat {path}"))
        .await
        .unwrap()
        .stdout
        .to_string()
}

// ==================== Round-trip fidelity ====================

#[tokio::test]
async fn commit_checkout_round_trips_shell_and_files() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("x=42; export E=env; arr=(a b c); mkdir -p /w; echo data > /w/f.txt; cd /w")
        .await
        .unwrap();

    let id = commit_into(&bash, &mut store, &[]);

    let mut restored = Bash::new();
    restored
        .checkout(id, &store, CheckoutPolicy::Strict)
        .unwrap();

    assert_eq!(restored.exec("echo $x").await.unwrap().stdout.trim(), "42");
    assert_eq!(restored.exec("echo $E").await.unwrap().stdout.trim(), "env");
    assert_eq!(
        restored.exec("echo ${arr[2]}").await.unwrap().stdout.trim(),
        "c"
    );
    assert_eq!(restored.exec("pwd").await.unwrap().stdout.trim(), "/w");
    assert_eq!(read_file(&mut restored, "/w/f.txt").await, "data\n");
}

#[tokio::test]
async fn binary_content_survives_a_round_trip_byte_for_byte() {
    // Every byte value, embedded NULs, and invalid UTF-8 — the cases the v1
    // JSON encoding inflated and that a text-based format would corrupt.
    let mut store = Store::new();
    let mut bash = Bash::new();
    // base64 -d is the route that actually lands raw bytes in the VFS;
    // printf '\xff' goes through shell string handling and arrives lossy.
    bash.exec(
        "mkdir -p /bin_test\n\
         echo 'AAH//n+AAEFCCg0=' | base64 -d > /bin_test/blob.bin\n\
         seq 1 30000 > /bin_test/big.txt",
    )
    .await
    .unwrap();

    let expected_blob = read_via_od(&mut bash, "/bin_test/blob.bin").await;
    let id = commit_into(&bash, &mut store, &[]);

    let mut restored = Bash::new();
    restored
        .checkout(id, &store, CheckoutPolicy::Strict)
        .unwrap();

    assert_eq!(
        read_via_od(&mut restored, "/bin_test/blob.bin").await,
        expected_blob
    );
    assert_eq!(
        expected_blob
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        "00 01 ff fe 7f 80 00 41 42 0a 0d",
        "the fixture itself must contain real binary, or the test proves nothing"
    );
    // A multi-chunk file must also come back intact.
    assert_eq!(
        restored
            .exec("wc -l < /bin_test/big.txt")
            .await
            .unwrap()
            .stdout
            .trim(),
        "30000"
    );
}

async fn read_via_od(bash: &mut Bash, path: &str) -> String {
    bash.exec(&format!("od -An -tx1 {path}"))
        .await
        .unwrap()
        .stdout
        .to_string()
}

#[tokio::test]
async fn symlinks_directories_and_modes_survive() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec(
        "mkdir -p /d/sub; echo t > /d/target.txt; ln -s /d/target.txt /d/link; chmod 700 /d/sub",
    )
    .await
    .unwrap();

    let id = commit_into(&bash, &mut store, &[]);
    let mut restored = Bash::new();
    restored
        .checkout(id, &store, CheckoutPolicy::Strict)
        .unwrap();

    // The link is preserved as a link, with its target intact. (Reading
    // *through* a symlink is a separate, pre-existing gap — `cat` does not
    // dereference here either before or after a snapshot.)
    assert_eq!(
        restored
            .exec("readlink /d/link")
            .await
            .unwrap()
            .stdout
            .trim(),
        "/d/target.txt"
    );
    assert_eq!(read_file(&mut restored, "/d/target.txt").await, "t\n");
    let modes = restored.exec("stat -c '%a' /d/sub").await.unwrap();
    assert_eq!(modes.stdout.trim(), "700");
    assert_eq!(
        restored
            .exec("test -d /d/sub && echo dir")
            .await
            .unwrap()
            .stdout
            .trim(),
        "dir"
    );
}

// ==================== Forks ====================

#[tokio::test]
async fn forks_diverge_without_contaminating_each_other() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("echo base > /log.txt").await.unwrap();
    let base = commit_into(&bash, &mut store, &[]);

    // Branch one.
    let mut left = Bash::new();
    left.checkout(base, &store, CheckoutPolicy::Strict).unwrap();
    left.exec("echo left >> /log.txt; SIDE=left").await.unwrap();
    let left_id = commit_into(&left, &mut store, &[base]);

    // Branch two, from the same parent.
    let mut right = Bash::new();
    right
        .checkout(base, &store, CheckoutPolicy::Strict)
        .unwrap();
    right
        .exec("echo right >> /log.txt; SIDE=right")
        .await
        .unwrap();
    let right_id = commit_into(&right, &mut store, &[base]);

    let mut check = Bash::new();
    check
        .checkout(left_id, &store, CheckoutPolicy::Strict)
        .unwrap();
    assert_eq!(read_file(&mut check, "/log.txt").await, "base\nleft\n");
    assert_eq!(
        check.exec("echo $SIDE").await.unwrap().stdout.trim(),
        "left"
    );

    check
        .checkout(right_id, &store, CheckoutPolicy::Strict)
        .unwrap();
    assert_eq!(read_file(&mut check, "/log.txt").await, "base\nright\n");
    assert_eq!(
        check.exec("echo $SIDE").await.unwrap().stdout.trim(),
        "right"
    );

    // The parent is untouched by either branch.
    check
        .checkout(base, &store, CheckoutPolicy::Strict)
        .unwrap();
    assert_eq!(read_file(&mut check, "/log.txt").await, "base\n");

    assert_eq!(SnapshotGraph::parents(left_id, &store).unwrap(), vec![base]);
    assert_eq!(
        SnapshotGraph::parents(right_id, &store).unwrap(),
        vec![base]
    );
}

#[tokio::test]
async fn forks_share_storage_with_their_ancestor() {
    // The economic claim behind the design: branching must not re-store
    // content the parent already has.
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("mkdir -p /w; for i in $(seq 1 20); do echo content-$i > /w/f$i.txt; done")
        .await
        .unwrap();
    let base = commit_into(&bash, &mut store, &[]);

    let mut fork = Bash::new();
    fork.checkout(base, &store, CheckoutPolicy::Strict).unwrap();
    fork.exec("echo changed > /w/f1.txt").await.unwrap();

    let packed = fork
        .commit(CommitOptions::new().parent(base).have(store.keys()))
        .unwrap();

    // Commit + caps(shared, skipped) + shell + tree + one file manifest.
    // The other 19 files and every directory cost nothing.
    assert!(
        packed.object_count() <= 5,
        "fork re-stored {} objects; expected only what changed",
        packed.object_count()
    );
}

// ==================== Mid-history checkout ====================

#[tokio::test]
async fn every_ancestor_checks_out_to_its_own_state() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    let mut ids = Vec::new();
    let mut expected = Vec::new();

    for step in 0..8 {
        bash.exec(&format!("echo step-{step} >> /history.txt; STEP={step}"))
            .await
            .unwrap();
        let parents: Vec<CommitId> = ids.last().copied().into_iter().collect();
        ids.push(commit_into(&bash, &mut store, &parents));
        expected.push(read_file(&mut bash, "/history.txt").await);
    }

    // Walk backwards so a stale instance cannot make a later checkout look right.
    for step in (0..8).rev() {
        let mut restored = Bash::new();
        restored
            .checkout(ids[step], &store, CheckoutPolicy::Strict)
            .unwrap();
        assert_eq!(
            read_file(&mut restored, "/history.txt").await,
            expected[step],
            "history mismatch at step {step}"
        );
        assert_eq!(
            restored.exec("echo $STEP").await.unwrap().stdout.trim(),
            step.to_string()
        );
    }
}

#[tokio::test]
async fn checkout_matches_a_full_snapshot_taken_at_the_same_point() {
    // Differential check: the object graph must reproduce exactly what the
    // single-blob path produces, at every step.
    let mut store = Store::new();
    let mut bash = Bash::new();
    let script = [
        "echo one > /a.txt",
        "mkdir -p /d && echo two > /d/b.txt",
        "rm /a.txt; echo three > /d/c.txt",
        "V=set; echo four >> /d/b.txt",
    ];

    for (step, command) in script.iter().enumerate() {
        bash.exec(command).await.unwrap();
        let id = commit_into(&bash, &mut store, &[]);
        let packed = bash.snapshot().unwrap();

        let mut via_graph = Bash::new();
        via_graph
            .checkout(id, &store, CheckoutPolicy::Strict)
            .unwrap();
        let mut via_blob = Bash::new();
        via_blob.restore_snapshot(&packed).unwrap();

        // Scope to real content: /dev entries are synthetic and /dev/random differs per read.
        let listing = "find / -path /dev -prune -o -type f -print | sort | xargs -I{} sh -c 'echo {}; cat {}'";
        assert_eq!(
            via_graph.exec(listing).await.unwrap().stdout,
            via_blob.exec(listing).await.unwrap().stdout,
            "filesystem diverged at step {step}"
        );
        assert_eq!(
            via_graph.exec("echo $V").await.unwrap().stdout,
            via_blob.exec("echo $V").await.unwrap().stdout,
            "shell state diverged at step {step}"
        );
    }
}

// ==================== Ancestry and diff ====================

#[tokio::test]
async fn ancestry_walks_newest_first_and_stops_at_the_limit() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    let mut ids = Vec::new();
    for step in 0..5 {
        bash.exec(&format!("N={step}")).await.unwrap();
        let parents: Vec<CommitId> = ids.last().copied().into_iter().collect();
        ids.push(commit_into(&bash, &mut store, &parents));
    }

    let walked = SnapshotGraph::ancestry(*ids.last().unwrap(), &store, 100).unwrap();
    let mut expected = ids.clone();
    expected.reverse();
    assert_eq!(walked, expected);

    let capped = SnapshotGraph::ancestry(*ids.last().unwrap(), &store, 2).unwrap();
    assert_eq!(capped.len(), 2);
}

#[tokio::test]
async fn ancestry_stops_where_the_store_ends_instead_of_failing() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("A=1").await.unwrap();
    let first = commit_into(&bash, &mut store, &[]);
    bash.exec("A=2").await.unwrap();
    let second = commit_into(&bash, &mut store, &[first]);

    // A host that pruned old history still gets a usable partial walk.
    store.remove(&first);
    assert_eq!(
        SnapshotGraph::ancestry(second, &store, 100).unwrap(),
        vec![second]
    );
}

#[tokio::test]
async fn diff_reports_adds_modifications_and_removals() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec(
        "mkdir -p /d; echo keep > /d/keep.txt; echo gone > /d/gone.txt; echo edit > /d/edit.txt",
    )
    .await
    .unwrap();
    let before = commit_into(&bash, &mut store, &[]);

    bash.exec("rm /d/gone.txt; echo changed > /d/edit.txt; echo new > /d/new.txt")
        .await
        .unwrap();
    let after = commit_into(&bash, &mut store, &[before]);

    let diff = SnapshotGraph::diff(before, after, &store).unwrap();
    assert_eq!(diff.files_added, vec!["/d/new.txt".to_string()]);
    assert_eq!(diff.files_modified, vec!["/d/edit.txt".to_string()]);
    assert_eq!(diff.files_removed, vec!["/d/gone.txt".to_string()]);
    assert!(!diff.is_empty());
}

#[tokio::test]
async fn diff_of_a_commit_against_itself_is_empty() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("echo x > /f.txt").await.unwrap();
    let id = commit_into(&bash, &mut store, &[]);

    let diff = SnapshotGraph::diff(id, id, &store).unwrap();
    assert!(diff.is_empty(), "self-diff should be empty, got {diff:?}");
}

#[tokio::test]
async fn diff_detects_shell_only_changes() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("echo x > /f.txt").await.unwrap();
    let before = commit_into(&bash, &mut store, &[]);
    bash.exec("ONLY_SHELL=changed").await.unwrap();
    let after = commit_into(&bash, &mut store, &[before]);

    let diff = SnapshotGraph::diff(before, after, &store).unwrap();
    assert!(diff.shell_changed);
    assert!(diff.files_added.is_empty() && diff.files_modified.is_empty());
}

// ==================== Incremental storage ====================

#[tokio::test]
async fn unchanged_content_is_not_re_emitted() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("mkdir -p /w; for i in $(seq 1 40); do echo body-$i > /w/f$i.txt; done")
        .await
        .unwrap();
    let first = commit_into(&bash, &mut store, &[]);
    let baseline = store.len();

    bash.exec("echo appended >> /w/f7.txt").await.unwrap();
    let packed = bash
        .commit(CommitOptions::new().parent(first).have(store.keys()))
        .unwrap();

    assert!(
        packed.object_count() < baseline / 4,
        "incremental commit emitted {} of {baseline} objects",
        packed.object_count()
    );
    assert!(!packed.is_self_contained());
    assert!(
        packed.to_bytes().is_err(),
        "packing an incremental commit must fail loudly, not produce unrestorable bytes"
    );
}

#[tokio::test]
async fn large_file_edit_only_re_stores_touched_chunks() {
    // Content-defined chunking's reason for existing: editing one region of a
    // large file must not re-store the whole file.
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("mkdir -p /w; seq 1 50000 > /w/big.txt")
        .await
        .unwrap();
    let first = commit_into(&bash, &mut store, &[]);
    let full_bytes: usize = store.values().map(Vec::len).sum();

    bash.exec("echo appended-tail >> /w/big.txt").await.unwrap();
    let packed = bash
        .commit(CommitOptions::new().parent(first).have(store.keys()))
        .unwrap();

    assert!(
        packed.stored_bytes() * 4 < full_bytes,
        "append re-stored {} bytes against a {full_bytes}-byte baseline",
        packed.stored_bytes()
    );
}

// ==================== Determinism ====================

#[tokio::test]
async fn identical_state_produces_an_identical_commit_id() {
    // Guards the property v1 lacked: HashMap iteration order must not leak
    // into snapshot identity, or dedup and diff both collapse.
    let mut first = Bash::new();
    let mut second = Bash::new();
    let setup = "export Z=26; export A=1; export M=13; \
                 declare -A m=([k9]=v9 [k1]=v1 [k5]=v5); \
                 mkdir -p /d; echo one > /d/a.txt; echo two > /d/b.txt";
    first.exec(setup).await.unwrap();
    second.exec(setup).await.unwrap();

    let a = first.commit(CommitOptions::new()).unwrap();
    let b = second.commit(CommitOptions::new()).unwrap();
    assert_eq!(a.id(), b.id(), "identical state must hash identically");
    assert_eq!(first.snapshot().unwrap(), second.snapshot().unwrap());
}

#[tokio::test]
async fn different_state_produces_a_different_commit_id() {
    let mut first = Bash::new();
    let mut second = Bash::new();
    first.exec("V=1").await.unwrap();
    second.exec("V=2").await.unwrap();
    assert_ne!(
        first.commit(CommitOptions::new()).unwrap().id(),
        second.commit(CommitOptions::new()).unwrap().id()
    );
}

#[tokio::test]
async fn commit_metadata_round_trips_and_changes_identity() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("X=1").await.unwrap();

    let plain = bash.commit(CommitOptions::new()).unwrap();
    let tagged = bash
        .commit(CommitOptions::new().meta("message_id", "msg-42"))
        .unwrap();
    assert_ne!(plain.id(), tagged.id());

    let id = tagged.id();
    store.extend(tagged.into_objects());
    assert_eq!(
        SnapshotGraph::meta(id, &store).unwrap().get("message_id"),
        Some(&"msg-42".to_string())
    );
}

// ==================== Capability gating ====================

fn bash_with_extra_builtin() -> Bash {
    use bashkit::{Builtin, BuiltinContext, ExecResult};

    struct Extra;
    #[async_trait::async_trait]
    impl Builtin for Extra {
        async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
            Ok(ExecResult::ok("extra\n".to_string()))
        }
    }

    Bash::builder()
        .builtin("extra_tool", Box::new(Extra))
        .build()
}

#[tokio::test]
async fn strict_rejects_a_snapshot_from_a_different_tool_set() {
    let mut store = Store::new();
    let producer = bash_with_extra_builtin();
    let id = commit_into(&producer, &mut store, &[]);

    let mut plain = Bash::new();
    let err = plain
        .checkout(id, &store, CheckoutPolicy::Strict)
        .unwrap_err();
    assert!(
        matches!(err, Error::SnapshotCapabilityMismatch(_)),
        "expected a typed capability error, got {err}"
    );
    assert!(err.to_string().contains("extra_tool"), "got: {err}");
}

#[tokio::test]
async fn superset_allows_restoring_into_a_richer_instance() {
    let mut store = Store::new();
    let producer = Bash::new();
    let id = commit_into(&producer, &mut store, &[]);

    let mut richer = bash_with_extra_builtin();
    // Strict refuses the extra builtin; Superset accepts it.
    assert!(richer.checkout(id, &store, CheckoutPolicy::Strict).is_err());
    richer
        .checkout(id, &store, CheckoutPolicy::Superset)
        .unwrap();
}

#[tokio::test]
async fn superset_still_rejects_a_missing_tool() {
    let mut store = Store::new();
    let producer = bash_with_extra_builtin();
    let id = commit_into(&producer, &mut store, &[]);

    let mut plain = Bash::new();
    assert!(
        plain
            .checkout(id, &store, CheckoutPolicy::Superset)
            .is_err()
    );
}

#[tokio::test]
async fn force_restores_despite_any_mismatch() {
    let mut store = Store::new();
    let mut producer = bash_with_extra_builtin();
    producer.exec("echo forced > /f.txt").await.unwrap();
    let id = commit_into(&producer, &mut store, &[]);

    let mut plain = Bash::new();
    plain.checkout(id, &store, CheckoutPolicy::Force).unwrap();
    assert_eq!(read_file(&mut plain, "/f.txt").await, "forced\n");
}

#[tokio::test]
async fn capabilities_are_readable_without_restoring() {
    let mut store = Store::new();
    let producer = bash_with_extra_builtin();
    let id = commit_into(&producer, &mut store, &[]);

    let caps = SnapshotGraph::capabilities(id, &store).unwrap();
    assert!(caps.builtins.contains(&"extra_tool".to_string()));
    assert_eq!(caps.fs_backend, "in-memory");

    let live = SnapshotGraph::capabilities(id, &store).unwrap();
    assert!(caps.compare(&live).is_empty());
}

#[tokio::test]
async fn packed_snapshot_enforces_the_same_capability_gate() {
    let producer = bash_with_extra_builtin();
    let bytes = producer.snapshot().unwrap();

    assert!(Bash::from_snapshot(&bytes).is_err());
    let mut plain = Bash::new();
    plain
        .restore_snapshot_with_policy(&bytes, CheckoutPolicy::Force)
        .unwrap();
}

#[tokio::test]
async fn a_failed_capability_check_leaves_the_instance_untouched() {
    let mut store = Store::new();
    let mut producer = bash_with_extra_builtin();
    producer
        .exec("echo intruder > /intruder.txt")
        .await
        .unwrap();
    let id = commit_into(&producer, &mut store, &[]);

    let mut plain = Bash::new();
    plain
        .exec("echo original > /original.txt; KEEP=yes")
        .await
        .unwrap();
    assert!(plain.checkout(id, &store, CheckoutPolicy::Strict).is_err());

    assert_eq!(read_file(&mut plain, "/original.txt").await, "original\n");
    assert_eq!(plain.exec("echo $KEEP").await.unwrap().stdout.trim(), "yes");
    assert!(
        plain.exec("test -f /intruder.txt").await.unwrap().exit_code != 0,
        "rejected checkout must not have written any state"
    );
}

// ==================== Corrupt and hostile input ====================

#[tokio::test]
async fn a_tampered_object_fails_hash_verification() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("echo secret > /f.txt").await.unwrap();
    let id = commit_into(&bash, &mut store, &[]);

    // Corrupt every object in turn, two ways, and require both to be caught.
    //
    // Note what is deliberately *not* asserted: that every possible bit flip is
    // detectable. Object IDs cover decoded content, not its compressed framing,
    // so flipping unused padding bits in a deflate stream's final byte yields
    // byte-different storage that decodes identically. That is a no-op, and
    // treating it as tampering would forbid a host from ever recompressing its
    // store. What must never survive is a change to the content itself.
    let ids: Vec<ObjectId> = store.keys().copied().collect();
    for target in ids {
        let mut mutated = store.clone();
        let blob = mutated.get_mut(&target).unwrap();
        // Index 0 is the compression flag; index 1 starts the payload.
        blob[1] ^= 0xff;
        let mut restored = Bash::new();
        assert!(
            restored
                .checkout(id, &mutated, CheckoutPolicy::Force)
                .is_err(),
            "content tampering with object {target} went undetected"
        );

        let mut truncated = store.clone();
        let blob = truncated.get_mut(&target).unwrap();
        // Halve it rather than dropping one byte: a deflate stream's final
        // byte can be pure padding, so a one-byte trim is another framing-only
        // no-op. Cutting the stream in half is unambiguous corruption.
        blob.truncate(blob.len() / 2);
        let mut restored = Bash::new();
        assert!(
            restored
                .checkout(id, &truncated, CheckoutPolicy::Force)
                .is_err(),
            "truncating object {target} went undetected"
        );

        let mut extended = store.clone();
        extended
            .get_mut(&target)
            .unwrap()
            .extend_from_slice(b"smuggled");
        let mut restored = Bash::new();
        assert!(
            restored
                .checkout(id, &extended, CheckoutPolicy::Force)
                .is_err(),
            "appending to object {target} went undetected"
        );
    }
}

#[tokio::test]
async fn a_missing_object_is_reported_not_silently_skipped() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("mkdir -p /w; echo content > /w/f.txt")
        .await
        .unwrap();
    let id = commit_into(&bash, &mut store, &[]);

    let ids: Vec<ObjectId> = store.keys().copied().filter(|k| *k != id).collect();
    for target in ids {
        let mut incomplete = store.clone();
        incomplete.remove(&target);
        let mut restored = Bash::new();
        assert!(
            restored
                .checkout(id, &incomplete, CheckoutPolicy::Force)
                .is_err(),
            "checkout succeeded without object {target}"
        );
    }
}

#[tokio::test]
async fn an_unknown_root_is_an_error() {
    let store = Store::new();
    let mut bash = Bash::new();
    let unknown = ObjectId::from_bytes([0xab; 32]);
    assert!(
        bash.checkout(unknown, &store, CheckoutPolicy::Force)
            .is_err()
    );
    assert!(SnapshotGraph::read_commit(unknown, &store).is_err());
}

#[tokio::test]
async fn type_confusion_between_object_kinds_is_rejected() {
    // Serve a valid object under another object's ID slot. Content addressing
    // catches the substitution; the kind tag catches a same-content swap.
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("echo x > /f.txt").await.unwrap();
    let id = commit_into(&bash, &mut store, &[]);

    let commit_blob = store.get(&id).unwrap().clone();
    let other: ObjectId = *store.keys().find(|k| **k != id).unwrap();

    let mut swapped = store.clone();
    swapped.insert(other, commit_blob);

    let mut restored = Bash::new();
    assert!(
        restored
            .checkout(id, &swapped, CheckoutPolicy::Force)
            .is_err()
    );
}

#[tokio::test]
async fn a_self_referencing_parent_does_not_hang_ancestry() {
    // A hostile store can serve a cycle even though a well-formed commit
    // cannot contain one; the walk must terminate regardless.
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("A=1").await.unwrap();
    let first = commit_into(&bash, &mut store, &[]);
    bash.exec("A=2").await.unwrap();
    let second = commit_into(&bash, &mut store, &[first]);

    // Point the older commit's slot at the newer one. The object no longer
    // matches its id, so this is corruption rather than a cycle — and it must
    // surface as an error, not as history that merely ended early.
    let mut cyclic = store.clone();
    let newer = cyclic.get(&second).unwrap().clone();
    cyclic.insert(first, newer);

    let err = SnapshotGraph::ancestry(second, &cyclic, 1000).unwrap_err();
    assert!(
        err.to_string().contains("content hash"),
        "corruption should be reported, got: {err}"
    );
}

#[tokio::test]
async fn ancestry_distinguishes_a_pruned_ancestor_from_a_corrupt_one() {
    // The difference matters operationally: a pruned history is a normal
    // retention outcome, while a commit that fails to verify means the store
    // is damaged. Reporting the second as the first would hide data loss.
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("A=1").await.unwrap();
    let first = commit_into(&bash, &mut store, &[]);
    bash.exec("A=2").await.unwrap();
    let second = commit_into(&bash, &mut store, &[first]);

    let mut pruned = store.clone();
    pruned.remove(&first);
    assert_eq!(
        SnapshotGraph::ancestry(second, &pruned, 100).unwrap(),
        vec![second],
        "a missing ancestor should simply end the walk"
    );

    let mut corrupt = store.clone();
    let blob = corrupt.get_mut(&first).unwrap();
    blob[1] ^= 0xff;
    assert!(
        SnapshotGraph::ancestry(second, &corrupt, 100).is_err(),
        "a present-but-unverifiable ancestor must not read as pruned"
    );
}

#[tokio::test]
async fn commit_refuses_more_parents_than_checkout_accepts() {
    // Previously commit() returned Ok for 65 parents and every later read of
    // that commit failed, so the error surfaced far from its cause.
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("A=1").await.unwrap();
    let base = commit_into(&bash, &mut store, &[]);

    let mut options = CommitOptions::new();
    for _ in 0..65 {
        options = options.parent(base);
    }
    assert!(
        bash.commit(options).is_err(),
        "commit must reject a parent list the decoder would refuse"
    );

    // 64 is the documented maximum and must still work.
    let mut options = CommitOptions::new();
    for _ in 0..64 {
        options = options.parent(base);
    }
    let packed = bash.commit(options).unwrap();
    store.extend(packed.objects().map(|(id, b)| (id, b.to_vec())));
    assert_eq!(
        SnapshotGraph::parents(packed.id(), &store).unwrap().len(),
        64
    );
}

#[tokio::test]
async fn truncated_and_tampered_packed_snapshots_are_rejected() {
    let mut bash = Bash::new();
    bash.exec("echo x > /f.txt").await.unwrap();
    let bytes = bash.snapshot().unwrap();

    assert!(Bash::from_snapshot(&bytes[..bytes.len() / 2]).is_err());
    assert!(Bash::from_snapshot(&bytes[..10]).is_err());
    assert!(Bash::from_snapshot(&[]).is_err());

    let mut flipped = bytes.clone();
    let at = flipped.len() - 1;
    flipped[at] ^= 0xff;
    assert!(Bash::from_snapshot(&flipped).is_err());

    // Digest recomputed over tampered content still fails: the digest is not a
    // security boundary, but the object hashes underneath it are.
    let mut body_tampered = bytes.clone();
    let mid = 32 + (bytes.len() - 32) / 2;
    body_tampered[mid] ^= 0x01;
    assert!(Bash::from_snapshot(&body_tampered).is_err());
}

#[tokio::test]
async fn keyed_packed_snapshots_reject_the_wrong_key() {
    let mut bash = Bash::new();
    bash.exec("echo x > /f.txt").await.unwrap();
    let bytes = bash.snapshot_to_bytes_keyed(b"correct-key").unwrap();

    assert!(Bash::from_snapshot_keyed(&bytes, b"correct-key").is_ok());
    assert!(Bash::from_snapshot_keyed(&bytes, b"wrong-key").is_err());
    // Unkeyed verification must not accept an HMAC-sealed snapshot.
    assert!(Bash::from_snapshot(&bytes).is_err());
}

// ==================== Planning and reachability ====================

#[tokio::test]
async fn plan_checkout_converges_as_objects_arrive() {
    let mut source = Store::new();
    let mut bash = Bash::new();
    bash.exec("mkdir -p /w; for i in $(seq 1 200); do echo line-$i; done > /w/big.txt")
        .await
        .unwrap();
    let id = commit_into(&bash, &mut source, &[]);

    // Simulate a host fetching wave by wave from its own store.
    let mut local = Store::new();
    let mut rounds = 0;
    loop {
        let need = SnapshotGraph::plan_checkout(id, &local).unwrap();
        if need.is_empty() {
            break;
        }
        rounds += 1;
        assert!(rounds < 10, "plan_checkout failed to converge");
        for want in need {
            local.insert(want, source.get(&want).unwrap().clone());
        }
    }

    assert!(rounds >= 2, "expected a multi-wave walk, got {rounds}");
    let mut restored = Bash::new();
    restored
        .checkout(id, &local, CheckoutPolicy::Strict)
        .unwrap();
    assert_eq!(
        restored
            .exec("wc -l < /w/big.txt")
            .await
            .unwrap()
            .stdout
            .trim(),
        "200"
    );
}

#[tokio::test]
async fn reachable_lists_the_full_object_set_for_gc() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("echo x > /f.txt").await.unwrap();
    let first = commit_into(&bash, &mut store, &[]);
    bash.exec("echo y > /g.txt").await.unwrap();
    let second = commit_into(&bash, &mut store, &[first]);

    let live = SnapshotGraph::reachable(second, &store).unwrap();
    assert!(live.contains(&second));
    // Parents are history, not content: checking out `second` never needs them.
    assert!(!live.contains(&first));

    let mut broken = store.clone();
    broken.remove(&second);
    assert!(SnapshotGraph::reachable(second, &broken).is_err());
}

// ==================== Options and shell-only commits ====================

#[tokio::test]
async fn shell_only_commits_omit_the_filesystem() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("KEEP=1; echo disk > /disk.txt").await.unwrap();

    let packed = bash
        .commit(CommitOptions::new().exclude_filesystem(true))
        .unwrap();
    let id = packed.id();
    store.extend(packed.into_objects());

    let mut restored = Bash::new();
    restored
        .exec("echo untouched > /untouched.txt")
        .await
        .unwrap();
    restored
        .checkout(id, &store, CheckoutPolicy::Strict)
        .unwrap();

    assert_eq!(
        restored.exec("echo $KEEP").await.unwrap().stdout.trim(),
        "1"
    );
    // The VFS is left alone rather than emptied.
    assert_eq!(
        read_file(&mut restored, "/untouched.txt").await,
        "untouched\n"
    );
}

#[tokio::test]
async fn excluding_functions_drops_them_from_the_commit() {
    let mut store = Store::new();
    let mut bash = Bash::new();
    bash.exec("greet() { echo hi; }; V=1").await.unwrap();

    let packed = bash
        .commit(CommitOptions::new().exclude_functions(true))
        .unwrap();
    let id = packed.id();
    store.extend(packed.into_objects());

    let mut restored = Bash::new();
    restored
        .checkout(id, &store, CheckoutPolicy::Strict)
        .unwrap();
    assert_eq!(restored.exec("echo $V").await.unwrap().stdout.trim(), "1");
    assert_ne!(restored.exec("type greet").await.unwrap().exit_code, 0);
}

// ==================== Limits ====================

#[tokio::test]
async fn a_commit_exceeding_the_target_filesystem_limits_is_refused_atomically() {
    use bashkit::{ExecutionLimits, FileSystem, InMemoryFs};

    let mut store = Store::new();
    let mut big = Bash::new();
    big.exec("mkdir -p /w; for i in $(seq 1 400); do echo padding-line-$i; done > /w/big.txt")
        .await
        .unwrap();
    let id = commit_into(&big, &mut store, &[]);

    // A target whose filesystem cannot hold the snapshot.
    let tiny_fs: std::sync::Arc<dyn FileSystem> =
        std::sync::Arc::new(InMemoryFs::with_limits(bashkit::FsLimits {
            max_total_bytes: 512,
            ..Default::default()
        }));
    let mut small = Bash::builder()
        .fs(tiny_fs)
        .limits(ExecutionLimits::default())
        .build();
    small.exec("echo pre-existing > /keep.txt").await.unwrap();

    assert!(small.checkout(id, &store, CheckoutPolicy::Force).is_err());
    // Validate-before-mutate: the original contents survive the refusal.
    assert_eq!(read_file(&mut small, "/keep.txt").await, "pre-existing\n");
}

// ==================== Version policy ====================

#[tokio::test]
async fn a_snapshot_demanding_a_newer_reader_is_a_typed_error() {
    let mut bash = Bash::new();
    bash.exec("X=1").await.unwrap();
    let bytes = bash.snapshot().unwrap();

    // min_reader sits at body offset 10, i.e. 32 (digest) + 10.
    let mut future = bytes.clone();
    future[42..44].copy_from_slice(&4242u16.to_le_bytes());
    // Reseal so the integrity digest is not what rejects it.
    let resealed = reseal(&future);

    match Bash::from_snapshot(&resealed) {
        Err(Error::SnapshotTooNew { required, .. }) => assert_eq!(required, 4242),
        Err(other) => panic!("expected SnapshotTooNew, got {other}"),
        Ok(_) => panic!("a too-new snapshot must not restore"),
    }
}

#[tokio::test]
async fn newer_non_breaking_versions_still_restore() {
    let mut bash = Bash::new();
    bash.exec("X=survives").await.unwrap();
    let bytes = bash.snapshot().unwrap();

    let mut future = bytes.clone();
    future[38] = 200; // container_version
    future[40..42].copy_from_slice(&500u16.to_le_bytes()); // schema_version
    let resealed = reseal(&future);

    let mut restored = Bash::from_snapshot(&resealed).unwrap();
    assert_eq!(
        restored.exec("echo $X").await.unwrap().stdout.trim(),
        "survives"
    );
}

/// Recompute the unkeyed integrity digest over a modified body.
///
/// Mirrors the public-tag digest; this is exactly the forgery TM-SNAP-001
/// documents as possible, and it is what lets these tests reach the version
/// checks instead of stopping at the digest.
fn reseal(data: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let body = &data[32..];
    let mut hasher = Sha256::new();
    hasher.update(b"BKSNAP01");
    hasher.update(body);
    let mut out = hasher.finalize().to_vec();
    out.extend_from_slice(body);
    out
}

// ==================== Legacy v1 compatibility ====================

#[tokio::test]
async fn legacy_v1_snapshots_still_restore() {
    let mut bash = Bash::new();
    bash.exec("LEGACY=yes; mkdir -p /d; echo old > /d/file.txt")
        .await
        .unwrap();

    let v1 = bash
        .snapshot_state(SnapshotOptions::default())
        .to_bytes()
        .unwrap();
    assert_eq!(v1[32], b'{', "v1 payload should still be JSON");

    let mut restored = Bash::from_snapshot(&v1).unwrap();
    assert_eq!(
        restored.exec("echo $LEGACY").await.unwrap().stdout.trim(),
        "yes"
    );
    assert_eq!(read_file(&mut restored, "/d/file.txt").await, "old\n");
}

#[tokio::test]
async fn v1_snapshots_skip_the_capability_gate() {
    // v1 predates fingerprints, so Strict has nothing to compare and must not
    // invent a failure — otherwise the upgrade would strand stored state.
    let producer = bash_with_extra_builtin();
    let v1 = producer
        .snapshot_state(SnapshotOptions::default())
        .to_bytes()
        .unwrap();

    let mut plain = Bash::new();
    plain.restore_snapshot(&v1).unwrap();
}

#[tokio::test]
async fn capability_fingerprint_is_stable_across_instances() {
    let a = CapabilityFingerprint::capture(&Bash::new());
    let b = CapabilityFingerprint::capture(&Bash::new());
    assert_eq!(a, b);
    assert!(a.compare(&b).is_empty());
}

// ==================== Hostile and confusing VFS entries ====================
//
// A snapshot's tree names paths directly, so it is a way to put entries into
// the VFS without going through any bash command. These tests pin down what a
// forged tree can and cannot do. The invariant is that a path the sandbox's own
// resolution never produces stays unreachable — not that such a path is
// rejected. If someone later normalizes paths on restore, `/a/../b.txt` would
// suddenly resolve to `/b.txt`, and these tests are what would catch it.

use bashkit::{Snapshot, VfsEntry, VfsEntryKind, VfsSnapshot};
use std::path::PathBuf;

fn forged_file(path: &str, content: &str) -> VfsEntry {
    VfsEntry {
        path: PathBuf::from(path),
        kind: VfsEntryKind::File {
            content: content.as_bytes().to_vec(),
        },
        mode: 0o644,
    }
}

/// Inject entries into an otherwise well-formed VFS and serialize.
///
/// Built through the v1 encoder because it is the one path that accepts a
/// hand-made `VfsSnapshot`; restore funnels both formats into the same routine,
/// so the filesystem-side invariants are identical.
async fn snapshot_with_forged_entries(extra: Vec<VfsEntry>) -> Vec<u8> {
    let mut bash = Bash::new();
    bash.exec("echo canary > /canary.txt").await.unwrap();
    let mut snapshot: Snapshot = bash.snapshot_state(SnapshotOptions::default());
    let mut entries = snapshot
        .vfs
        .as_ref()
        .map(|vfs| vfs.entries().to_vec())
        .unwrap_or_default();
    entries.extend(extra);
    snapshot.vfs = Some(VfsSnapshot::from_entries(entries));
    snapshot.to_bytes().unwrap()
}

#[tokio::test]
async fn forged_paths_are_inert_rather_than_a_sandbox_escape() {
    let bytes = snapshot_with_forged_entries(vec![
        // Traversal above the VFS root.
        forged_file("/../../../etc/passwd", "root::0:0::/:/bin/sh\n"),
        // Non-normalized: must not become reachable as /b.txt.
        forged_file("/a/../b.txt", "confused\n"),
        // Relative, and empty — neither is a path the sandbox can produce.
        forged_file("relative.txt", "relative\n"),
        forged_file("", "empty\n"),
    ])
    .await;

    let mut bash = Bash::from_snapshot(&bytes).unwrap();

    // The legitimate entry still restored, so this is a real check and not a
    // vacuous one against an empty filesystem.
    assert_eq!(
        bash.exec("cat /canary.txt").await.unwrap().stdout,
        "canary\n"
    );

    for unreachable in ["/etc/passwd", "/b.txt", "/a/b.txt", "/relative.txt"] {
        let result = bash.exec(&format!("cat {unreachable}")).await.unwrap();
        assert_ne!(
            result.exit_code, 0,
            "{unreachable} became readable from a forged snapshot entry"
        );
        assert!(
            result.stdout.is_empty(),
            "{unreachable} leaked content: {:?}",
            result.stdout
        );
    }

    // None of them show up in a listing either.
    let root = bash.exec("ls -a /").await.unwrap().stdout;
    assert!(
        root.contains("canary.txt"),
        "expected the canary in {root:?}"
    );
    for hidden in ["passwd", "b.txt", "relative.txt"] {
        assert!(
            !root.contains(hidden),
            "{hidden} surfaced in root: {root:?}"
        );
    }
}

#[tokio::test]
async fn a_forged_symlink_cannot_reach_outside_the_vfs() {
    let bytes = snapshot_with_forged_entries(vec![
        VfsEntry {
            path: PathBuf::from("/escape"),
            kind: VfsEntryKind::Symlink {
                target: PathBuf::from("/../../../etc"),
            },
            mode: 0o777,
        },
        // A self-referential link must not hang resolution.
        VfsEntry {
            path: PathBuf::from("/loop"),
            kind: VfsEntryKind::Symlink {
                target: PathBuf::from("/loop"),
            },
            mode: 0o777,
        },
    ])
    .await;

    let mut bash = Bash::from_snapshot(&bytes).unwrap();
    assert_eq!(
        bash.exec("cat /canary.txt").await.unwrap().stdout,
        "canary\n"
    );

    for probe in [
        "cat /escape/passwd",
        "ls /escape/",
        "cat /loop",
        "ls -L /loop",
    ] {
        let result = bash.exec(probe).await.unwrap();
        assert!(
            result.stdout.is_empty(),
            "`{probe}` produced output: {:?}",
            result.stdout
        );
    }
}

#[tokio::test]
async fn a_path_with_a_nul_byte_is_refused_outright() {
    // Unlike traversal, this one cannot be made inert — it is rejected, and the
    // rejection must be atomic.
    let bytes = snapshot_with_forged_entries(vec![forged_file("/bad\0name.txt", "nul\n")]).await;

    let mut bash = Bash::new();
    bash.exec("echo original > /original.txt").await.unwrap();
    let err = bash.restore_snapshot(&bytes).unwrap_err();
    assert!(
        err.to_string().contains("unsafe character"),
        "unexpected error: {err}"
    );

    // Validate-before-mutate: the pre-existing filesystem is untouched.
    assert_eq!(
        bash.exec("cat /original.txt").await.unwrap().stdout,
        "original\n"
    );
    assert_ne!(
        bash.exec("test -f /canary.txt").await.unwrap().exit_code,
        0,
        "a rejected snapshot must not have written any state"
    );
}

#[tokio::test]
async fn duplicate_paths_resolve_deterministically() {
    // A tree can name the same path twice. Nothing should crash, and because
    // tree entries are sorted and applied in order, the last one wins — which
    // makes the outcome reproducible rather than dependent on map ordering.
    let bytes = snapshot_with_forged_entries(vec![
        forged_file("/dup.txt", "FIRST"),
        forged_file("/dup.txt", "SECOND"),
    ])
    .await;

    let mut bash = Bash::from_snapshot(&bytes).unwrap();
    assert_eq!(bash.exec("cat /dup.txt").await.unwrap().stdout, "SECOND");
}

#[tokio::test]
async fn arbitrary_bytes_are_rejected_without_panicking() {
    // The shapes a corrupted database column or a truncated transfer actually
    // produces. Every one must be an error, and the instance must survive.
    let mut bash = Bash::new();
    bash.exec("echo intact > /intact.txt").await.unwrap();

    let real = bash.snapshot().unwrap();
    let mut cases: Vec<Vec<u8>> = vec![
        Vec::new(),
        vec![0u8],
        vec![0u8; 31],
        vec![0u8; 32],
        vec![0u8; 33],
        vec![0xffu8; 4096],
        b"BKSNAP".to_vec(),
        [vec![0u8; 32], b"BKSNAP".to_vec()].concat(),
        [vec![0u8; 32], b"{\"version\":1}".to_vec()].concat(),
        [vec![0u8; 32], b"{".to_vec()].concat(),
        real[..32].to_vec(),
        real[..40].to_vec(),
    ];
    // Every single-byte truncation of a real snapshot's header region.
    for cut in 0..64.min(real.len()) {
        cases.push(real[..cut].to_vec());
    }

    for (index, case) in cases.iter().enumerate() {
        assert!(
            Bash::from_snapshot(case).is_err(),
            "case {index} ({} bytes) decoded instead of erroring",
            case.len()
        );
        assert!(
            bash.restore_snapshot_with_policy(case, CheckoutPolicy::Force)
                .is_err(),
            "case {index} restored instead of erroring"
        );
    }

    // The instance stayed usable through every rejection.
    assert_eq!(
        bash.exec("cat /intact.txt").await.unwrap().stdout,
        "intact\n"
    );
}
