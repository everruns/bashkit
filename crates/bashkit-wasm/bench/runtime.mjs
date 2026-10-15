// Representative commands guard against buying transfer savings with unusable execution.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
const pkg = resolve(process.argv[2]);
const { initBashkit, Bash } = await import(pathToFileURL(resolve(pkg, "index.js")));
await initBashkit(readFileSync(resolve(pkg, "bashkit_wasm_bg.wasm")));
const cases = {
  arithmetic: "n=0; for ((i=0;i<20000;i++)); do ((n+=i)); done; echo $n",
  substitutions: 'for ((i=0;i<1000;i++)); do a=$(echo hi); done; echo "$a"',
  jq: "seq 1000 | jq -s 'map(. * 2) | add'",
  text: "seq 10000 | sed 's/1/x/g' | sort | wc -l",
};
const results = {};
for (const [name, script] of Object.entries(cases)) {
  const samples = [];
  let output;
  for (let i = 0; i < 8; i++) {
    const bash = new Bash({ profile: "interactive", maxCommands: 1000000, maxLoopIterations: 100000 });
    const start = performance.now();
    const result = await bash.execute(script);
    const ms = performance.now() - start;
    if (result.exitCode !== 0) throw new Error(`${name}: ${result.stderr}`);
    if (output !== undefined && output !== result.stdout) throw new Error("unstable output");
    output = result.stdout;
    if (i) samples.push(ms);
    result.free(); bash.free();
  }
  results[name] = { stdout: output, samples, median: [...samples].sort((a, b) => a - b)[3] };
}
console.log(JSON.stringify({ node: process.version, package: pkg, results }, null, 2));
