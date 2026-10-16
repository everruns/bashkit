// Real single-threaded WASM package: no native addon or shared-memory fallback.
import {test} from "node:test";
import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {Bash, initBashkit} from "../pkg/index.js";
await initBashkit(readFileSync(new URL("../pkg/bashkit_wasm_bg.wasm", import.meta.url)));

test("live final filter, bounded capture and request-scoped binary I/O", {timeout: 5000}, async () => {
  let release;
  const gate = new Promise((resolve) => {release = resolve;});
  const encoder = new TextEncoder();
  let saved;
  const shell = new Bash({commands: {
    async produce(ctx) {
      saved = ctx;
      await ctx.stdout.write(encoder.encode("error: first\n"));
      await gate;
      await ctx.stdout.write(encoder.encode("error: last\n"));
      return 0;
    },
    async copy(ctx) {
      for await (const bytes of ctx.stdin) await ctx.stdout.write(bytes);
      return {exitCode: 0};
    },
    async binary(ctx) {await ctx.stdout.write(Uint8Array.of(0, 255, 97, 98)); return 7;},
  }});
  const chunks = [];
  const result = await shell.execute("produce | copy | grep error", {
    capture: 64, async stdout(bytes) {chunks.push(bytes); release(); await new Promise(r => setTimeout(r, 1));},
  });
  assert.equal(result.stdout, "error: first\nerror: last\n");
  assert.equal(Buffer.concat(chunks).toString(), result.stdout);
  await assert.rejects(saved.stdout.write(Uint8Array.of(1)), /request closed/);
  await assert.rejects(saved.stdin.readChunk(), /request closed/);
  const binary = await shell.execute("binary", {capture: 3});
  assert.deepEqual([...binary.stdoutBytes], [0, 255, 97]);
  assert.equal(binary.stdoutTruncated, true);
  assert.equal(binary.exitCode, 7);
});

test("sink-only execution, sink failure, cancellation and recovery", {timeout: 5000}, async () => {
  const shell = new Bash();
  const chunks = [];
  const result = await shell.execute("echo hello", {stdout(bytes) {chunks.push(bytes);}});
  assert.equal(result.stdout, "");
  assert.equal(Buffer.concat(chunks).toString(), "hello\n");
  await assert.rejects(shell.execute("echo hello", {stdout() {throw new Error("sink failed");}}), /sink failed/);
  const controller = new AbortController();
  await assert.rejects(shell.execute("echo hello", {
    signal: controller.signal,
    async stdout() {controller.abort(); await new Promise(() => {});},
  }), /cancelled/);
  assert.equal((await shell.execute("echo recovered", {capture: 64})).stdout, "recovered\n");
});


test("signal-only options interrupt pending commands and preserve ordinary capture", {timeout: 5000}, async () => {
  const controller = new AbortController();
  const shell = new Bash({commands: {async pending() {
    controller.abort();
    await new Promise(() => {});
    return 0;
  }}});
  await assert.rejects(shell.execute("pending", {signal: controller.signal}), /cancelled/);
  assert.equal((await shell.execute("echo captured", {signal: new AbortController().signal})).stdout, "captured\n");
});
