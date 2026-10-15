// Fresh instances, exact bytes/status, bounded worker lifetime. GNU Bash is the oracle.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { execFileSync, spawnSync } from "node:child_process";
import { Worker, isMainThread, parentPort, workerData } from "node:worker_threads";

if (!isMainThread) {
  const { Bash, initBashkit } = await import(pathToFileURL(resolve(workerData.pkg, "index.js")));
  await initBashkit(readFileSync(resolve(workerData.pkg, "bashkit_wasm_bg.wasm")));
  for (const item of workerData.cases) {
    const bash = new Bash({ profile: "interactive", maxCommands: 100000,
      maxLoopIterations: 10000, maxMemory: 8 * 1024 * 1024, timeoutMs: 1000 });
    try {
      const result = await bash.execute(item.script);
      parentPort.postMessage({ name: item.name, stdout: Buffer.from(result.stdoutBytes).toString("base64"), status: result.exitCode });
      result.free();
    } catch (error) {
      parentPort.postMessage({ name: item.name, error: String(error) });
    } finally { bash.free(); }
  }
} else {
  const cases = JSON.parse(readFileSync(new URL("./compat-cases.json", import.meta.url)));
  const oracle = process.env.BASH ?? "bash";
  const version = execFileSync(oracle, ["--version"], { encoding: "utf8" }).split("\n")[0];
  if (!/version ([5-9]|[1-9][0-9])\./.test(version)) throw new Error("GNU Bash 5+ required");
  const expected = cases.map(item => {
    const r = spawnSync(oracle, ["--noprofile", "--norc", "-c", item.script], { timeout: 2000, maxBuffer: 65536 });
    if (r.error || r.signal) throw new Error(`oracle failed: ${item.name}`);
    return { stdout: r.stdout.toString("base64"), status: r.status };
  });
  const packages = process.argv.slice(2);
  if (!packages.length) packages.push(new URL("../pkg/", import.meta.url).pathname);
  const reports = [];
  for (const pkg of packages) {
    const actual = await new Promise((resolveResults, reject) => {
      const results = [];
      const worker = new Worker(new URL(import.meta.url), { workerData: { pkg: resolve(pkg), cases } });
      const timer = setTimeout(() => { worker.terminate(); reject(new Error("compatibility worker exceeded 30s")); }, 30000);
      worker.on("message", item => results.push(item));
      worker.on("error", reject);
      worker.on("exit", code => { clearTimeout(timer); code ? reject(new Error(`worker exited ${code}`)) : resolveResults(results); });
    });
    if (actual.length !== cases.length) throw new Error("incomplete results");
    const results = actual.map((result, i) => {
      const pass = result.stdout === expected[i].stdout && result.status === expected[i].status;
      return { name: result.name, pass, ...(!pass ? { expected: expected[i], actual: result } : {}) };
    });
    reports.push({ package: resolve(pkg), passed: results.filter(r => r.pass).length, total: cases.length, results });
  }
  console.log(JSON.stringify({ oracle: version, reports }, null, 2));
  // Single-package mode is the CI gate; comparisons record old releases' failures.
  if (packages.length === 1 && reports[0].passed !== cases.length) process.exitCode = 1;
}
