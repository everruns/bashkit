# Streaming commands

Read command input and deliver output while a script runs. Register a `Command`
with `BashBuilder::command`; the handler writes bytes and returns its exit status
separately.

**See also:**

- [Custom builtins](custom_builtins.md), buffered handlers and shell extensions.
- [Streaming guide](https://bashkit.sh/docs/streaming/), Python/JS examples, runtime support and migration.
- [Execution API](https://docs.rs/bashkit/latest/bashkit/struct.Execution.html), all builder options.

## Register a live command

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

## Errors and cancellation

Nonzero `exit_code` is an ordinary shell result. Sink failure, cancellation and
resource exhaustion return `Err`. A failed sink stops the engine, including
pending handlers. `stdout_truncated` and `stderr_truncated` report capture or
sandbox delivery truncation.

`.cancel_with(token)` accepts an external atomic cancellation flag. The session's
`cancellation_token()` also remains active; reset a session cancellation flag
before the next execution. Deadlines cover pending sinks as well as engine work.
Command I/O and filesystem handles expire when the execution ends.

Use buffered execution when `after_tool` or `after_exec` must inspect or replace
whole output. Live execution rejects those hooks and `ExecOptions` output
observers; use async byte sinks for live delivery.

See the [public guide](https://bashkit.sh/docs/streaming/#capture-limits-and-flow-control)
for capture limits, memory ownership, routing and operations that still buffer.
