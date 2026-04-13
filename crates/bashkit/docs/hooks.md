# Hooks

Bashkit provides an interceptor hook system that lets you observe, modify, or
cancel operations at key points in the execution pipeline. Hooks are registered
at build time via [`BashBuilder`] and are immutable after construction.

**See also:**
- [Custom Builtins](./custom_builtins.md) - Extending the shell with commands
- [Threat Model](./threat-model.md) - Security considerations
- [API Documentation](https://docs.rs/bashkit) - Full API reference

## Quick Start

```rust
use bashkit::{Bash, hooks::{HookAction, ExecInput}};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder()
    .before_exec(Box::new(|input: ExecInput| {
        println!("about to run: {}", input.script);
        HookAction::Continue(input)
    }))
    .build();

let result = bash.exec("echo hello").await?;
assert_eq!(result.stdout, "hello\n");
# Ok(())
# }
```

## How Hooks Work

Every hook is an **interceptor** — a closure that receives owned data and must
return a [`HookAction`]:

- **`HookAction::Continue(value)`** — proceed with the (possibly modified) value
- **`HookAction::Cancel(reason)`** — abort the operation

Multiple hooks of the same type run in registration order. If any hook returns
`Cancel`, later hooks are skipped and the operation is aborted.

Hooks have **zero overhead** when none are registered — the interpreter checks
`Vec::is_empty()` and skips the hook path entirely.

## Hook Types

| Hook | Fires | Can modify | Can cancel |
|------|-------|------------|------------|
| `before_exec` | Before script execution | Script text | Yes |
| `after_exec` | After script execution | stdout, stderr, exit code | No* |
| `before_tool` | Before a builtin command runs | Tool name, args | Yes |
| `after_tool` | After a builtin command completes | Tool name, stdout, exit code | No* |
| `on_exit` | When `exit` builtin runs | Exit code | Yes |
| `on_error` | On interpreter error | Error message | No* |
| `before_http` | Before HTTP request (after allowlist) | URL, method, headers | Yes |
| `after_http` | After HTTP response received | URL, status, headers | No* |

*These hooks receive `Continue`/`Cancel` for API consistency, but cancelling an
already-completed operation is a no-op in practice.

## Execution Hooks

### `before_exec` — Modify or Block Scripts

Fires before each `bash.exec()` call. The hook receives an [`ExecInput`] with
the script text and can rewrite or cancel it.

```rust
use bashkit::{Bash, hooks::{HookAction, ExecInput}};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder()
    .before_exec(Box::new(|mut input: ExecInput| {
        // Block dangerous commands
        if input.script.contains("rm -rf") {
            return HookAction::Cancel("destructive command blocked".into());
        }
        // Rewrite scripts on the fly
        input.script = input.script.replace("world", "hooks");
        HookAction::Continue(input)
    }))
    .build();

let result = bash.exec("echo hello world").await?;
assert_eq!(result.stdout, "hello hooks\n");

// Cancelled scripts return exit code 1
let result = bash.exec("rm -rf /").await?;
assert_eq!(result.exit_code, 1);
# Ok(())
# }
```

### `after_exec` — Observe Results

Fires after script execution completes. Useful for logging, metrics, or
post-processing.

```rust
use bashkit::{Bash, hooks::{HookAction, ExecOutput}};
use std::sync::{Arc, Mutex};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let log = Arc::new(Mutex::new(Vec::new()));
let log_clone = log.clone();

let mut bash = Bash::builder()
    .after_exec(Box::new(move |output: ExecOutput| {
        log_clone.lock().unwrap().push(format!(
            "[exit {}] {}",
            output.exit_code,
            output.script,
        ));
        HookAction::Continue(output)
    }))
    .build();

bash.exec("echo first").await?;
bash.exec("echo second").await?;

let entries = log.lock().unwrap();
assert_eq!(entries.len(), 2);
assert!(entries[0].contains("first"));
# Ok(())
# }
```

## Tool Hooks

Tool hooks fire around **registered builtin commands** (e.g. `echo`, `cat`,
`grep`). They do not fire for shell-level special builtins like `declare`,
`local`, or `export`.

### `before_tool` — Intercept Commands

```rust
use bashkit::{Bash, hooks::{HookAction, ToolEvent}};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder()
    .before_tool(Box::new(|event: ToolEvent| {
        // Block specific commands
        if event.name == "curl" {
            return HookAction::Cancel("curl is disabled".into());
        }
        HookAction::Continue(event)
    }))
    .build();

let result = bash.exec("echo allowed").await?;
assert_eq!(result.exit_code, 0);
# Ok(())
# }
```

### `after_tool` — Audit Command Results

```rust
use bashkit::{Bash, hooks::{HookAction, ToolResult}};
use std::sync::{Arc, Mutex};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let results = Arc::new(Mutex::new(Vec::new()));
let results_clone = results.clone();

let mut bash = Bash::builder()
    .after_tool(Box::new(move |result: ToolResult| {
        results_clone.lock().unwrap().push((
            result.name.clone(),
            result.exit_code,
        ));
        HookAction::Continue(result)
    }))
    .build();

bash.exec("echo hello").await?;

let captured = results.lock().unwrap();
assert_eq!(captured[0].0, "echo");
assert_eq!(captured[0].1, 0);
# Ok(())
# }
```

## Lifecycle Hooks

### `on_exit` — Handle Script Exit

Fires when the `exit` builtin is called. Can modify the exit code or prevent
the exit entirely.

```rust
use bashkit::{Bash, hooks::{HookAction, ExitEvent}};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

# fn main() {
let exited = Arc::new(AtomicBool::new(false));
let flag = exited.clone();

let bash = Bash::builder()
    .on_exit(Box::new(move |event: ExitEvent| {
        flag.store(true, Ordering::Relaxed);
        HookAction::Continue(event)
    }))
    .build();
# }
```

### `on_error` — Handle Errors

Fires when the interpreter encounters an error (parse errors, runtime errors).

```rust
use bashkit::{Bash, hooks::{HookAction, ErrorEvent}};
use std::sync::{Arc, Mutex};

# fn main() {
let errors = Arc::new(Mutex::new(Vec::new()));
let errors_clone = errors.clone();

let bash = Bash::builder()
    .on_error(Box::new(move |event: ErrorEvent| {
        errors_clone.lock().unwrap().push(event.message.clone());
        HookAction::Continue(event)
    }))
    .build();
# }
```

## HTTP Hooks

HTTP hooks require the `http_client` feature (enabled by default). They fire
around HTTP requests made by `curl`, `wget`, and `http` builtins.

HTTP hooks fire **after** the [`NetworkAllowlist`] check, so the security
boundary stays in bashkit — hooks cannot bypass the allowlist.

### `before_http` — Filter or Modify Requests

```rust
use bashkit::{Bash, NetworkAllowlist, hooks::{HookAction, HttpRequestEvent}};

# fn main() {
let bash = Bash::builder()
    .network(NetworkAllowlist::allow_all())
    .before_http(Box::new(|mut req: HttpRequestEvent| {
        // Add a custom header to all requests
        req.headers.push(("X-Source".into(), "bashkit".into()));

        // Block requests to certain domains
        if req.url.contains("blocked.example.com") {
            return HookAction::Cancel("blocked by policy".into());
        }

        HookAction::Continue(req)
    }))
    .build();
# }
```

### `after_http` — Observe Responses

```rust
use bashkit::{Bash, NetworkAllowlist, hooks::{HookAction, HttpResponseEvent}};
use std::sync::{Arc, Mutex};

# fn main() {
let responses = Arc::new(Mutex::new(Vec::new()));
let responses_clone = responses.clone();

let bash = Bash::builder()
    .network(NetworkAllowlist::allow_all())
    .after_http(Box::new(move |resp: HttpResponseEvent| {
        responses_clone.lock().unwrap().push((
            resp.url.clone(),
            resp.status,
        ));
        HookAction::Continue(resp)
    }))
    .build();
# }
```

## Chaining Multiple Hooks

Multiple hooks of the same type run in registration order. Each hook receives
the output of the previous one:

```rust
use bashkit::{Bash, hooks::{HookAction, ExecInput}};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder()
    .before_exec(Box::new(|mut input: ExecInput| {
        input.script = input.script.replace("world", "hooks");
        HookAction::Continue(input)
    }))
    .before_exec(Box::new(|mut input: ExecInput| {
        input.script = input.script.replace("hello", "greetings");
        HookAction::Continue(input)
    }))
    .build();

let result = bash.exec("echo hello world").await?;
assert_eq!(result.stdout, "greetings hooks\n");
# Ok(())
# }
```

If any hook in the chain returns `Cancel`, the remaining hooks are skipped:

```rust
use bashkit::{Bash, hooks::{HookAction, ExecInput}};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder()
    .before_exec(Box::new(|_input: ExecInput| {
        HookAction::Cancel("first hook cancelled".into())
    }))
    .before_exec(Box::new(|input: ExecInput| {
        // This hook never runs
        HookAction::Continue(input)
    }))
    .build();

let result = bash.exec("echo never runs").await?;
assert_eq!(result.exit_code, 1);
# Ok(())
# }
```

## Event Payloads

| Payload | Fields |
|---------|--------|
| [`ExecInput`] | `script: String` |
| [`ExecOutput`] | `script: String`, `stdout: String`, `stderr: String`, `exit_code: i32` |
| [`ToolEvent`] | `name: String`, `args: Vec<String>` |
| [`ToolResult`] | `name: String`, `stdout: String`, `exit_code: i32` |
| [`ExitEvent`] | `code: i32` |
| [`ErrorEvent`] | `message: String` |
| [`HttpRequestEvent`] | `method: String`, `url: String`, `headers: Vec<(String, String)>` |
| [`HttpResponseEvent`] | `url: String`, `status: u16`, `headers: Vec<(String, String)>` |

## Thread Safety

Hook closures must be `Send + Sync`. Use `Arc` and atomic types for shared
state between hooks and the caller:

```rust
use bashkit::{Bash, hooks::{HookAction, ExecOutput}};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

# fn main() {
let exec_count = Arc::new(AtomicU64::new(0));
let counter = exec_count.clone();

let bash = Bash::builder()
    .after_exec(Box::new(move |output: ExecOutput| {
        counter.fetch_add(1, Ordering::Relaxed);
        HookAction::Continue(output)
    }))
    .build();
# }
```

[`BashBuilder`]: crate::BashBuilder
[`HookAction`]: crate::hooks::HookAction
[`ExecInput`]: crate::hooks::ExecInput
[`ExecOutput`]: crate::hooks::ExecOutput
[`ToolEvent`]: crate::hooks::ToolEvent
[`ToolResult`]: crate::hooks::ToolResult
[`ExitEvent`]: crate::hooks::ExitEvent
[`ErrorEvent`]: crate::hooks::ErrorEvent
[`HttpRequestEvent`]: crate::hooks::HttpRequestEvent
[`HttpResponseEvent`]: crate::hooks::HttpResponseEvent
[`NetworkAllowlist`]: crate::NetworkAllowlist
