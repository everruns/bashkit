// VFS API: writeFile, readFile, mkdir, exists, remove, bash interop, reset.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash } from "./_setup.mjs";

describe("VFS API", () => {
  it("writeFile + readFile roundtrip", () => {
    const bash = new Bash();
    bash.writeFile("/tmp/hello.txt", "Hello, VFS!");
    assert.equal(bash.readFile("/tmp/hello.txt"), "Hello, VFS!");
  });

  it("writeFile overwrites", () => {
    const bash = new Bash();
    bash.writeFile("/tmp/o.txt", "first");
    bash.writeFile("/tmp/o.txt", "second");
    assert.equal(bash.readFile("/tmp/o.txt"), "second");
  });

  it("readFile throws on missing file", () => {
    assert.throws(() => new Bash().readFile("/nonexistent/file.txt"));
  });

  it("mkdir, exists, remove", () => {
    const bash = new Bash();
    bash.mkdir("/tmp/vdir");
    assert.ok(bash.exists("/tmp/vdir"));
    bash.writeFile("/tmp/vdir/f.txt", "data");
    assert.ok(bash.exists("/tmp/vdir/f.txt"));
    bash.remove("/tmp/vdir", true);
    assert.ok(!bash.exists("/tmp/vdir"));
  });

  it("mkdir recursive", () => {
    const bash = new Bash();
    bash.mkdir("/a/b/c/d", true);
    assert.ok(bash.exists("/a/b/c/d"));
    assert.ok(bash.exists("/a/b"));
  });

  it("VFS ↔ bash interop", () => {
    const bash = new Bash();
    bash.writeFile("/tmp/from-vfs.txt", "vfs-content");
    assert.equal(bash.executeSync("cat /tmp/from-vfs.txt").stdout, "vfs-content");
    bash.executeSync("echo bash-content > /tmp/from-bash.txt");
    assert.equal(bash.readFile("/tmp/from-bash.txt"), "bash-content\n");
  });

  it("reset clears VFS state", () => {
    const bash = new Bash();
    bash.writeFile("/tmp/p.txt", "data");
    assert.ok(bash.exists("/tmp/p.txt"));
    bash.reset();
    assert.ok(!bash.exists("/tmp/p.txt"));
  });
});
