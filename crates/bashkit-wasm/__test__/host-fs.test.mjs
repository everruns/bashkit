// Integration tests for the host-backed filesystem (`new Bash({ fs })`).
//
// The fake host below is deliberately async on every method: it is the shape a
// real embedder has (a Durable Object, IndexedDB, OPFS), and it proves the
// interpreter suspends and resumes correctly across host calls rather than
// relying on an accidentally-synchronous store.
//
// Run against the built package like the rest of the suite:
//
//   node --test crates/bashkit-wasm/__test__/

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { initBashkit, Bash } from "../pkg/index.js";

const wasmBytes = readFileSync(
  fileURLToPath(new URL("../pkg/bashkit_wasm_bg.wasm", import.meta.url)),
);
await initBashkit(wasmBytes);

// --- Fake host -------------------------------------------------------------

const encoder = new TextEncoder();
const decoder = new TextDecoder();

function fsError(code, message) {
  const error = new Error(message);
  error.code = code;
  return error;
}

// Minimal async host filesystem over a Map. Implements only the required
// methods, so `append`, `copy`, `rename`, and `chmod` exercise the synthesized
// fallbacks in the bridge.
class FakeHost {
  // path -> {type, content?, mode, mtimeMs}
  entries = new Map();
  calls = [];

  constructor(seed = {}) {
    for (const path of ["/", "/tmp", "/dev", "/home", "/home/user", "/workspace"]) {
      this.entries.set(path, { type: "dir", mode: 0o755, mtimeMs: 0 });
    }
    // Scripts redirect to /dev/null often enough that a host which omits it
    // looks broken. Hosts are expected to provide it; this one does.
    this.entries.set("/dev/null", { type: "file", content: new Uint8Array(), mode: 0o666 });
    for (const [path, text] of Object.entries(seed)) {
      this.entries.set(path, { type: "file", content: encoder.encode(text), mode: 0o644 });
    }
  }

  text(path) {
    const entry = this.entries.get(path);
    return entry?.content === undefined ? undefined : decoder.decode(entry.content);
  }

  // Every method resolves on a later microtask so nothing can complete in the
  // first poll — the suspension path is the one under test.
  async #tick(name) {
    this.calls.push(name);
    await Promise.resolve();
  }

  async read(path) {
    await this.#tick("read");
    const entry = this.entries.get(path);
    if (entry === undefined) throw fsError("ENOENT", `no such file: ${path}`);
    if (entry.type === "dir") throw fsError("EISDIR", `is a directory: ${path}`);
    return entry.content;
  }

  async write(path, content) {
    await this.#tick("write");
    if (path === "/dev/null") return;
    this.entries.set(path, { type: "file", content, mode: 0o644, mtimeMs: 0 });
  }

  async mkdir(path, recursive) {
    await this.#tick("mkdir");
    if (this.entries.has(path)) {
      if (recursive) return;
      throw fsError("EEXIST", `exists: ${path}`);
    }
    if (recursive) {
      const parts = path.split("/").filter(Boolean);
      let prefix = "";
      for (const part of parts) {
        prefix += `/${part}`;
        if (!this.entries.has(prefix)) {
          this.entries.set(prefix, { type: "dir", mode: 0o755, mtimeMs: 0 });
        }
      }
      return;
    }
    this.entries.set(path, { type: "dir", mode: 0o755, mtimeMs: 0 });
  }

  async remove(path, recursive) {
    await this.#tick("remove");
    if (!this.entries.has(path)) throw fsError("ENOENT", `no such file: ${path}`);
    this.entries.delete(path);
    if (!recursive) return;
    for (const key of [...this.entries.keys()]) {
      if (key.startsWith(`${path}/`)) this.entries.delete(key);
    }
  }

  async stat(path) {
    await this.#tick("stat");
    const entry = this.entries.get(path);
    if (entry === undefined) throw fsError("ENOENT", `no such file: ${path}`);
    return {
      type: entry.type,
      size: entry.content?.length ?? 0,
      mode: entry.mode,
      mtimeMs: entry.mtimeMs ?? 0,
    };
  }

  async readDir(path) {
    await this.#tick("readDir");
    if (!this.entries.has(path)) throw fsError("ENOENT", `no such file: ${path}`);
    const prefix = path === "/" ? "/" : `${path}/`;
    const out = [];
    for (const [key, entry] of this.entries) {
      if (key === path || !key.startsWith(prefix)) continue;
      const rest = key.slice(prefix.length);
      if (rest.length === 0 || rest.includes("/")) continue;
      out.push({
        name: rest,
        type: entry.type,
        size: entry.content?.length ?? 0,
        mode: entry.mode,
        mtimeMs: entry.mtimeMs ?? 0,
      });
    }
    return out;
  }

  async exists(path) {
    await this.#tick("exists");
    return this.entries.has(path);
  }
}

const hostBash = (host, options = {}) =>
  new Bash({ fs: host, cwd: "/workspace", ...options });

// --- Reads and writes ------------------------------------------------------

test("host fs: a redirect lands in the host store", async () => {
  const host = new FakeHost();
  const bash = hostBash(host);
  const r = await bash.execute('echo "hello" > /workspace/greeting.txt');
  assert.equal(r.stderr, "");
  assert.equal(r.exitCode, 0);
  assert.equal(host.text("/workspace/greeting.txt"), "hello\n");
});

test("host fs: reads come from the host store, not a copy", async () => {
  const host = new FakeHost({ "/workspace/seed.txt": "from the host\n" });
  const r = await hostBash(host).execute("cat /workspace/seed.txt");
  assert.equal(r.stdout, "from the host\n");
});

test("host fs: writes made between runs are visible to the next run", async () => {
  const host = new FakeHost();
  const bash = hostBash(host);
  await bash.execute("echo first > /workspace/log.txt");
  host.entries.set("/workspace/side.txt", {
    type: "file",
    content: encoder.encode("written behind bash's back\n"),
    mode: 0o644,
  });
  const r = await bash.execute("cat /workspace/side.txt");
  assert.equal(r.stdout, "written behind bash's back\n");
});

test("host fs: append redirect uses the synthesized append", async () => {
  const host = new FakeHost({ "/workspace/log.txt": "one\n" });
  const r = await hostBash(host).execute('echo two >> /workspace/log.txt');
  assert.equal(r.exitCode, 0);
  assert.equal(host.text("/workspace/log.txt"), "one\ntwo\n");
});

test("host fs: text tools run over host bytes", async () => {
  const host = new FakeHost({
    "/workspace/data.txt": "alpha 1\nbeta 2\ngamma 3\n",
  });
  const r = await hostBash(host).execute(
    "grep -c . /workspace/data.txt && sed -n 2p /workspace/data.txt",
  );
  assert.equal(r.stdout, "3\nbeta 2\n");
});

test("host fs: jq reads a host file", async () => {
  const host = new FakeHost({ "/workspace/x.json": '{"a":{"b":41}}' });
  const r = await hostBash(host).execute("jq -c '.a.b + 1' /workspace/x.json");
  assert.equal(r.stdout.trim(), "42");
});

// --- Directories -----------------------------------------------------------

test("host fs: mkdir -p, ls, and rm -r", async () => {
  const host = new FakeHost();
  const bash = hostBash(host);
  const made = await bash.execute("mkdir -p /workspace/a/b && echo x > /workspace/a/b/f.txt");
  assert.equal(made.exitCode, 0, made.stderr);
  assert.equal(host.entries.has("/workspace/a/b"), true);

  const listed = await bash.execute("ls /workspace/a/b");
  assert.equal(listed.stdout, "f.txt\n");

  const removed = await bash.execute("rm -r /workspace/a");
  assert.equal(removed.exitCode, 0, removed.stderr);
  assert.equal(host.entries.has("/workspace/a/b/f.txt"), false);
});

test("host fs: cp and mv work through the synthesized fallbacks", async () => {
  const host = new FakeHost({ "/workspace/src.txt": "payload\n" });
  const bash = hostBash(host);
  const copied = await bash.execute("cp /workspace/src.txt /workspace/copy.txt");
  assert.equal(copied.exitCode, 0, copied.stderr);
  assert.equal(host.text("/workspace/copy.txt"), "payload\n");

  const moved = await bash.execute("mv /workspace/copy.txt /workspace/moved.txt");
  assert.equal(moved.exitCode, 0, moved.stderr);
  assert.equal(host.text("/workspace/moved.txt"), "payload\n");
  assert.equal(host.entries.has("/workspace/copy.txt"), false);
});

test("host fs: chmod is accepted when the host has no permission model", async () => {
  const host = new FakeHost({ "/workspace/build.sh": "echo hi\n" });
  const r = await hostBash(host).execute("chmod +x /workspace/build.sh");
  assert.equal(r.exitCode, 0, r.stderr);
});

// --- Error mapping ---------------------------------------------------------

test("host fs: an ENOENT host error is a miss, not a failure", async () => {
  const host = new FakeHost();
  const bash = hostBash(host);
  // Builtins that branch on the error kind (rather than surfacing the raw
  // message) must see NotFound and print bash's own wording.
  const listed = await bash.execute("ls /workspace/missing.txt");
  assert.notEqual(listed.exitCode, 0);
  assert.match(listed.stderr, /No such file or directory/);

  // `test -f` is pure kind/existence logic — a mapped ENOENT must not read as
  // an I/O failure.
  const tested = await bash.execute(
    "if [ -f /workspace/missing.txt ]; then echo yes; else echo no; fi",
  );
  assert.equal(tested.stdout, "no\n");
  assert.equal(tested.stderr, "");
});

test("host fs: an ENOENT host message reaches stderr for builtins that report it", async () => {
  const host = new FakeHost();
  const r = await hostBash(host).execute("cat /workspace/missing.txt");
  assert.notEqual(r.exitCode, 0);
  assert.match(r.stderr, /cat: \/workspace\/missing\.txt:/);
});

test("host fs: a host error without a code still surfaces its message", async () => {
  const host = new FakeHost();
  host.read = async () => {
    throw new Error("host store unreachable");
  };
  const r = await hostBash(host).execute("cat /workspace/anything.txt");
  assert.notEqual(r.exitCode, 0);
  assert.match(r.stderr, /host store unreachable/);
});

test("host fs: a rejected write fails the command, not the run", async () => {
  const host = new FakeHost();
  host.write = async () => {
    throw fsError("EACCES", "read-only workspace");
  };
  const r = await hostBash(host).execute(
    'echo nope > /workspace/f.txt; echo "still running"',
  );
  assert.match(r.stdout, /still running/);
  assert.match(r.stderr, /read-only workspace|Permission denied/);
});

// --- Construction contract -------------------------------------------------

test("host fs: executeSync reports the suspension instead of blocking", async () => {
  const host = new FakeHost({ "/workspace/a.txt": "x\n" });
  assert.throws(
    () => hostBash(host).executeSync("cat /workspace/a.txt"),
    /did not complete synchronously/,
  );
});

test("host fs: files and fs cannot be combined", () => {
  assert.throws(
    () => new Bash({ fs: new FakeHost(), files: { "/workspace/a.txt": "x" } }),
    /cannot be combined/,
  );
});

test("host fs: a missing required method is rejected at construction", () => {
  const complete = new FakeHost();
  // Plain object rather than a FakeHost subclass: methods on a prototype chain
  // resolve fine, so the only way to be genuinely missing one is to omit it.
  const partial = {
    read: (p) => complete.read(p),
    write: (p, c) => complete.write(p, c),
    mkdir: (p, r) => complete.mkdir(p, r),
    remove: (p, r) => complete.remove(p, r),
    stat: (p) => complete.stat(p),
    exists: (p) => complete.exists(p),
    // readDir deliberately absent
  };
  assert.throws(() => new Bash({ fs: partial }), /missing the required method 'readDir'/);
});

test("host fs: a class instance satisfies the method check via its prototype", () => {
  assert.doesNotThrow(() => new Bash({ fs: new FakeHost() }));
});

test("host fs: a non-object fs is rejected", () => {
  assert.throws(() => new Bash({ fs: 42 }), /must be an object/);
});

test("host fs: every call actually reached the host", async () => {
  const host = new FakeHost({ "/workspace/a.txt": "x\n" });
  await hostBash(host).execute("cat /workspace/a.txt > /workspace/b.txt");
  assert.ok(host.calls.includes("read"), "expected a host read");
  assert.ok(host.calls.includes("write"), "expected a host write");
});
