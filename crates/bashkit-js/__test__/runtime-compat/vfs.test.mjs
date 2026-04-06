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

  it("stat returns file metadata", () => {
    const bash = new Bash();
    bash.writeFile("/tmp/stat.txt", "hello");
    const meta = bash.stat("/tmp/stat.txt");
    assert.equal(meta.fileType, "file");
    assert.equal(meta.size, 5);
    assert.ok(meta.mode > 0);
  });

  it("stat returns directory metadata", () => {
    const bash = new Bash();
    bash.mkdir("/tmp/statdir");
    const meta = bash.stat("/tmp/statdir");
    assert.equal(meta.fileType, "directory");
  });

  it("readDir returns entries with metadata", () => {
    const bash = new Bash();
    bash.mkdir("/tmp/rd");
    bash.writeFile("/tmp/rd/file.txt", "content");
    bash.mkdir("/tmp/rd/sub");
    const entries = bash.readDir("/tmp/rd");
    assert.ok(Array.isArray(entries));
    assert.equal(entries.length, 2);
    const file = entries.find((e) => e.name === "file.txt");
    const dir = entries.find((e) => e.name === "sub");
    assert.ok(file);
    assert.ok(dir);
    assert.equal(file.metadata.fileType, "file");
    assert.equal(dir.metadata.fileType, "directory");
  });

  it("appendFile appends content", () => {
    const bash = new Bash();
    bash.writeFile("/tmp/ap.txt", "first");
    bash.appendFile("/tmp/ap.txt", "-second");
    assert.equal(bash.readFile("/tmp/ap.txt"), "first-second");
  });

  it("chmod changes file mode", () => {
    const bash = new Bash();
    bash.writeFile("/tmp/ch.txt", "data");
    bash.chmod("/tmp/ch.txt", 0o755);
    const meta = bash.stat("/tmp/ch.txt");
    assert.equal(meta.mode, 0o755);
  });

  it("symlink and readLink roundtrip", () => {
    const bash = new Bash();
    bash.writeFile("/tmp/target.txt", "data");
    bash.symlink("/tmp/target.txt", "/tmp/link.txt");
    assert.equal(bash.readLink("/tmp/link.txt"), "/tmp/target.txt");
    const meta = bash.stat("/tmp/link.txt");
    assert.equal(meta.fileType, "symlink");
  });

  it("fs() accessor provides same operations", () => {
    const bash = new Bash();
    const fs = bash.fs();
    fs.writeFile("/tmp/fsapi.txt", "via-fs");
    assert.equal(fs.readFile("/tmp/fsapi.txt"), "via-fs");
    const meta = fs.stat("/tmp/fsapi.txt");
    assert.equal(meta.fileType, "file");
    const entries = fs.readDir("/tmp");
    assert.ok(entries.some((e) => e.name === "fsapi.txt"));
  });
});
