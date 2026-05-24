# Custom Builtins in Bashkit

Register your own commands as bash builtins. They behave like baked-in
commands: invoke them by name, pipe data through them, redirect their output
into the virtual filesystem. They share the interpreter's VFS and shell
state, so a builtin's output written to `/scratch/out.json` is still there in
the next `execute()` call.

This page covers the Node bindings (`@everruns/bashkit`). The Rust core
exposes the same capability via `BashBuilder::builtin_registry`; see the
[bashkit rustdoc guide](https://docs.rs/bashkit/latest/bashkit/custom_builtins_guide/)
for the Rust API and `crates/bashkit/tests/builtin_registry_tests.rs` for
worked examples.

## Quick start

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash({
  customBuiltins: {
    greet: (ctx) => `hello ${ctx.argv[0] ?? "world"}\n`,
    "get-order": async (ctx) => {
      const order = await fetchOrder(ctx.argv[0]);
      return JSON.stringify(order) + "\n";
    },
  },
});

await bash.execute("mkdir -p /scratch");
await bash.execute("get-order 42 > /scratch/order.json");
console.log((await bash.execute("cat /scratch/order.json")).stdout);
```

Two ways to register:

| API | When | Notes |
|-----|------|-------|
| `new Bash({ customBuiltins: {...} })` | At construction | Convenient for a fixed set of builtins. |
| `bash.addBuiltin(name, callback)` | Any time after | Safe to call after `execute()` has accumulated state — the interpreter is **not** rebuilt and the VFS stays intact. |
| `bash.removeBuiltin(name)` | Any time after | Subsequent invocations fall through to baked-in builtins / `$PATH`. |

Same API on `BashTool`.

## The callback contract

A callback receives one argument — a `BuiltinContext` snapshot of shell
state at invocation time — and returns the stdout to emit:

```typescript
import type { BuiltinContext, BuiltinCallback } from "@everruns/bashkit";

interface BuiltinContext {
  readonly name: string;                       // command name as invoked
  readonly argv: string[];                     // args, not including the name
  readonly stdin: string | null;               // piped input, null if no pipe
  readonly env: Record<string, string>;        // exported env vars
  readonly cwd: string;                        // current working directory
}

type BuiltinCallback = (ctx: BuiltinContext) => string | Promise<string>;
```

Sync (`string`) and async (`Promise<string>`) returns are both supported.
Internally, every return is wrapped with `Promise.resolve(...)` so the Rust
adapter handles them uniformly.

The return value is treated as stdout. To emit a specific exit code or
stderr, throw — exceptions become stderr with exit code 1, like a real
failing command:

```typescript
const bash = new Bash({
  customBuiltins: {
    fail: () => {
      throw new Error("nope");          // → stderr: "GenericFailure, Error: nope", exit 1
    },
    "async-fail": async () => {
      throw new Error("async nope");    // same, with the async-rejection text
    },
  },
});
```

## Sync vs async — and why you can't use `executeSync()`

Custom builtins are dispatched over NAPI's threadsafe-function bridge, which
schedules callbacks on the JS event loop. That means **the JS event loop
must be free to dispatch them**.

`bash.executeSync()` blocks the JS event loop synchronously while the
interpreter runs. If the script invokes a custom builtin, the dispatch
never gets a chance to fire — the call deadlocks.

> **Always use `await bash.execute(...)`** when custom builtins are
> registered. This matches `ScriptedTool`'s constraint.

A runtime guardrail to fail fast instead of deadlocking is tracked in
[#1725](https://github.com/everruns/bashkit/issues/1725).

## Persistent VFS

The interpreter's virtual filesystem persists across `execute()` calls,
*including* the calls inside which a custom builtin wrote files. This is
the main difference from `ScriptedTool`, where each script gets a fresh
interpreter:

```typescript
const bash = new Bash({
  customBuiltins: {
    log: (ctx) => `${new Date().toISOString()} ${ctx.argv.join(" ")}\n`,
  },
});

// Each call appends to the same virtual file.
await bash.execute("log started >> /var/log/app.log");
await bash.execute("log processed 42 >> /var/log/app.log");
await bash.execute("log done >> /var/log/app.log");

console.log((await bash.execute("cat /var/log/app.log")).stdout);
// 2026-05-24T... started
// 2026-05-24T... processed 42
// 2026-05-24T... done
```

## Override precedence

Command resolution order in the interpreter:

1. Shell functions defined in the script
2. POSIX special builtins (`exec`, `set`, `:`, `eval`, …)
3. **Custom builtins** (`customBuiltins` + `addBuiltin`)
4. Baked-in builtins (`cat`, `ls`, `grep`, …)
5. Scripts on `$PATH`

So custom builtins **can** override baked-in commands (e.g. wrap `cat` with
tracing), but a shell function defined in the script still wins:

```typescript
const bash = new Bash({
  customBuiltins: {
    thing: () => "from-builtin\n",
  },
});

// Custom builtin wins over the baked-in (no baked-in `thing` anyway)
console.log((await bash.execute("thing")).stdout);          // from-builtin

// Shell function wins over the custom builtin
const r = await bash.execute(
  "thing() { printf 'from-function\\n'; }\nthing",
);
console.log(r.stdout);                                       // from-function
```

`command -v thing` and `command -V thing` report custom builtins as builtins.

## Lifecycle

| Operation | Custom builtins |
|-----------|----------------|
| `bash.reset()` | **Preserved.** The registry is host-side; only interpreter shell state and VFS are reset. |
| `bash.snapshot()` / `restoreSnapshot()` | **Not preserved.** Snapshots contain interpreter state only. Re-pass `customBuiltins` (or call `addBuiltin`) after restoring. |
| `Bash.fromSnapshot(data, options)` | Same as `restoreSnapshot`: pass `customBuiltins` in the `options`. |
| `bash.addBuiltin` / `removeBuiltin` | Take effect immediately for the next `execute()`. No interpreter rebuild. |

## `BashTool`

`BashTool` has the same API — useful when exposing a sandboxed shell to an
LLM as a tool. Custom builtins augment the tool's command surface:

```typescript
import { BashTool } from "@everruns/bashkit";

const tool = new BashTool({
  customBuiltins: {
    "get-weather": async (ctx) => {
      const city = ctx.argv[0] ?? "unknown";
      return JSON.stringify({ city, temp: 72, sky: "clear" }) + "\n";
    },
  },
});

const r = await tool.execute(
  "get-weather 'San Francisco' | jq -r '.temp'",
);
// 72
```

## Common patterns

### Wrap a host API

Expose a callable that pulls from your backend, leaves the result in the
VFS, and lets the LLM (or downstream shell logic) process it with normal
shell tools:

```typescript
const bash = new Bash({
  customBuiltins: {
    "search-tickets": async (ctx) => {
      const tickets = await db.tickets.search(ctx.argv[0]);
      return tickets.map((t) => `${t.id}\t${t.title}`).join("\n") + "\n";
    },
  },
});

await bash.execute(
  "search-tickets 'auth bug' > /tmp/results.tsv && wc -l < /tmp/results.tsv",
);
```

### Stage-based pipelines

Custom builtins can read piped stdin and emit transformed output — chain
them like any other bash command:

```typescript
const bash = new Bash({
  customBuiltins: {
    parse: (ctx) => JSON.parse(ctx.stdin ?? "{}").value ?? "",
    sign: async (ctx) => signWithKms(ctx.stdin?.trim() ?? ""),
  },
});

await bash.execute("cat /in/req.json | parse | sign > /out/signed.txt");
```

### Override for tracing or recording

Wrap a baked-in builtin to log every invocation while preserving original
behavior (call into the bashkit interpreter via the parent if you need the
original result — for full override+passthrough see the Rust API):

```typescript
const calls: string[] = [];
const bash = new Bash({
  customBuiltins: {
    cat: (ctx) => {
      calls.push(`cat ${ctx.argv.join(" ")}`);
      // Read files directly via bash.fs() and return contents
      return ctx.argv
        .map((p) => bash.readFile(p))
        .join("");
    },
  },
});
```

## See also

- Example script: [`examples/custom_builtins.mjs`](https://github.com/everruns/bashkit/blob/main/examples/custom_builtins.mjs) — runnable, asserts at every step, exercised in CI.
- API reference: [`@everruns/bashkit` README](https://github.com/everruns/bashkit/blob/main/crates/bashkit-js/README.md) — option/method signatures.
- Rust core: [`bashkit::BuiltinRegistry`](https://docs.rs/bashkit/latest/bashkit/struct.BuiltinRegistry.html), [`BashBuilder::builtin_registry`](https://docs.rs/bashkit/latest/bashkit/struct.BashBuilder.html#method.builtin_registry).
- Design rationale: PR [#1721](https://github.com/everruns/bashkit/pull/1721).
- Python parity: tracked in [#1724](https://github.com/everruns/bashkit/issues/1724).
- `executeSync` deadlock guardrail: tracked in [#1725](https://github.com/everruns/bashkit/issues/1725).
