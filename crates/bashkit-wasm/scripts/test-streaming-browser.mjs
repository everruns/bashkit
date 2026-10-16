// Optional real-browser gate. Requires Playwright and Chromium; no COOP/COEP.
// PLAYWRIGHT_MODULE and CHROME_EXECUTABLE can select installed dependencies.
import {createServer} from "node:http";
import {readFile} from "node:fs/promises";
import {resolve, extname} from "node:path";
import {fileURLToPath} from "node:url";
import {createRequire} from "node:module";
import assert from "node:assert/strict";
const {chromium} = process.env.PLAYWRIGHT_MODULE
  ? await import(process.env.PLAYWRIGHT_MODULE)
  : createRequire(import.meta.url)("playwright");
const root = fileURLToPath(new URL("../pkg/", import.meta.url));
const server = createServer(async (request, response) => {
  if (request.url === "/") {response.end("<!doctype html><title>Streaming test</title>"); return;}
  const path = resolve(root, `.${request.url.split("?")[0]}`);
  if (!path.startsWith(root)) {response.writeHead(403).end(); return;}
  try {
    response.setHeader("Content-Type", extname(path) === ".wasm" ? "application/wasm" : "text/javascript");
    response.end(await readFile(path));
  } catch {response.writeHead(404).end();}
});
await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
let browser;
try {
  browser = await chromium.launch({executablePath: process.env.CHROME_EXECUTABLE ?? "/usr/bin/chromium", headless: true, args: ["--no-sandbox"]});
  const page = await browser.newPage();
  page.setDefaultTimeout(15000);
  await page.goto(origin);
  // The exact same scenario runs on the main thread and a module worker.
  const scenario = async (origin) => {
    const {Bash, initBashkit} = await import(`${origin}/index.js`);
    await initBashkit();
    let release;
    const gate = new Promise(resolve => {release = resolve;});
    const encoder = new TextEncoder();
    const shell = new Bash({commands: {
      async produce(ctx) {
        await ctx.stdout.write(encoder.encode("error: first\n"));
        await gate;
        await ctx.stdout.write(encoder.encode("error: last\n"));
        return 0;
      },
      async copy(ctx) {for await (const bytes of ctx.stdin) await ctx.stdout.write(bytes); return 0;},
      async binary(ctx) {await ctx.stdout.write(Uint8Array.of(0, 255, 97)); return 0;},
    }});
    const chunks = [];
    let completed = false;
    let deliveredBeforeCompletion = false;
    const result = await shell.execute("produce | copy | grep error", {
      capture: 64, async stdout(bytes) {
        deliveredBeforeCompletion ||= !completed;
        chunks.push(...bytes);
        release();
        await new Promise(resolve => setTimeout(resolve, 2));
      },
    });
    completed = true;
    const binary = await shell.execute("binary", {capture: 3});
    const controller = new AbortController();
    let cancelled = false;
    try {
      await shell.execute("echo pending", {signal: controller.signal, async stdout() {
        controller.abort(); await new Promise(() => {});
      }});
    } catch (error) {cancelled = String(error).includes("cancelled");}
    const recovered = await shell.execute("echo recovered", {capture: 64});
    return {deliveredBeforeCompletion, stdout: result.stdout, bytes: [...binary.stdoutBytes], cancelled, recovered: recovered.stdout, chunkText: new TextDecoder().decode(new Uint8Array(chunks))};
  };
  const expected = {deliveredBeforeCompletion: true, stdout: "error: first\nerror: last\n", bytes: [0, 255, 97], cancelled: true, recovered: "recovered\n", chunkText: "error: first\nerror: last\n"};
  const main = await page.evaluate(`Promise.race([(${scenario})(${JSON.stringify(origin)}), new Promise((_, reject) => setTimeout(() => reject(new Error("main thread timed out")), 15000))])`);
  assert.deepEqual(main, expected);
  const worker = await page.evaluate(async ({source, origin}) => {
    const script = `(${source})(${JSON.stringify(origin)}).then(result => postMessage({result}), error => postMessage({error: String(error)}));`;
    const url = URL.createObjectURL(new Blob([script], {type: "text/javascript"}));
    const worker = new Worker(url, {type: "module"});
    try {
      return await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("worker timed out")), 15000);
        worker.onmessage = event => {clearTimeout(timer); event.data.error ? reject(new Error(event.data.error)) : resolve(event.data.result);};
        worker.onerror = event => {clearTimeout(timer); reject(new Error(event.message));};
      });
    } finally {worker.terminate(); URL.revokeObjectURL(url);}
  }, {source: String(scenario), origin});
  assert.deepEqual(worker, expected);
  console.log("Chromium main thread + worker: live delivery, bytes, async sink, cancellation and reuse passed");
} finally {
  await browser?.close();
  await new Promise(resolve => server.close(resolve));
}
