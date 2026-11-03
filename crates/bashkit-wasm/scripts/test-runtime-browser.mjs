// Real Chromium + module workers, deliberately served without isolation headers.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFileSync, existsSync } from "node:fs";
const { chromium } = await import(
  process.env.PLAYWRIGHT_MODULE ?? "playwright"
);
const base = new URL("../pkg/runtimes/", import.meta.url);
const files = new Map(
  [
    "index.js",
    "runtime-functions.js",
    "bashkit_wasm.js",
    "bashkit_wasm_bg.wasm",
  ].map((name) => [`/${name}`, readFileSync(new URL(name, base))]),
);
const worker = `import { Bash, initBashkit } from '/index.js';
await initBashkit();
const bash = new Bash({typescript:true,typescriptExternalFunctions:['lookup'],typescriptExternalHandler:async()=>42});
postMessage((await bash.execute("ts -c 'await lookup()'", {capture:64, stdout:async()=>{await Promise.resolve();}})).stdout);`;
const server = createServer((req, res) => {
  if (req.url === "/")
    return res
      .writeHead(200, { "Content-Type": "text/html" })
      .end("<!doctype html><title>Runtime test</title>");
  if (req.url === "/worker.mjs")
    return res
      .writeHead(200, { "Content-Type": "text/javascript" })
      .end(worker);
  if (req.url === "/service")
    return res
      .writeHead(200, { "Content-Type": "application/json" })
      .end('{"value":42}');
  const file = files.get(req.url);
  if (!file) return res.writeHead(404).end();
  res
    .writeHead(200, {
      "Content-Type": req.url.endsWith(".wasm")
        ? "application/wasm"
        : "text/javascript",
    })
    .end(file);
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const executablePath =
  process.env.CHROME_EXECUTABLE ??
  process.env.BASHKIT_CHROMIUM ??
  ["/usr/bin/chromium", "/usr/bin/google-chrome"].find(existsSync);
let browser;
try {
  browser = await chromium.launch({ executablePath, headless: true });
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const result = await page.evaluate(async () => {
    const { Bash, initBashkit } = await import("/index.js");
    await initBashkit();
    const lookup = async (_name, _args, _kwargs, { signal }) =>
      (await (await fetch("/service", { signal })).json()).value;
    const bash = new Bash({
      python: true,
      typescript: true,
      externalFunctions: ["lookup"],
      externalHandler: lookup,
      typescriptExternalFunctions: ["lookup"],
      typescriptExternalHandler: (name, args, context) =>
        lookup(name, args, {}, context),
    });
    const outputs = [],
      streamed = [];
    for (const script of [
      "python -c 'print(lookup())'",
      "ts -c 'await lookup()'",
    ]) {
      const result = await bash.execute(script, {
        capture: 64,
        stdout: async (bytes) => {
          await Promise.resolve();
          streamed.push(new TextDecoder().decode(bytes));
        },
      });
      outputs.push(result.stdout);
    }
    const workers = await Promise.all(
      Array.from(
        { length: 3 },
        () =>
          new Promise((resolve, reject) => {
            const worker = new Worker("/worker.mjs", { type: "module" });
            worker.onmessage = (event) => {
              worker.terminate();
              resolve(event.data);
            };
            worker.onerror = (event) => {
              worker.terminate();
              reject(new Error(event.message));
            };
          }),
      ),
    );
    return { isolated: crossOriginIsolated, outputs, streamed, workers };
  });
  assert.equal(result.isolated, false);
  assert.deepEqual(result.outputs, ["42\n", "42\n"]);
  assert.deepEqual(result.streamed, ["42\n", "42\n"]);
  assert.deepEqual(result.workers, ["42\n", "42\n", "42\n"]);
  assert.deepEqual(errors, []);
  console.log(
    "Chromium: Python/TypeScript callbacks, fetch and module workers pass without COOP/COEP",
  );
} finally {
  await browser?.close();
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
}
