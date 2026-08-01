// Snapshot history across Node, Bun, and Deno.
//
// The AVA suite covers this API in depth on Node; this file exists because the
// object store crosses the FFI boundary as `Record<string, Buffer>`, and Buffer
// is exactly the kind of type whose behaviour differs between runtimes. The
// cases here are the ones where a runtime difference would actually show:
// binary blobs surviving a round trip, byte-level mutation being detected, and
// hex ids staying stable.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  Bash,
  snapshotAncestry,
  snapshotCapabilities,
  snapshotDiff,
  snapshotMeta,
  snapshotParents,
  snapshotPlanCheckout,
  snapshotReachable,
} from "./_setup.mjs";

/** Commit into `store`, reusing whatever it already holds. */
function commitInto(bash, store, parents, meta) {
  const packed = bash.commit({ parents, meta, have: Object.keys(store) });
  Object.assign(store, packed.objects);
  return packed;
}

describe("snapshot history", () => {
  it("round-trips shell state and files through a commit", () => {
    const store = {};
    const bash = new Bash();
    bash.executeSync("X=42; mkdir -p /w; echo data > /w/f.txt; cd /w");
    const packed = commitInto(bash, store);

    assert.equal(packed.id.length, 64);
    assert.match(packed.id, /^[0-9a-f]{64}$/);
    assert.equal(packed.selfContained, true);

    const restored = new Bash();
    restored.checkout(packed.id, store);
    assert.equal(restored.executeSync("echo $X").stdout.trim(), "42");
    assert.equal(restored.executeSync("pwd").stdout.trim(), "/w");
    assert.equal(restored.executeSync("cat /w/f.txt").stdout, "data\n");
  });

  it("carries binary file content across the FFI boundary intact", () => {
    const store = {};
    const bash = new Bash();
    bash.executeSync("echo 'AAH//n+AAEFCCg0=' | base64 -d > /blob.bin");
    const expected = bash.executeSync("od -An -tx1 /blob.bin").stdout.split(/\s+/).filter(Boolean);
    assert.deepEqual(expected, [
      "00", "01", "ff", "fe", "7f", "80", "00", "41", "42", "0a", "0d",
    ]);

    const packed = commitInto(bash, store);
    const restored = new Bash();
    restored.checkout(packed.id, store);
    const actual = restored.executeSync("od -An -tx1 /blob.bin").stdout.split(/\s+/).filter(Boolean);
    assert.deepEqual(actual, expected);
  });

  it("stores objects as byte containers this runtime can mutate", () => {
    // Whatever the runtime hands back for a Buffer, indexing it must reach the
    // underlying bytes — otherwise the tamper check below is vacuous.
    const store = {};
    const bash = new Bash();
    bash.executeSync("echo secret > /f.txt");
    const commitId = commitInto(bash, store).id;

    for (const [id, blob] of Object.entries(store)) {
      assert.equal(typeof blob.length, "number");
      assert.ok(blob.length > 0, `object ${id} is empty`);
    }

    const victim = Object.keys(store)[0];
    const poisoned = { ...store };
    const bytes = Uint8Array.from(poisoned[victim]);
    bytes[1] ^= 0xff; // index 0 is the compression flag
    poisoned[victim] = bytes;
    assert.throws(() => new Bash().checkout(commitId, poisoned, "force"));
  });

  it("forks from a non-tip commit without contaminating siblings", () => {
    const store = {};
    const bash = new Bash();
    bash.executeSync("echo base > /log.txt");
    const base = commitInto(bash, store).id;

    const left = new Bash();
    left.checkout(base, store);
    left.executeSync("echo left >> /log.txt");
    const leftId = commitInto(left, store, [base]).id;

    const right = new Bash();
    right.checkout(base, store);
    right.executeSync("echo right >> /log.txt");
    const rightId = commitInto(right, store, [base]).id;

    const check = new Bash();
    check.checkout(leftId, store);
    assert.equal(check.executeSync("cat /log.txt").stdout, "base\nleft\n");
    check.checkout(rightId, store);
    assert.equal(check.executeSync("cat /log.txt").stdout, "base\nright\n");
    check.checkout(base, store);
    assert.equal(check.executeSync("cat /log.txt").stdout, "base\n");

    assert.deepEqual(snapshotParents(leftId, store), [base]);
    assert.deepEqual(snapshotParents(rightId, store), [base]);
  });

  it("keeps incremental commits small and unpackable", () => {
    const store = {};
    const bash = new Bash({ maxCommands: 10_000 });
    bash.executeSync("mkdir -p /w; for i in $(seq 1 30); do echo body-$i > /w/f$i.txt; done");
    const first = commitInto(bash, store);

    bash.executeSync("echo appended >> /w/f7.txt");
    const second = bash.commit({ parents: [first.id], have: Object.keys(store) });

    assert.ok(second.objectCount < first.objectCount / 4);
    assert.equal(second.selfContained, false);
    assert.equal(second.packed, null);
  });

  it("exposes graph queries", () => {
    const store = {};
    const bash = new Bash();
    const ids = [];
    for (let step = 0; step < 3; step++) {
      bash.executeSync(`N=${step}; echo ${step} > /step.txt`);
      const parents = ids.length ? [ids[ids.length - 1]] : undefined;
      ids.push(commitInto(bash, store, parents, { step: String(step) }).id);
    }

    assert.deepEqual(snapshotAncestry(ids[2], store), [...ids].reverse());
    assert.equal(snapshotAncestry(ids[2], store, 2).length, 2);
    assert.equal(snapshotMeta(ids[1], store).step, "1");

    const diff = snapshotDiff(ids[0], ids[1], store);
    assert.deepEqual(diff.filesModified, ["/step.txt"]);
    assert.equal(diff.shellChanged, true);

    assert.ok(snapshotReachable(ids[2], store).includes(ids[2]));
    assert.deepEqual(snapshotPlanCheckout(ids[2], store), []);

    const caps = snapshotCapabilities(ids[2], store);
    assert.equal(caps.fsBackend, "in-memory");
    assert.ok(caps.builtins.includes("echo"));
    assert.deepEqual(caps.builtins, new Bash().capabilities().builtins);
  });

  it("fetches objects lazily until the plan is empty", () => {
    const remote = {};
    const bash = new Bash({ maxCommands: 10_000 });
    bash.executeSync("mkdir -p /w; seq 1 20000 > /w/big.txt");
    const commitId = commitInto(bash, remote).id;

    const local = {};
    let rounds = 0;
    for (;;) {
      const needed = snapshotPlanCheckout(commitId, local);
      if (needed.length === 0) break;
      rounds += 1;
      assert.ok(rounds < 10, "plan_checkout failed to converge");
      for (const id of needed) local[id] = remote[id];
    }

    assert.ok(rounds >= 2, "expected a multi-wave walk");
    const restored = new Bash();
    restored.checkout(commitId, local);
    assert.equal(restored.executeSync("wc -l < /w/big.txt").stdout.trim(), "20000");
  });

  it("rejects malformed ids, including non-ASCII", () => {
    assert.throws(() => snapshotParents("not-a-hash", {}), /invalid snapshot object id/);
    // 64 bytes of UTF-8 but not 64 characters: must error, never crash.
    assert.throws(
      () => snapshotParents("é".repeat(32), {}),
      /invalid snapshot object id/,
    );
    assert.throws(() => snapshotParents("a".repeat(63) + "é", {}), /invalid snapshot object id/);
  });

  it("rejects an unknown checkout policy", () => {
    const store = {};
    const bash = new Bash();
    const commitId = commitInto(bash, store).id;
    assert.throws(
      () => new Bash().checkout(commitId, store, "whatever"),
      /unknown checkout policy/,
    );
  });

  it("leaves the instance untouched when a checkout fails", () => {
    const store = {};
    const donor = new Bash();
    donor.executeSync("echo intruder > /intruder.txt");
    const commitId = commitInto(donor, store).id;

    const broken = { ...store };
    delete broken[Object.keys(broken).find((id) => id !== commitId)];

    const bash = new Bash();
    bash.executeSync("echo original > /original.txt; KEEP=yes");
    assert.throws(() => bash.checkout(commitId, broken, "force"));

    assert.equal(bash.executeSync("cat /original.txt").stdout, "original\n");
    assert.equal(bash.executeSync("echo $KEEP").stdout.trim(), "yes");
    assert.notEqual(bash.executeSync("test -f /intruder.txt").exitCode, 0);
  });
});
