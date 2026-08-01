import test from "ava";

import type { CheckoutPolicy } from "../wrapper.js";
import {
  Bash,
  snapshotAncestry,
  snapshotCapabilities,
  snapshotDiff,
  snapshotMeta,
  snapshotParents,
  snapshotPlanCheckout,
  snapshotReachable,
} from "../wrapper.js";

type Store = Record<string, Buffer>;

/** Commit into `store`, reusing whatever it already holds. */
function commitInto(bash: Bash, store: Store, parents?: string[], meta?: Record<string, string>) {
  const packed = bash.commit({ parents, meta, have: Object.keys(store) });
  Object.assign(store, packed.objects);
  return packed;
}

test("commit and checkout round-trip shell state and files", (t) => {
  const store: Store = {};
  const bash = new Bash();
  bash.executeSync("X=42; mkdir -p /w; echo data > /w/f.txt; cd /w");
  const packed = commitInto(bash, store);

  t.is(packed.id.length, 64);
  t.true(packed.selfContained);

  const restored = new Bash();
  restored.checkout(packed.id, store);
  t.is(restored.executeSync("echo $X").stdout.trim(), "42");
  t.is(restored.executeSync("pwd").stdout.trim(), "/w");
  t.is(restored.executeSync("cat /w/f.txt").stdout, "data\n");
});

test("identical state produces an identical commit id", (t) => {
  const a = new Bash();
  const b = new Bash();
  for (const bash of [a, b]) {
    bash.executeSync("export Z=26; export A=1; echo same > /f.txt");
  }
  t.is(a.commit().id, b.commit().id);
});

test("forks diverge without contaminating each other", (t) => {
  const store: Store = {};
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
  t.is(check.executeSync("cat /log.txt").stdout, "base\nleft\n");
  check.checkout(rightId, store);
  t.is(check.executeSync("cat /log.txt").stdout, "base\nright\n");
  check.checkout(base, store);
  t.is(check.executeSync("cat /log.txt").stdout, "base\n");

  t.deepEqual(snapshotParents(leftId, store), [base]);
  t.deepEqual(snapshotParents(rightId, store), [base]);
});

test("incremental commits do not re-store unchanged content", (t) => {
  const store: Store = {};
  const bash = new Bash({ maxCommands: 10_000 });
  bash.executeSync("mkdir -p /w; for i in $(seq 1 30); do echo body-$i > /w/f$i.txt; done");
  const first = commitInto(bash, store);

  bash.executeSync("echo appended >> /w/f7.txt");
  const second = bash.commit({ parents: [first.id], have: Object.keys(store) });

  t.true(second.objectCount < first.objectCount / 4);
  t.false(second.selfContained);
  // An incremental commit omits objects, so there is nothing safe to pack.
  t.is(second.packed, null);
});

test("a self-contained commit packs to restorable bytes", (t) => {
  const bash = new Bash();
  bash.executeSync("PACKED=yes; echo body > /f.txt");
  const packed = bash.commit();
  t.not(packed.packed, null);

  const restored = new Bash();
  restored.restoreSnapshot(packed.packed!);
  t.is(restored.executeSync("echo $PACKED").stdout.trim(), "yes");
});

test("diff reports adds, modifications, and removals", (t) => {
  const store: Store = {};
  const bash = new Bash();
  bash.executeSync("mkdir -p /d; echo keep > /d/keep.txt; echo gone > /d/gone.txt");
  const before = commitInto(bash, store).id;

  bash.executeSync("rm /d/gone.txt; echo new > /d/new.txt; echo edit > /d/keep.txt");
  const after = commitInto(bash, store, [before]).id;

  const diff = snapshotDiff(before, after, store);
  t.deepEqual(diff.filesAdded, ["/d/new.txt"]);
  t.deepEqual(diff.filesModified, ["/d/keep.txt"]);
  t.deepEqual(diff.filesRemoved, ["/d/gone.txt"]);
});

test("ancestry walks newest-first and carries metadata", (t) => {
  const store: Store = {};
  const bash = new Bash();
  const ids: string[] = [];
  for (let step = 0; step < 4; step++) {
    bash.executeSync(`N=${step}`);
    const parents = ids.length ? [ids[ids.length - 1]] : undefined;
    ids.push(commitInto(bash, store, parents, { step: String(step) }).id);
  }

  t.deepEqual(snapshotAncestry(ids[ids.length - 1], store), [...ids].reverse());
  t.is(snapshotAncestry(ids[ids.length - 1], store, 2).length, 2);
  t.is(snapshotMeta(ids[2], store).step, "2");
});

test("plan_checkout converges so objects can be fetched lazily", (t) => {
  const remote: Store = {};
  const bash = new Bash({ maxCommands: 10_000 });
  bash.executeSync("mkdir -p /w; seq 1 20000 > /w/big.txt");
  const commitId = commitInto(bash, remote).id;

  const local: Store = {};
  let rounds = 0;
  for (;;) {
    const needed = snapshotPlanCheckout(commitId, local);
    if (needed.length === 0) break;
    rounds += 1;
    t.true(rounds < 10, "plan_checkout failed to converge");
    for (const id of needed) local[id] = remote[id];
  }

  t.true(rounds >= 2, "expected a multi-wave walk");
  const restored = new Bash();
  restored.checkout(commitId, local);
  t.is(restored.executeSync("wc -l < /w/big.txt").stdout.trim(), "20000");
});

test("capabilities are readable from a commit and from a live instance", (t) => {
  const store: Store = {};
  const bash = new Bash();
  const commitId = commitInto(bash, store).id;

  const fromCommit = snapshotCapabilities(commitId, store);
  const fromLive = bash.capabilities();
  t.deepEqual(fromCommit.builtins, fromLive.builtins);
  t.is(fromCommit.fsBackend, "in-memory");
  t.true(fromCommit.builtins.includes("echo"));
});

test("reachable reports the object set a commit needs", (t) => {
  const store: Store = {};
  const bash = new Bash();
  bash.executeSync("echo x > /f.txt");
  const commitId = commitInto(bash, store).id;

  const live = snapshotReachable(commitId, store);
  t.true(live.includes(commitId));
  t.true(live.every((id) => id in store));
});

test("bad ids, unknown policies, and tampered objects are rejected", (t) => {
  const store: Store = {};
  const bash = new Bash();
  bash.executeSync("echo secret > /f.txt");
  const commitId = commitInto(bash, store).id;

  t.throws(() => snapshotParents("not-a-hash", {}), { message: /invalid snapshot object id/ });
  // The cast is the point: TypeScript rejects this at compile time, and the
  // binding must also reject it at runtime for callers coming from plain JS.
  t.throws(
    () => new Bash().checkout(commitId, store, "whatever" as CheckoutPolicy),
    { message: /unknown checkout policy/ },
  );

  for (const victim of Object.keys(store)) {
    const poisoned: Store = { ...store };
    const blob = Buffer.from(poisoned[victim]);
    blob[1] ^= 0xff; // index 0 is the compression flag
    poisoned[victim] = blob;
    t.throws(() => new Bash().checkout(commitId, poisoned, "force"));
  }
});

test("a failed checkout leaves the instance untouched", (t) => {
  const store: Store = {};
  const donor = new Bash();
  donor.executeSync("echo intruder > /intruder.txt");
  const commitId = commitInto(donor, store).id;

  const broken: Store = { ...store };
  delete broken[Object.keys(broken).find((id) => id !== commitId)!];

  const bash = new Bash();
  bash.executeSync("echo original > /original.txt; KEEP=yes");
  t.throws(() => bash.checkout(commitId, broken, "force"));

  t.is(bash.executeSync("cat /original.txt").stdout, "original\n");
  t.is(bash.executeSync("echo $KEEP").stdout.trim(), "yes");
  t.not(bash.executeSync("test -f /intruder.txt").exitCode, 0);
});
