"""Snapshot history from Python: commits, forks, rewinds, and the object graph.

Mirrors the Rust integration coverage in
``crates/bashkit/tests/integration/snapshot_history_tests.rs`` at the binding
boundary — the concern here is that ids, object stores, and policies survive
the Python round trip, not that the graph itself is correct.
"""

import pytest

from bashkit import Bash, BashError, CapabilityFingerprint, SnapshotGraph


def commit_into(bash, store, parents=None, meta=None):
    """Commit into ``store``, reusing whatever it already holds."""
    packed = bash.commit(parents=parents, meta=meta, have=list(store))
    store.update(packed.objects)
    return packed


# ---------------------------------------------------------------- round trip


def test_commit_checkout_round_trip():
    store = {}
    bash = Bash()
    bash.execute_sync("X=42; mkdir -p /w; echo data > /w/f.txt; cd /w")
    packed = commit_into(bash, store)

    restored = Bash()
    restored.checkout(packed.id, store)

    assert restored.execute_sync("echo $X").stdout.strip() == "42"
    assert restored.execute_sync("pwd").stdout.strip() == "/w"
    assert restored.execute_sync("cat /w/f.txt").stdout == "data\n"


def test_commit_id_is_hex_and_stable_for_identical_state():
    a, b = Bash(), Bash()
    for bash in (a, b):
        bash.execute_sync("export Z=26; export A=1; echo same > /f.txt")

    first = a.commit()
    second = b.commit()
    assert first.id == second.id
    assert len(first.id) == 64
    assert int(first.id, 16) >= 0  # hex, parseable by any host database


def test_binary_content_round_trips():
    store = {}
    bash = Bash()
    bash.execute_sync("echo 'AAH//n+AAEFCCg0=' | base64 -d > /blob.bin")
    expected = bash.execute_sync("od -An -tx1 /blob.bin").stdout.split()
    assert expected == ["00", "01", "ff", "fe", "7f", "80", "00", "41", "42", "0a", "0d"]

    packed = commit_into(bash, store)
    restored = Bash()
    restored.checkout(packed.id, store)
    assert restored.execute_sync("od -An -tx1 /blob.bin").stdout.split() == expected


# --------------------------------------------------------------------- forks


def test_forks_diverge_without_contaminating_each_other():
    store = {}
    bash = Bash()
    bash.execute_sync("echo base > /log.txt")
    base = commit_into(bash, store).id

    left = Bash()
    left.checkout(base, store)
    left.execute_sync("echo left >> /log.txt")
    left_id = commit_into(left, store, parents=[base]).id

    right = Bash()
    right.checkout(base, store)
    right.execute_sync("echo right >> /log.txt")
    right_id = commit_into(right, store, parents=[base]).id

    check = Bash()
    check.checkout(left_id, store)
    assert check.execute_sync("cat /log.txt").stdout == "base\nleft\n"
    check.checkout(right_id, store)
    assert check.execute_sync("cat /log.txt").stdout == "base\nright\n"
    check.checkout(base, store)
    assert check.execute_sync("cat /log.txt").stdout == "base\n"

    assert SnapshotGraph.parents(left_id, store) == [base]
    assert SnapshotGraph.parents(right_id, store) == [base]


def test_incremental_commits_do_not_re_store_everything():
    store = {}
    bash = Bash(max_commands=10_000)
    bash.execute_sync("mkdir -p /w; for i in $(seq 1 30); do echo body-$i > /w/f$i.txt; done")
    first = commit_into(bash, store)
    assert first.is_self_contained  # no `have` was passed on the first commit

    bash.execute_sync("echo appended >> /w/f7.txt")
    second = bash.commit(parents=[first.id], have=list(store))

    assert second.object_count < first.object_count / 4
    assert not second.is_self_contained
    with pytest.raises(BashError, match="incremental"):
        second.to_bytes()


def test_self_contained_commit_packs_to_restorable_bytes():
    bash = Bash()
    bash.execute_sync("PACKED=yes; echo body > /f.txt")
    packed = bash.commit()

    restored = Bash()
    restored.restore_snapshot(packed.to_bytes())
    assert restored.execute_sync("echo $PACKED").stdout.strip() == "yes"
    assert restored.execute_sync("cat /f.txt").stdout == "body\n"


# ------------------------------------------------------------- graph queries


def test_ancestry_and_meta():
    store = {}
    bash = Bash()
    ids = []
    for step in range(4):
        bash.execute_sync(f"N={step}")
        parents = [ids[-1]] if ids else None
        ids.append(commit_into(bash, store, parents=parents, meta={"step": str(step)}).id)

    assert SnapshotGraph.ancestry(ids[-1], store) == list(reversed(ids))
    assert len(SnapshotGraph.ancestry(ids[-1], store, limit=2)) == 2
    assert SnapshotGraph.meta(ids[2], store)["step"] == "2"


def test_diff_reports_file_changes():
    store = {}
    bash = Bash()
    bash.execute_sync("mkdir -p /d; echo keep > /d/keep.txt; echo gone > /d/gone.txt")
    before = commit_into(bash, store).id

    bash.execute_sync("rm /d/gone.txt; echo new > /d/new.txt; echo edit > /d/keep.txt")
    after = commit_into(bash, store, parents=[before]).id

    diff = SnapshotGraph.diff(before, after, store)
    assert diff.files_added == ["/d/new.txt"]
    assert diff.files_modified == ["/d/keep.txt"]
    assert diff.files_removed == ["/d/gone.txt"]
    assert not diff.is_empty()
    assert SnapshotGraph.diff(before, before, store).is_empty()


def test_plan_checkout_converges_and_supports_lazy_fetch():
    remote = {}
    bash = Bash(max_commands=10_000)
    bash.execute_sync("mkdir -p /w; seq 1 20000 > /w/big.txt")
    commit_id = commit_into(bash, remote).id

    local = {}
    rounds = 0
    while True:
        needed = SnapshotGraph.plan_checkout(commit_id, local)
        if not needed:
            break
        rounds += 1
        assert rounds < 10, "plan_checkout failed to converge"
        local.update({oid: remote[oid] for oid in needed})

    assert rounds >= 2, "expected a multi-wave walk"
    restored = Bash()
    restored.checkout(commit_id, local)
    assert restored.execute_sync("wc -l < /w/big.txt").stdout.strip() == "20000"


def test_reachable_lists_objects_for_gc():
    store = {}
    bash = Bash()
    bash.execute_sync("echo x > /f.txt")
    commit_id = commit_into(bash, store).id

    live = SnapshotGraph.reachable(commit_id, store)
    assert commit_id in live
    assert set(live) <= set(store)


# ---------------------------------------------------------------- capability


def test_capabilities_readable_from_instance_and_commit():
    store = {}
    bash = Bash()
    commit_id = commit_into(bash, store).id

    from_commit = SnapshotGraph.capabilities(commit_id, store)
    from_live = bash.capabilities()
    assert isinstance(from_live, CapabilityFingerprint)
    assert from_commit.builtins == from_live.builtins
    assert from_commit.fs_backend == "in-memory"
    assert "echo" in from_commit.builtins


def test_checkout_policy_accepts_known_values_and_rejects_others():
    store = {}
    bash = Bash()
    commit_id = commit_into(bash, store).id

    for policy in ("strict", "superset", "force", "SUPERSET"):
        Bash().checkout(commit_id, store, policy=policy)

    with pytest.raises(ValueError, match="unknown checkout policy"):
        Bash().checkout(commit_id, store, policy="whatever")


# --------------------------------------------------------------- bad input


def test_malformed_commit_id_is_rejected():
    with pytest.raises(ValueError, match="invalid snapshot object id"):
        SnapshotGraph.parents("not-a-hash", {})


@pytest.mark.parametrize(
    "bad",
    [
        "é" * 32,  # 64 bytes, 32 characters
        "a" * 62 + "é",  # 64 bytes, trailing multi-byte character
        "é" + "a" * 62,  # 64 bytes, leading multi-byte character
        "\U00010348" * 16,  # 64 bytes of 4-byte characters
    ],
)
def test_non_ascii_commit_id_errors_instead_of_crashing(bad):
    # These are 64 *bytes* but not 64 hex characters. Parsing by byte offset
    # would split a character and abort the interpreter rather than raising.
    assert len(bad.encode()) == 64
    with pytest.raises(ValueError, match="invalid snapshot object id"):
        SnapshotGraph.parents(bad, {})
    with pytest.raises(ValueError, match="invalid snapshot object id"):
        Bash().checkout(bad, {})


def test_commit_rejects_more_parents_than_checkout_accepts():
    store = {}
    bash = Bash()
    base = commit_into(bash, store).id

    with pytest.raises(BashError, match="parents"):
        bash.commit(parents=[base] * 65)

    # 64 is the documented maximum and must still round-trip.
    packed = bash.commit(parents=[base] * 64)
    store.update(packed.objects)
    assert len(SnapshotGraph.parents(packed.id, store)) == 64


def test_unknown_commit_id_raises():
    with pytest.raises(BashError):
        SnapshotGraph.parents("ab" * 32, {})


def test_missing_object_is_reported():
    store = {}
    bash = Bash()
    bash.execute_sync("mkdir -p /w; echo content > /w/f.txt")
    commit_id = commit_into(bash, store).id

    for victim in [oid for oid in store if oid != commit_id]:
        incomplete = {k: v for k, v in store.items() if k != victim}
        with pytest.raises(BashError):
            Bash().checkout(commit_id, incomplete, policy="force")


def test_tampered_object_is_rejected():
    store = {}
    bash = Bash()
    bash.execute_sync("echo secret > /f.txt")
    commit_id = commit_into(bash, store).id

    for victim in store:
        poisoned = dict(store)
        blob = bytearray(poisoned[victim])
        blob[1] ^= 0xFF  # index 0 is the compression flag
        poisoned[victim] = bytes(blob)
        with pytest.raises(BashError):
            Bash().checkout(commit_id, poisoned, policy="force")


def test_failed_checkout_leaves_the_instance_untouched():
    store = {}
    donor = Bash()
    donor.execute_sync("echo intruder > /intruder.txt")
    commit_id = commit_into(donor, store).id
    broken = {k: v for k, v in store.items() if k != commit_id}
    # Drop something the checkout needs, so it fails partway through planning.
    broken.pop(next(iter(broken)))

    bash = Bash()
    bash.execute_sync("echo original > /original.txt; KEEP=yes")
    with pytest.raises(BashError):
        bash.checkout(commit_id, broken, policy="force")

    assert bash.execute_sync("cat /original.txt").stdout == "original\n"
    assert bash.execute_sync("echo $KEEP").stdout.strip() == "yes"
    assert bash.execute_sync("test -f /intruder.txt").exit_code != 0


def test_snapshot_graph_cannot_be_instantiated():
    with pytest.raises(TypeError, match="namespace"):
        SnapshotGraph()
