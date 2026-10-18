# Streaming commands and execution

Read command input and deliver output while a script runs. Streaming commands
work with shell pipes and redirections, so you can connect host APIs and async
services to commands such as `cat` and `grep`.

A handler reads bytes from `stdin`, awaits writes to `stdout` and `stderr`, and
returns an exit status. Choose output sinks for live delivery, bounded capture
for a final result, or both. See [runtime support](#runtime-support) for the APIs
available on each surface.

## Rust

Implement `Command` and register it with `BashBuilder::command`:

```rust
use bashkit::{async_trait, Bash, Command, CommandContext, CommandStatus};

struct Copy;

#[async_trait]
impl Command for Copy {
    async fn execute(&self, ctx: CommandContext<'_>) -> bashkit::Result<CommandStatus> {
        while let Some(bytes) = ctx.stdin.read_chunk().await? {
            ctx.stdout.write_all(&bytes).await?;
        }
        Ok(CommandStatus::success())
    }
}

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder().command("copy", Copy).build();
let mut delivered = Vec::new();
let result = bash.execute("printf hello | copy")
    .stdout(&mut delivered)
    .capture(1024)
    .await?;

assert_eq!(delivered, b"hello");
assert_eq!(result.stdout.as_bytes(), b"hello");
assert_eq!(result.exit_code, 0);
# Ok(())
# }
```

`.stdout(&mut writer)` and `.stderr(&mut writer)` accept Tokio async byte
writers. The example uses a `Vec<u8>` to collect delivered bytes in the host;
replace it with your destination's async writer to forward output. For async
callbacks, use `.stdout_fn(callback)` and `.stderr_fn(callback)`, which await
callbacks returning `Result<()>`.

`.capture(n)` retains at most `n` bytes per stream in the final `ExecResult`.
Without capture, the result's output fields are empty. Output without a sink or
capture is drained and discarded. Use `as_bytes()` on captured `StreamData` for
exact bytes.

Pass stdin, invocation arguments or request-scoped extensions through
`.options(ExecOptions)`. See the [Rust API reference](https://docs.rs/bashkit/latest/bashkit/struct.Execution.html)
for the full builder API.

## Python

Install the [native Python package](start-python.md), then register an async
callable with `add_command`:

```python
import asyncio
from bashkit import Bash, CommandStatus

async def copy(ctx):
    async for chunk in ctx.stdin:
        await ctx.stdout.write(chunk)
    return CommandStatus()

async def main():
    shell = Bash()
    shell.add_command("copy", copy)

    async def show_output(chunk: bytes):
        print(repr(chunk), flush=True)

    result = await shell.execute(
        "printf hello | copy", stdout=show_output, capture=1024,
    )
    assert result.stdout_bytes == b"hello"
    assert result.exit_code == 0

asyncio.run(main())
```

`BashTool` supports the same registration and execution options. Handlers return
`CommandStatus` or an integer exit status. Read input with `async for` or
`await ctx.stdin.read_chunk()`, which returns `bytes` or `None` at EOF.

Output sinks must be async callables accepting `bytes`; await your destination
inside the callback. Use `stdout_bytes` and `stderr_bytes` for exact captured
output. The `stdout` and `stderr` text properties use lossy decoding.

An ordinary `execute(script)` captures bounded output. Passing `stdout` or
`stderr` selects sink delivery; add `capture=n` to retain a final copy as well.
Live commands and byte sinks require async `execute()`; `execute_sync()` cannot
drive them. See the [Python API reference](https://bashkit.sh/api/python/).

## JavaScript

Install the [native JavaScript package](start-node.md) for Node, Bun or Deno:

```javascript
import { Bash } from "@everruns/bashkit";

const shell = new Bash({
  commands: {
    async copy(ctx) {
      for await (const bytes of ctx.stdin) {
        await ctx.stdout.write(bytes);
      }
      return { exitCode: 0 };
    },
  },
});

const result = await shell.execute("printf hello | copy", {
  capture: 1024,
  async stdout(bytes) {
    console.log(bytes); // Uint8Array
  },
});
console.assert(result.stdout === "hello");
console.assert(result.exitCode === 0);
```

`addCommand(name, callback)` registers a handler after construction. Handlers
return a Promise resolving to an integer or `{exitCode}`. Native `BashTool`
supports the same options.

Configured [Python and TypeScript host functions](start-node.md#python-and-typescript-host-functions)
remain available with byte sinks and explicit capture. Their handlers retain
the caller's async context and receive an execution-scoped abort signal.

Sinks receive `Uint8Array` and may return `void` or a Promise; return the
Promise when your destination needs time to consume a chunk. Use
`stdin.readChunk()` to read a chunk or `null` at EOF, and
`stdoutBytes`/`stderrBytes` for exact captured output. Ordinary `execute(script)`
captures bounded output; sink delivery retains a final copy only when `capture`
is specified. Live commands and byte sinks require async `execute()`. See the
[TypeScript API reference](https://bashkit.sh/api/typescript/).

### Browser and worker

The [browser package](start-browser.md), `@everruns/bashkit-wasm`, exposes the
same `commands`, `addCommand` and byte-sink APIs. Initialize it before creating
a shell:

```javascript
import { Bash, initBashkit } from "@everruns/bashkit-wasm";

await initBashkit();

const shell = new Bash();
const result = await shell.execute("printf hello", {
  capture: 1024,
  async stdout(bytes) {
    console.log(bytes);
  },
});
console.assert(result.stdout === "hello");
```

This works on the main thread and in a module worker without `SharedArrayBuffer`
or cross-origin isolation headers. Use `execute()` for live handlers and sinks;
`executeSync()` cannot wait for host Promises.

## Capture, limits and flow control

Capture and delivery are separate choices:

| Output policy | Sink receives bytes while running | Final output retained |
| --- | --- | --- |
| Capture only | No | Up to the capture limit per stream |
| Sink only | Yes | None |
| Sink and capture | Yes | Up to the capture limit per stream |
| Neither, with Rust `Bash::execute` | No; output is discarded | None |

Python and JavaScript `execute(script)` default to bounded capture. Rust
`Bash::execute(script)` defaults to discard; use `.capture(n)` when you need a
final output value. Rust `exec()` remains the buffered convenience operation.

Capture is clamped to the sandbox's stdout/stderr byte limits. A smaller capture
limit does not shorten sink delivery. The sandbox limits still cap total
output: bytes beyond those limits are drained and discarded, and the result's
truncation flags are set. Capture truncation also sets those flags.
See [sandbox configuration](configuration.md) for resource limits.

Live pipes hold at most 4 KiB of queued bytes. The engine awaits each sink
callback before delivering its next chunk. Await writes inside handlers and
await downstream work inside sinks to preserve backpressure. Chunks are byte
segments, not complete lines or UTF-8 characters; use a streaming decoder when
converting text across chunk boundaries.

Memory retained by your destination is separate from engine capture. A growing
host array or queue can still retain the whole output even when capture is
disabled. Rust async writers flush at completion; flush inside a byte callback
when a buffered destination must make each chunk visible immediately.

Streaming builtin readers charge input to the request's aggregate byte budget.
Record buffers and whole-input collectors also reserve intermediate memory
before growing. A very long line can therefore fail the memory budget even
when pipe queues remain within 4 KiB; disabling capture does not disable these
resource limits.

Plain `cat`, line-printing `grep`, and nonsqueezing `tr` preserve live delivery
through descriptor duplication and file redirection, including `2>&1`.
Redirected files are opened once before execution. Options that collect or
summarize output can still delay delivery until their result is available.
Other utilities, including `sed`, `awk`, and `tee`, retain their buffered
execution paths.

Process substitutions `<(...)` and `>(...)` preserve raw bytes, including NUL
and invalid UTF-8, but still buffer rather than running both sides as live
tasks. Command substitution `$(...)` remains a text boundary and removes NUL.

Background jobs remain scoped to one execution and buffer output until
completion. Final job output reaches live sinks before execution completes,
including when the script does not explicitly call `wait`.

## Routing, errors and cancellation

A command context provides its name, expanded arguments (`args` in Rust,
`argv` in Python/JS), exported environment, cwd and a request-scoped filesystem,
along with its byte reader and writers. Use those capabilities inside handlers;
do not call execution or state methods on the same shell from its callbacks.

Shell redirects apply to live command output. Files are opened before handler
invocation, and writes are visible before the handler completes. Redirection
order matters:

```bash
command >/out 2>&1   # stdout and stderr go to /out
command 2>&1 >/out   # stderr keeps the original stdout destination
```

Separate stdout/stderr sinks have no shared ordering guarantee. Merge streams
with `2>&1` when you need one destination in write order.

Nonzero command status is a normal shell result. In Rust, an unhandled
broken-pipe write produces status 141; a handler that catches the error can
return its own status. Sink failure stops the execution and drops pending
engine work. Execution errors are reported as follows:

| API | Sink, cancellation or resource-limit failure |
| --- | --- |
| Rust | `Err` |
| Native Python and JavaScript | The result's `error` field |
| JS/WASM | Rejected execution Promise |

Deadlines cover pending sinks as well as engine work. In Rust, use the session's
`cancellation_token()` or `.cancel_with(token)` on the execution builder. In
Python, call `cancel()` and clear it with `clear_cancel()` before reusing the
shell. In JavaScript, pass `signal: controller.signal` in execute options to
interrupt pending delivery. Signal cancellation clears after the call finishes;
explicit `cancel()` remains set until `clearCancel()`.

Command I/O and filesystem capabilities expire when their execution ends,
including cancellation or sink failure. Retaining a context does not authorize
later I/O. Cancellation does not roll back host side effects already performed.

## Runtime support

| Surface | Streaming API | Limitations |
| --- | --- | --- |
| Rust library | Live `Command`, async writers/callbacks, bounded capture | Async execution required. |
| Native Python | `Bash`/`BashTool.add_command`, async byte sinks and capture | Native asyncio API; unavailable in Pyodide. |
| Native JavaScript: Node, Bun, Deno | `Bash`/`BashTool.addCommand`, byte sinks and capture | Async `execute()` required. |
| Browser and module worker | `@everruns/bashkit-wasm` commands, sinks and capture | Async `execute()` required; sync cannot drive host Promises. |
| Other JS/edge hosts | Same WASM API where host facilities are available | Streaming support depends on the host's timers and async callback support. |
| CLI and interactive shell | Live stdout/stderr delivery | Host stdin is still read within its existing input cap. |
| In-process Terminal | Terminal polling, screen and output APIs | General byte-sink options and the `commands` constructor option are not Terminal APIs. |
| Rust tools and scripted tools | Existing informational output streams and final tool results | Existing tool queues do not provide end-to-end bounded backpressure. Native Python/JS `BashTool.execute` byte sinks are supported separately. |
| Native C ABI | `bashkit_execute_streaming`, byte callbacks and optional capture | Callbacks are synchronous; no custom-command registration. See [C API](c-api.md#streaming-output). |
| Pyodide / JupyterLite | Existing synchronous execution | Async commands and byte sinks are unavailable. |
| WASI libraries | Shared core library | No published streaming embedding API. |
| Non-JS component / Hyperlight | Experimental component API | No streaming interface across the component boundary. |
| Native npm package's threaded WASI/browser fallback | Existing generated addon API | Public command/sink wrapper helpers are not exposed through its browser entry. Use `@everruns/bashkit-wasm` for these APIs. |

## Migration and limitations

For a handler that needs live I/O, replace returned output strings with awaited
writes, buffered stdin with reader iteration, and the returned output value with
an exit status. Buffered `Builtin` callbacks remain available for whole-input
operations and supported synchronous execution paths.

If you use Rust's script analysis API, update the type that describes where a
command appears in a script (for example, inside a function or command
substitution):

```rust
// Before
use bashkit::CommandContext;

// After
use bashkit::CommandPlacement;
```

Update references such as `CommandContext::Direct` to
`CommandPlacement::Direct` as well. Imports from `bashkit::analysis` also use
`CommandPlacement`. `bashkit::CommandContext` now names the context passed to
live command handlers, including their stdin/stdout/stderr.
This rename only affects Rust code using script analysis; see the
[script analysis guide](../crates/bashkit/docs/script-analysis.md).

The new C streaming entry point is additive within ABI v1.

Python `on_output`, native JS `onOutput` and WASM `executeWithOutput` remain
synchronous observers with buffered final results. Use the new byte sinks for
awaitable delivery. Native Python/JS execution rejects combining these observers
with explicit capture/sink options; Rust's execution builder also rejects an
`ExecOptions` output observer.

Whole-output `after_tool`/`after_exec` hooks cannot inspect or replace bytes that
have already been delivered. Live execution rejects these hooks; use buffered
execution for whole-output transformations.

Streaming is supported by live handlers and selected builtin paths, not every
shell operation. Whole-input utilities, command substitution `$(...)`, process
substitutions `<(...)`/`>(...)`, job results and text observers can still retain
complete values within existing limits. A live handler does not make a buffered
downstream utility incremental.

## See also

- [Rust custom builtins](../crates/bashkit/docs/custom_builtins.md)
- [JavaScript custom builtins](custom_builtins_js.md)
- [Sandbox configuration](configuration.md)
- [LLM tools](llm-tools.md)
- [Terminal](terminal.md)
- [C API](c-api.md)
- [Browser quickstart](start-browser.md)
- [Pyodide quickstart](start-pyodide.md)
