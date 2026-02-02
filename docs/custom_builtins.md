# Custom Builtins

BashKit supports registering custom builtin commands to extend the shell with
domain-specific functionality. Custom builtins have full access to the execution
context including arguments, environment variables, shell variables, and the
virtual filesystem.

**See also:**
- [API Documentation](https://docs.rs/bashkit) - Full API reference
- [Compatibility Reference](./compatibility.md) - Supported bash features

## Quick Start

```rust,ignore
use bashkit::{Bash, Builtin, BuiltinContext, ExecResult, async_trait};

struct MyCommand;

#[async_trait]
impl Builtin for MyCommand {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let name = ctx.args.first().map(|s| s.as_str()).unwrap_or("World");
        Ok(ExecResult::ok(format!("Hello, {}!\n", name)))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut bash = Bash::builder()
        .builtin("greet", Box::new(MyCommand))
        .build();

    let result = bash.exec("greet Alice").await?;
    assert_eq!(result.stdout, "Hello, Alice!\n");
    Ok(())
}
```

## The Builtin Trait

All custom builtins must implement the `Builtin` trait:

```rust,ignore
#[async_trait]
pub trait Builtin: Send + Sync {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> Result<ExecResult>;
}
```

The trait is async-first (via `async_trait`) and requires `Send + Sync` for
thread safety in async contexts.

## Execution Context

The `BuiltinContext` provides access to the execution environment:

```rust,ignore
pub struct BuiltinContext<'a> {
    /// Command arguments (not including the command name)
    pub args: &'a [String],

    /// Environment variables
    pub env: &'a HashMap<String, String>,

    /// Shell variables (mutable)
    pub variables: &'a mut HashMap<String, String>,

    /// Current working directory (mutable)
    pub cwd: &'a mut PathBuf,

    /// Virtual filesystem
    pub fs: Arc<dyn FileSystem>,

    /// Standard input (from pipeline)
    pub stdin: Option<&'a str>,
}
```

### Arguments

Arguments are passed as a slice of strings, excluding the command name itself:

```rust,ignore
// For "mycommand arg1 arg2", ctx.args = ["arg1", "arg2"]
let first_arg = ctx.args.first().map(|s| s.as_str()).unwrap_or("default");
```

### Environment Variables

Read-only access to environment variables set via `BashBuilder::env()` or `export`:

```rust,ignore
let home = ctx.env.get("HOME").map(|s| s.as_str()).unwrap_or("/");
```

### Shell Variables

Mutable access to shell variables allows builtins to set variables:

```rust,ignore
ctx.variables.insert("RESULT".to_string(), "computed_value".to_string());
```

### Filesystem Access

The virtual filesystem supports all standard operations:

```rust,ignore
// Read a file
let content = ctx.fs.read_file(Path::new("/data/input.txt")).await?;

// Write a file
ctx.fs.write_file(Path::new("/output/result.txt"), b"output").await?;

// Check existence
if ctx.fs.exists(Path::new("/config")).await? {
    // ...
}
```

### Standard Input

When the builtin is invoked in a pipeline, stdin contains the output from the
previous command:

```rust,ignore
// echo "hello" | mycommand
let input = ctx.stdin.unwrap_or("");
let processed = input.to_uppercase();
```

## Return Values

Builtins return `Result<ExecResult>`:

```rust,ignore
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}
```

Helper constructors:

```rust,ignore
// Success with output
ExecResult::ok("output\n".to_string())

// Error with message and exit code
ExecResult::err("error message\n".to_string(), 1)
```

## Examples

### Database Query Builtin

```rust,ignore
use bashkit::{Bash, Builtin, BuiltinContext, ExecResult, async_trait};
use sqlx::PgPool;
use std::sync::Arc;

struct Psql {
    pool: Arc<PgPool>,
}

#[async_trait]
impl Builtin for Psql {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        // Parse -c "query" argument
        let query = match ctx.args.iter().position(|a| a == "-c") {
            Some(i) => ctx.args.get(i + 1).map(|s| s.as_str()).unwrap_or(""),
            None => return Ok(ExecResult::err("Usage: psql -c 'query'\n".into(), 1)),
        };

        // Execute query (simplified - real impl would format results)
        match sqlx::query(query).fetch_all(&*self.pool).await {
            Ok(rows) => Ok(ExecResult::ok(format!("{} rows\n", rows.len()))),
            Err(e) => Ok(ExecResult::err(format!("ERROR: {}\n", e), 1)),
        }
    }
}

// Usage
let pool = Arc::new(PgPool::connect("postgres://...").await?);
let mut bash = Bash::builder()
    .builtin("psql", Box::new(Psql { pool }))
    .build();

bash.exec("psql -c 'SELECT * FROM users'").await?;
```

### HTTP Client Builtin

```rust,ignore
struct HttpGet {
    client: reqwest::Client,
}

#[async_trait]
impl Builtin for HttpGet {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let url = match ctx.args.first() {
            Some(url) => url,
            None => return Ok(ExecResult::err("Usage: httpget <url>\n".into(), 1)),
        };

        match self.client.get(url).send().await {
            Ok(resp) => {
                let body = resp.text().await.unwrap_or_default();
                Ok(ExecResult::ok(body))
            }
            Err(e) => Ok(ExecResult::err(format!("Error: {}\n", e), 1)),
        }
    }
}
```

### Overriding Default Builtins

Custom builtins can override default builtins by using the same name:

```rust,ignore
struct SecureEcho;

#[async_trait]
impl Builtin for SecureEcho {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        // Redact sensitive patterns
        let output: Vec<_> = ctx.args.iter()
            .map(|s| if s.contains("password") { "[REDACTED]" } else { s.as_str() })
            .collect();
        Ok(ExecResult::ok(format!("{}\n", output.join(" "))))
    }
}

let bash = Bash::builder()
    .builtin("echo", Box::new(SecureEcho))  // Overrides default echo
    .build();
```

## Best Practices

1. **Return proper exit codes**: Use 0 for success, non-zero for errors
2. **Include newlines**: Output should end with `\n` for proper formatting
3. **Handle missing args gracefully**: Provide usage messages for incorrect invocations
4. **Use stderr for errors**: Write error messages to `ExecResult::stderr`
5. **Keep builtins stateless when possible**: Use `Arc` for shared state that needs mutation

## Thread Safety

The `Builtin` trait requires `Send + Sync`. For builtins with mutable state, use
appropriate synchronization:

```rust,ignore
struct Counter {
    count: Arc<std::sync::atomic::AtomicU64>,
}

#[async_trait]
impl Builtin for Counter {
    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let n = self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(ExecResult::ok(format!("{}\n", n)))
    }
}
```

## Integration with Scripts

Custom builtins integrate seamlessly with bash scripting:

```bash
# Variables work
NAME="Alice"
greet $NAME

# Pipelines work
echo "hello world" | upper | head -1

# Conditionals work
if mycheck; then
    echo "passed"
else
    echo "failed"
fi

# Loops work
for item in a b c; do
    process $item
done
```
