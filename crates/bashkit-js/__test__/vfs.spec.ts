import test from "ava";
import { Bash } from "../wrapper.js";

// ============================================================================
// VFS — readFile / writeFile
// ============================================================================

test("writeFile + readFile roundtrip", (t) => {
  const bash = new Bash();
  bash.writeFile("/tmp/hello.txt", "Hello, VFS!");
  t.is(bash.readFile("/tmp/hello.txt"), "Hello, VFS!");
});

test("writeFile overwrites existing content", (t) => {
  const bash = new Bash();
  bash.writeFile("/tmp/over.txt", "first");
  bash.writeFile("/tmp/over.txt", "second");
  t.is(bash.readFile("/tmp/over.txt"), "second");
});

test("readFile throws on missing file", (t) => {
  const bash = new Bash();
  t.throws(() => bash.readFile("/nonexistent/file.txt"));
});

test("writeFile preserves binary-like content", (t) => {
  const bash = new Bash();
  const content = "line1\nline2\n\ttabbed\n";
  bash.writeFile("/tmp/multi.txt", content);
  t.is(bash.readFile("/tmp/multi.txt"), content);
});

test("writeFile with empty content", (t) => {
  const bash = new Bash();
  bash.writeFile("/tmp/empty.txt", "");
  t.is(bash.readFile("/tmp/empty.txt"), "");
});

// ============================================================================
// VFS — mkdir
// ============================================================================

test("mkdir creates directory", (t) => {
  const bash = new Bash();
  bash.mkdir("/tmp/newdir");
  t.true(bash.exists("/tmp/newdir"));
});

test("mkdir recursive creates parent chain", (t) => {
  const bash = new Bash();
  bash.mkdir("/a/b/c/d", true);
  t.true(bash.exists("/a/b/c/d"));
  t.true(bash.exists("/a/b/c"));
  t.true(bash.exists("/a/b"));
});

test("mkdir non-recursive fails without parent", (t) => {
  const bash = new Bash();
  t.throws(() => bash.mkdir("/x/y/z"));
});

// ============================================================================
// VFS — exists
// ============================================================================

test("exists returns false for missing path", (t) => {
  const bash = new Bash();
  t.false(bash.exists("/does/not/exist"));
});

test("exists returns true for file", (t) => {
  const bash = new Bash();
  bash.writeFile("/tmp/e.txt", "x");
  t.true(bash.exists("/tmp/e.txt"));
});

test("exists returns true for directory", (t) => {
  const bash = new Bash();
  bash.mkdir("/tmp/edir");
  t.true(bash.exists("/tmp/edir"));
});

// ============================================================================
// VFS — remove
// ============================================================================

test("remove deletes a file", (t) => {
  const bash = new Bash();
  bash.writeFile("/tmp/rm.txt", "bye");
  t.true(bash.exists("/tmp/rm.txt"));
  bash.remove("/tmp/rm.txt");
  t.false(bash.exists("/tmp/rm.txt"));
});

test("remove recursive deletes directory tree", (t) => {
  const bash = new Bash();
  bash.mkdir("/tmp/tree/sub", true);
  bash.writeFile("/tmp/tree/sub/f.txt", "data");
  bash.remove("/tmp/tree", true);
  t.false(bash.exists("/tmp/tree"));
});

test("remove throws on missing path", (t) => {
  const bash = new Bash();
  t.throws(() => bash.remove("/no/such/file"));
});

// ============================================================================
// VFS ↔ bash interop
// ============================================================================

test("bash executeSync sees VFS-written files", (t) => {
  const bash = new Bash();
  bash.writeFile("/tmp/from-vfs.txt", "vfs-content");
  const r = bash.executeSync("cat /tmp/from-vfs.txt");
  t.is(r.stdout, "vfs-content");
});

test("readFile sees bash-created files", (t) => {
  const bash = new Bash();
  bash.executeSync("echo bash-content > /tmp/from-bash.txt");
  t.is(bash.readFile("/tmp/from-bash.txt"), "bash-content\n");
});

test("VFS mkdir makes directory visible to bash ls", (t) => {
  const bash = new Bash();
  bash.mkdir("/project/src/lib", true);
  bash.writeFile("/project/src/lib/mod.rs", "// rust");
  const r = bash.executeSync("ls /project/src/lib/");
  t.is(r.stdout.trim(), "mod.rs");
});

test("bash mkdir makes directory visible to VFS exists", (t) => {
  const bash = new Bash();
  bash.executeSync("mkdir -p /project/pkg");
  t.true(bash.exists("/project/pkg"));
});

test("reset clears VFS state", (t) => {
  const bash = new Bash();
  bash.writeFile("/tmp/persist.txt", "data");
  t.true(bash.exists("/tmp/persist.txt"));
  bash.reset();
  t.false(bash.exists("/tmp/persist.txt"));
});
