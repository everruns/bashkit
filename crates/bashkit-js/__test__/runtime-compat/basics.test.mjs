// Core execution: constructors, echo, arithmetic, options, reset.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash, getVersion } from "./_setup.mjs";

describe("version", () => {
  it("getVersion returns a semver string", () => {
    assert.match(getVersion(), /^\d+\.\d+\.\d+/);
  });
});

describe("Bash basics", () => {
  it("default constructor", () => {
    assert.ok(new Bash());
  });

  it("echo command", () => {
    const bash = new Bash();
    const r = bash.executeSync('echo "hello"');
    assert.equal(r.exitCode, 0);
    assert.equal(r.stdout.trim(), "hello");
  });

  it("empty command", () => {
    assert.equal(new Bash().executeSync("").exitCode, 0);
  });

  it("true returns 0, false returns non-zero", () => {
    const bash = new Bash();
    assert.equal(bash.executeSync("true").exitCode, 0);
    assert.notEqual(bash.executeSync("false").exitCode, 0);
  });

  it("arithmetic", () => {
    const bash = new Bash();
    assert.equal(bash.executeSync("echo $((10 * 5 - 3))").stdout.trim(), "47");
    assert.equal(bash.executeSync("echo $((17 % 5))").stdout.trim(), "2");
  });

  it("constructor with options", () => {
    const bash = new Bash({
      username: "testuser",
      hostname: "testhost",
      maxCommands: 1000,
      maxLoopIterations: 500,
    });
    assert.equal(bash.executeSync("whoami").stdout.trim(), "testuser");
    assert.equal(bash.executeSync("hostname").stdout.trim(), "testhost");
  });
});

describe("variables and state", () => {
  it("variable assignment and expansion", () => {
    const bash = new Bash();
    bash.executeSync("NAME=world");
    assert.equal(bash.executeSync('echo "Hello $NAME"').stdout.trim(), "Hello world");
  });

  it("state persists between calls", () => {
    const bash = new Bash();
    bash.executeSync("X=42");
    assert.equal(bash.executeSync("echo $X").stdout.trim(), "42");
  });

  it("default value expansion", () => {
    const bash = new Bash();
    assert.equal(bash.executeSync("echo ${MISSING:-default}").stdout.trim(), "default");
  });

  it("string length", () => {
    const bash = new Bash();
    bash.executeSync("S=hello");
    assert.equal(bash.executeSync("echo ${#S}").stdout.trim(), "5");
  });

  it("prefix/suffix removal", () => {
    const bash = new Bash();
    bash.executeSync("F=path/to/file.txt");
    assert.equal(bash.executeSync("echo ${F##*/}").stdout.trim(), "file.txt");
    bash.executeSync("G=file.tar.gz");
    assert.equal(bash.executeSync("echo ${G%%.*}").stdout.trim(), "file");
  });

  it("string replacement", () => {
    const bash = new Bash();
    bash.executeSync("S='hello world hello'");
    assert.equal(bash.executeSync('echo "${S//hello/bye}"').stdout.trim(), "bye world bye");
  });

  it("uppercase/lowercase conversion", () => {
    const bash = new Bash();
    bash.executeSync("S=hello");
    assert.equal(bash.executeSync('echo "${S^^}"').stdout.trim(), "HELLO");
    bash.executeSync("U=HELLO");
    assert.equal(bash.executeSync('echo "${U,,}"').stdout.trim(), "hello");
  });

  it("arrays", () => {
    const bash = new Bash();
    bash.executeSync("ARR=(apple banana cherry)");
    assert.equal(bash.executeSync('echo "${ARR[0]}"').stdout.trim(), "apple");
    assert.equal(bash.executeSync('echo "${#ARR[@]}"').stdout.trim(), "3");
    bash.executeSync("ARR+=(date)");
    assert.equal(bash.executeSync('echo "${#ARR[@]}"').stdout.trim(), "4");
  });
});

describe("reset", () => {
  it("clears variables and files", () => {
    const bash = new Bash();
    bash.executeSync("X=42");
    bash.executeSync('echo "data" > /tmp/r.txt');
    bash.reset();
    assert.equal(bash.executeSync("echo ${X:-unset}").stdout.trim(), "unset");
    assert.notEqual(bash.executeSync("cat /tmp/r.txt 2>&1").exitCode, 0);
  });

  it("preserves config after reset", () => {
    const bash = new Bash({ username: "keeper" });
    bash.executeSync("X=gone");
    bash.reset();
    assert.equal(bash.executeSync("whoami").stdout.trim(), "keeper");
  });
});

describe("isolation", () => {
  it("Bash instances have isolated variables", () => {
    const a = new Bash();
    const b = new Bash();
    a.executeSync("X=from_a");
    b.executeSync("X=from_b");
    assert.equal(a.executeSync("echo $X").stdout.trim(), "from_a");
    assert.equal(b.executeSync("echo $X").stdout.trim(), "from_b");
  });

  it("Bash instances have isolated filesystems", () => {
    const a = new Bash();
    const b = new Bash();
    a.executeSync('echo "a" > /tmp/iso.txt');
    assert.notEqual(b.executeSync("cat /tmp/iso.txt 2>&1").exitCode, 0);
  });
});
