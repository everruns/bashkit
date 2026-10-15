// Manual browser experiment. CI gates deterministic bytes/semantics, not noisy wall time.
// Supply PLAYWRIGHT_MODULE when Playwright is installed outside this workspace.
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { brotliCompressSync, constants } from "node:zlib";
import { artifact } from "./artifact.mjs";

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? "playwright");
const packages = process.argv.slice(2).map(resolvePath => resolve(resolvePath));
if (!packages.length) throw new Error("pass package directories to compare");
const trials = Number(process.env.TRIALS ?? 5);
if (!Number.isInteger(trials) || trials < 1 || trials > 20) throw new Error("TRIALS must be 1..20");
const conditions = {
  local: null,
  fast4g: { downloadThroughput: 1012500, uploadThroughput: 168750, latency: 165 },
  slow4g: { downloadThroughput: 180000, uploadThroughput: 84375, latency: 562.5 },
};
const routes = new Map();
packages.forEach((pkg, i) => {
  for (const file of ["index.js", "bashkit_wasm.js", "bashkit_wasm_bg.wasm"]) {
    routes.set(`/${i}/${file}`, { type: file.endsWith("wasm") ? "application/wasm" : "text/javascript",
      bytes: brotliCompressSync(readFileSync(resolve(pkg, file)), { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }) });
  }
  const html = `<pre id="screen"></pre><script type="module">
    const marks = { start: performance.now() };
    const { initBashkit, Bash, Terminal } = await import('/${i}/index.js');
    marks.imported = performance.now();
    await initBashkit(); marks.initialized = performance.now();
    const bash = new Bash();
    const r = await bash.execute('echo hi');
    if (r.stdout !== 'hi\\n' || r.exitCode !== 0) throw new Error('first command failed');
    marks.command = performance.now();
    if (Terminal) {
      const terminal = new Terminal(); await terminal.runUntilIdle();
      terminal.send('echo hi\\r'); await terminal.runUntilIdle();
      document.querySelector('#screen').textContent = terminal.screenText();
      if (!terminal.screenText().includes('hi\\n$')) throw new Error('terminal failed');
    } else document.querySelector('#screen').textContent = r.stdout + '$';
    await new Promise(requestAnimationFrame); await new Promise(requestAnimationFrame);
    marks.prompt = performance.now();
    window.result = { marks, resources: performance.getEntriesByType('resource').map(r => ({ name: r.name.split('/').at(-1), start: r.startTime, end: r.responseEnd, transfer: r.transferSize, encoded: r.encodedBodySize })) };
  </script>`;
  routes.set(`/${i}/`, { type: "text/html", bytes: Buffer.from(html), plain: true });
});
const server = createServer((req, res) => {
  const route = routes.get(req.url);
  if (!route) { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { "Content-Type": route.type, "Content-Length": route.bytes.length,
    "Cache-Control": "public, max-age=31536000, immutable", "Vary": "Accept-Encoding",
    ...(route.plain ? {} : { "Content-Encoding": "br" }) });
  res.end(route.bytes);
});
await new Promise(r => server.listen(0, "127.0.0.1", r));
let browser;
try {
  browser = await chromium.launch({ channel: process.env.CHROME_CHANNEL ?? "chrome", headless: true });
  const samples = [];
  for (const [condition, throttle] of Object.entries(conditions)) {
    for (let trial = 0; trial < trials; trial++) {
      const order = packages.map((_, i) => (i + trial) % packages.length);
      if (trial % 2) order.reverse();
      for (const i of order) {
        const context = await browser.newContext(); // Cold HTTP cache per sample.
        try {
          const page = await context.newPage();
          const session = await context.newCDPSession(page);
          await session.send("Network.enable");
          if (throttle) await session.send("Network.emulateNetworkConditions", { offline: false, ...throttle });
          await session.send("Emulation.setCPUThrottlingRate", { rate: 4 });
          const errors = []; page.on("pageerror", e => errors.push(String(e)));
          await page.goto(`http://127.0.0.1:${server.address().port}/${i}/`, { waitUntil: "domcontentloaded", timeout: 60000 });
          await page.waitForFunction(() => window.result, null, { timeout: 60000 });
          const result = await page.evaluate(() => window.result);
          if (errors.length) throw new Error(errors.join("\n"));
          samples.push({ package: i, condition, trial, ...result });
          console.error(`${condition} ${trial + 1}/${trials} package ${i}: ${result.marks.prompt.toFixed(1)}ms`);
        } finally { await context.close(); }
      }
    }
  }
  console.log(JSON.stringify({ method: "Cold context, Brotli 11, CDP Fast/Slow 4G, page CPU 4x; main-thread JS-host WASM, no terminal renderer dependency; prompt after first echo and two animation frames", browser: browser.version(), node: process.version,
    packages: packages.map(pkg => ({ path: pkg, ...artifact(pkg) })), samples }, null, 2));
} finally { await browser?.close(); await new Promise(r => server.close(r)); }
