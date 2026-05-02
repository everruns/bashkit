// Embedded SQLite (Turso) via the `sqlite: true` constructor flag.
//
// Mirrors the Python binding tests: opt-in gate, basic queries, VFS
// persistence across `executeSync()` calls, dot-commands, output
// formatting, and the security policies (ATTACH/DETACH + PRAGMA deny).

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash } from "./_setup.mjs";

describe("sqlite opt-in gate", () => {
  it("default constructor leaves sqlite unregistered", () => {
    const bash = new Bash();
    const r = bash.executeSync("sqlite :memory: 'SELECT 1'");
    assert.match(r.stderr, /command not found/);
  });

  it("sqlite: true registers the builtin and sets the env opt-in", () => {
    const bash = new Bash({ sqlite: true });
    const r = bash.executeSync("sqlite :memory: 'SELECT 1 + 2'");
    assert.equal(r.exitCode, 0, `stderr was: ${r.stderr}`);
    assert.equal(r.stdout.trim(), "3");
  });
});

describe("sqlite basic queries", () => {
  const bash = new Bash({ sqlite: true });

  it("CRUD round-trip", () => {
    const r = bash.executeSync(
      'sqlite :memory: \'CREATE TABLE t(a INTEGER, b TEXT); INSERT INTO t VALUES (1, "x"), (2, "y"); SELECT * FROM t ORDER BY a\'',
    );
    assert.equal(r.exitCode, 0, `stderr was: ${r.stderr}`);
    assert.equal(r.stdout, "1|x\n2|y\n");
  });

  it("-header flag emits a single header row", () => {
    const r = bash.executeSync(
      "sqlite -header :memory: 'CREATE TABLE t(x, y); INSERT INTO t VALUES (1, 2); SELECT * FROM t'",
    );
    assert.equal(r.exitCode, 0, `stderr was: ${r.stderr}`);
    assert.equal(r.stdout, "x|y\n1|2\n");
  });

  it("-json mode round-trips through JSON.parse", () => {
    const r = bash.executeSync(
      "sqlite -json :memory: 'SELECT 1 AS i, \"hi\" AS s'",
    );
    assert.equal(r.exitCode, 0, `stderr was: ${r.stderr}`);
    const parsed = JSON.parse(r.stdout.trim());
    assert.equal(parsed[0].i, 1);
    assert.equal(parsed[0].s, "hi");
  });
});

describe("sqlite VFS persistence", () => {
  it("database file persists across separate sqlite invocations", () => {
    const bash = new Bash({ sqlite: true });
    const seed = bash.executeSync(
      'sqlite /tmp/notes.sqlite \'CREATE TABLE notes(body TEXT); INSERT INTO notes VALUES ("hello")\'',
    );
    assert.equal(seed.exitCode, 0, `stderr was: ${seed.stderr}`);

    const read = bash.executeSync(
      "sqlite -header /tmp/notes.sqlite 'SELECT * FROM notes'",
    );
    assert.equal(read.exitCode, 0, `stderr was: ${read.stderr}`);
    assert.match(read.stdout, /hello/);
  });
});

describe("sqlite security policy surfaces through the binding", () => {
  const bash = new Bash({ sqlite: true });

  it("ATTACH is rejected by policy", () => {
    const r = bash.executeSync(
      `sqlite :memory: "ATTACH DATABASE '/tmp/other.db' AS other"`,
    );
    assert.notEqual(r.exitCode, 0);
    assert.match(r.stderr, /ATTACH\/DETACH is not supported/);
  });

  it("PRAGMA cache_size is rejected by the default deny list", () => {
    const r = bash.executeSync("sqlite :memory: 'PRAGMA cache_size = -1000'");
    assert.notEqual(r.exitCode, 0);
    assert.match(r.stderr, /PRAGMA cache_size is denied/);
  });

  it("PRAGMA user_version still passes (operational PRAGMAs not denied)", () => {
    const r = bash.executeSync(
      "sqlite :memory: 'PRAGMA user_version=42; PRAGMA user_version'",
    );
    assert.equal(r.exitCode, 0, `stderr was: ${r.stderr}`);
    assert.match(r.stdout, /42/);
  });
});
