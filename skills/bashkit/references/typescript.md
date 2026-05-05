# JavaScript and TypeScript API

Use this when the user wants Bashkit in Node.js, Bun, Deno, browser/WASM, or JS/TS agent frameworks.

## Install

```bash
npm install @everruns/bashkit
bun add @everruns/bashkit
deno add npm:@everruns/bashkit
```

## Sync Execution

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();

const result = bash.executeSync('echo "Hello, World!"');
console.log(result.stdout);

bash.executeSync("X=42");
console.log(bash.executeSync("echo $X").stdout);
```

## Async Execution

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();

const result = await bash.execute('printf "banana\\napple\\ncherry\\n" | sort');
console.log(result.stdout);

await bash.execute('printf "data\\n" > /tmp/file.txt');
console.log((await bash.execute("cat /tmp/file.txt")).stdout);
```

## Configuration

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash({
  username: "agent",
  hostname: "sandbox",
  maxCommands: 1000,
  maxLoopIterations: 10000,
  timeoutMs: 30_000,
  mounts: [{ path: "/workspace", root: "./src", writable: true }],
  python: false,
});
```

## Virtual Filesystem

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();

bash.mkdir("/data", true);
bash.writeFile("/data/config.json", '{"debug":true}');

console.log(bash.readFile("/data/config.json"));
console.log(bash.executeSync("cat /data/config.json").stdout);
```

## Live Output

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();

await bash.execute("for i in 1 2 3; do echo out-$i; echo err-$i >&2; done", {
  onOutput({ stdout, stderr }) {
    if (stdout) process.stdout.write(stdout);
    if (stderr) process.stderr.write(stderr);
  },
});
```

## Reference

- npm: https://www.npmjs.com/package/@everruns/bashkit
- JS/TS package docs: https://github.com/everruns/bashkit/tree/main/crates/bashkit-js
- Examples: https://github.com/everruns/bashkit/tree/main/examples
