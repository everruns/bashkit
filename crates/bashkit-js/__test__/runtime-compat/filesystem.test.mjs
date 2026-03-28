// Filesystem operations: read, write, append, mkdir, cp, mv, rm, cd, pwd,
// pipes, redirection, command substitution, heredocs.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash } from "./_setup.mjs";

describe("filesystem", () => {
  it("write, read, append", () => {
    const bash = new Bash();
    bash.executeSync('echo "line1" > /tmp/f.txt');
    bash.executeSync('echo "line2" >> /tmp/f.txt');
    const r = bash.executeSync("cat /tmp/f.txt");
    assert.ok(r.stdout.includes("line1"));
    assert.ok(r.stdout.includes("line2"));
  });

  it("mkdir, touch, ls", () => {
    const bash = new Bash();
    bash.executeSync("mkdir -p /tmp/d/sub");
    bash.executeSync("touch /tmp/d/sub/file.txt");
    assert.ok(bash.executeSync("ls /tmp/d/sub").stdout.includes("file.txt"));
  });

  it("cp and mv", () => {
    const bash = new Bash();
    bash.executeSync('echo "data" > /tmp/src.txt');
    bash.executeSync("cp /tmp/src.txt /tmp/cp.txt");
    assert.equal(bash.executeSync("cat /tmp/cp.txt").stdout.trim(), "data");
    bash.executeSync("mv /tmp/cp.txt /tmp/mv.txt");
    assert.equal(bash.executeSync("cat /tmp/mv.txt").stdout.trim(), "data");
    assert.notEqual(bash.executeSync("cat /tmp/cp.txt 2>&1").exitCode, 0);
  });

  it("rm and test flags", () => {
    const bash = new Bash();
    bash.executeSync("touch /tmp/rm.txt");
    assert.equal(bash.executeSync("test -f /tmp/rm.txt && echo yes").stdout.trim(), "yes");
    bash.executeSync("rm /tmp/rm.txt");
    assert.notEqual(bash.executeSync("test -f /tmp/rm.txt").exitCode, 0);
  });

  it("cd and pwd", () => {
    const bash = new Bash();
    bash.executeSync("mkdir -p /tmp/nav");
    bash.executeSync("cd /tmp/nav");
    assert.equal(bash.executeSync("pwd").stdout.trim(), "/tmp/nav");
  });
});

describe("pipes and redirection", () => {
  it("pipe echo to grep", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync('echo -e "foo\\nbar\\nbaz" | grep bar').stdout.trim(),
      "bar",
    );
  });

  it("pipe chain", () => {
    const bash = new Bash();
    assert.equal(
      bash.executeSync('echo -e "c\\na\\nb" | sort | head -1').stdout.trim(),
      "a",
    );
  });

  it("stderr redirect", () => {
    const r = new Bash().executeSync("echo err >&2");
    assert.ok(r.stderr.includes("err"));
  });

  it("command substitution", () => {
    assert.equal(
      new Bash().executeSync('echo "result: $(echo 42)"').stdout.trim(),
      "result: 42",
    );
  });

  it("heredoc", () => {
    const bash = new Bash();
    bash.executeSync("NAME=alice");
    const r = bash.executeSync("cat <<EOF\nhello $NAME\nEOF");
    assert.equal(r.stdout.trim(), "hello alice");
  });
});
