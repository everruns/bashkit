// Terminal (xterm.js-style) session tests. The bundle exposes `Terminal` only
// when built with the `terminal` cargo feature, which scripts/build.sh enables
// by default. With BASHKIT_WASM_FEATURES="" `Terminal` is undefined and these
// tests skip.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { initBashkit, Terminal } from "../pkg/index.js";

await initBashkit(
  readFileSync(fileURLToPath(new URL("../pkg/bashkit_wasm_bg.wasm", import.meta.url))),
);

const skip = Terminal === undefined && "built without the terminal feature";
const decoder = new TextDecoder();

test("terminal: prompt, echo and output bytes", { skip }, async () => {
  const term = new Terminal({ rows: 10, cols: 40 });
  assert.deepEqual(await term.runUntilIdle(), { status: "idle" });
  term.send("echo hi | tr a-z A-Z\r");
  assert.deepEqual(await term.runUntilIdle(), { status: "idle" });
  assert.equal(term.screenText(), "$ echo hi | tr a-z A-Z\nHI\n$");
  const out = decoder.decode(term.takeOutput());
  assert.match(out, /HI\r\n/);
  assert.equal(term.takeOutput().length, 0, "output is drained");
});

test("terminal: vi edits a VFS file", { skip }, async () => {
  const term = new Terminal();
  term.send("vi /tmp/n.txt\r");
  await term.runUntilIdle();
  assert.equal(term.isAlternateScreen(), true);
  term.send("ihello\x1b:wq\r");
  await term.runUntilIdle();
  assert.equal(term.isAlternateScreen(), false);
  assert.equal(term.fs().readFile("/tmp/n.txt"), "hello\n");
});

test("terminal: options, resize and exit", { skip }, async () => {
  const term = new Terminal({ cols: 50, files: { "/data/a.txt": "seeded\n" } });
  term.resize(12, 60);
  assert.equal(term.rows, 12);
  assert.equal(term.cols, 60);
  term.send("cat /data/a.txt; echo $COLUMNS\r");
  await term.runUntilIdle();
  assert.match(term.screenText(), /seeded\n60/);
  term.send("exit 4\r");
  assert.deepEqual(await term.runUntilIdle(), { status: "exited", exitCode: 4 });
  assert.equal(term.exitCode, 4);
});

test("terminal: keystrokes sent mid-command are queued", { skip }, async () => {
  const term = new Terminal();
  await term.runUntilIdle();
  term.send("sleep 0.05; echo done\r");
  const running = term.runUntilIdle();
  term.send("echo next\r");
  await running;
  await term.runUntilIdle();
  assert.match(term.screenText(), /done\n\$ echo next\nnext\n\$$/);
});
