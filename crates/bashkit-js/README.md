# @everruns/bashkit

Sandboxed bash interpreter for JavaScript/TypeScript. Native NAPI-RS bindings to the [bashkit](https://github.com/everruns/bashkit) Rust core. Works with Node.js, Bun, and Deno.

## Install

```bash
npm install @everruns/bashkit   # Node.js
bun add @everruns/bashkit       # Bun
deno add npm:@everruns/bashkit  # Deno
```

## Usage

```typescript
import { Bash, BashTool, getVersion } from '@everruns/bashkit';

// Basic usage
const bash = new Bash();
const result = bash.executeSync('echo "Hello, World!"');
console.log(result.stdout); // Hello, World!\n

// State persists between calls
bash.executeSync('X=42');
bash.executeSync('echo $X'); // stdout: 42\n

// With tool-contract metadata
const tool = new BashTool();
console.log(tool.name);           // "bashkit"
console.log(tool.inputSchema());  // JSON schema for LLM tool-use
console.log(tool.description());  // Token-efficient tool description
console.log(tool.help());         // Markdown help document
console.log(tool.systemPrompt()); // Compact system prompt

const r = tool.executeSync('echo hello');
console.log(r.stdout); // hello\n
```

## API

### `Bash`

Core interpreter with virtual filesystem.

- `new Bash(options?)` — create instance
- `executeSync(commands)` — run bash commands, returns `ExecResult`
- `executeSyncOrThrow(commands)` — run bash commands, throws `BashError` on non-zero exit
- `reset()` — clear state, preserve config

### `BashTool`

Interpreter + tool-contract metadata.

- All `Bash` methods, plus:
- `name` — tool name (`"bashkit"`)
- `version` — version string
- `shortDescription` — one-liner
- `description()` — token-efficient tool description
- `help()` — Markdown help document
- `systemPrompt()` — compact system prompt for LLM orchestration
- `inputSchema()` — JSON input schema
- `outputSchema()` — JSON output schema

### `BashOptions`

```typescript
interface BashOptions {
  username?: string;
  hostname?: string;
  maxCommands?: number;
  maxLoopIterations?: number;
}
```

### `ExecResult`

```typescript
interface ExecResult {
  stdout: string;
  stderr: string;
  exit_code: number;
  error?: string;
}
```

## Platform Support

| OS | Architecture |
|----|-------------|
| macOS | x86_64, aarch64 (Apple Silicon) |
| Linux | x86_64, aarch64 |
| Windows | x86_64 |
| WASM | wasm32-wasip1-threads |

## License

MIT
