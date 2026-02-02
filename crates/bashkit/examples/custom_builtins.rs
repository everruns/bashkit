//! Custom Builtins Example
//!
//! Demonstrates how to extend bashkit with custom builtin commands.
//! Custom builtins have access to arguments, environment, filesystem, and stdin.
//!
//! Run with: cargo run --example custom_builtins

use async_trait::async_trait;
use bashkit::{Bash, Builtin, BuiltinContext, ExecResult};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A simple greeting command
struct Greet {
    default_name: String,
}

#[async_trait]
impl Builtin for Greet {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let name = ctx
            .args
            .first()
            .map(|s| s.as_str())
            .unwrap_or(&self.default_name);
        Ok(ExecResult::ok(format!("Hello, {}!\n", name)))
    }
}

/// A command that transforms input to uppercase
struct Upper;

#[async_trait]
impl Builtin for Upper {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let input = ctx.stdin.unwrap_or("");
        Ok(ExecResult::ok(input.to_uppercase()))
    }
}

/// A counter command with shared state
struct Counter {
    count: Arc<AtomicU64>,
}

#[async_trait]
impl Builtin for Counter {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let increment: u64 = ctx.args.first().and_then(|s| s.parse().ok()).unwrap_or(1);
        let new_value = self.count.fetch_add(increment, Ordering::SeqCst) + increment;
        Ok(ExecResult::ok(format!("{}\n", new_value)))
    }
}

/// A command that reads environment variables
struct EnvReader;

#[async_trait]
impl Builtin for EnvReader {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let var_name = ctx.args.first().map(|s| s.as_str()).unwrap_or("HOME");
        let value = ctx
            .env
            .get(var_name)
            .map(|s| s.as_str())
            .unwrap_or("(not set)");
        Ok(ExecResult::ok(format!("{}={}\n", var_name, value)))
    }
}

/// A key-value store command (simulating a database)
struct KvStore {
    data: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
}

#[async_trait]
impl Builtin for KvStore {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let cmd = ctx.args.first().map(|s| s.as_str()).unwrap_or("help");

        match cmd {
            "get" => {
                let key = ctx.args.get(1).map(|s| s.as_str()).unwrap_or("");
                let data = self.data.read().await;
                match data.get(key) {
                    Some(value) => Ok(ExecResult::ok(format!("{}\n", value))),
                    None => Ok(ExecResult::err(format!("key not found: {}\n", key), 1)),
                }
            }
            "set" => {
                let key = ctx.args.get(1).cloned().unwrap_or_default();
                let value = ctx.args.get(2).cloned().unwrap_or_default();
                let mut data = self.data.write().await;
                data.insert(key, value);
                Ok(ExecResult::ok(String::new()))
            }
            "list" => {
                let data = self.data.read().await;
                let mut output = String::new();
                for (k, v) in data.iter() {
                    output.push_str(&format!("{}={}\n", k, v));
                }
                Ok(ExecResult::ok(output))
            }
            _ => Ok(ExecResult::err(
                "Usage: kv <get|set|list> [key] [value]\n".to_string(),
                1,
            )),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Custom Builtins Demo ===\n");

    // Create shared state for stateful builtins
    let counter = Arc::new(AtomicU64::new(0));
    let kv_data = Arc::new(tokio::sync::RwLock::new(HashMap::new()));

    // Build bash with custom builtins
    let mut bash = Bash::builder()
        .env("API_KEY", "secret-123")
        .env("DATABASE_URL", "postgres://localhost/mydb")
        .builtin(
            "greet",
            Box::new(Greet {
                default_name: "World".to_string(),
            }),
        )
        .builtin("upper", Box::new(Upper))
        .builtin("counter", Box::new(Counter { count: counter }))
        .builtin("getenv", Box::new(EnvReader))
        .builtin("kv", Box::new(KvStore { data: kv_data }))
        .build();

    // Demo 1: Simple greeting command
    println!("--- Greeting Command ---");
    let result = bash.exec("greet").await?;
    print!("greet: {}", result.stdout);

    let result = bash.exec("greet Alice").await?;
    print!("greet Alice: {}", result.stdout);

    // Demo 2: Pipeline with custom command
    println!("\n--- Pipeline with Upper ---");
    let result = bash.exec("echo 'hello world' | upper").await?;
    print!("echo 'hello world' | upper: {}", result.stdout);

    // Demo 3: Stateful counter
    println!("\n--- Stateful Counter ---");
    let result = bash.exec("counter").await?;
    print!("counter (1st call): {}", result.stdout);

    let result = bash.exec("counter").await?;
    print!("counter (2nd call): {}", result.stdout);

    let result = bash.exec("counter 10").await?;
    print!("counter 10: {}", result.stdout);

    // Demo 4: Environment access
    println!("\n--- Environment Access ---");
    let result = bash.exec("getenv API_KEY").await?;
    print!("{}", result.stdout);

    let result = bash.exec("getenv DATABASE_URL").await?;
    print!("{}", result.stdout);

    // Demo 5: Key-value store (simulating database)
    println!("\n--- Key-Value Store ---");
    bash.exec("kv set name Alice").await?;
    bash.exec("kv set email alice@example.com").await?;

    let result = bash.exec("kv get name").await?;
    print!("kv get name: {}", result.stdout);

    let result = bash.exec("kv list").await?;
    println!("kv list:\n{}", result.stdout);

    // Demo 6: Custom builtins in scripts
    println!("--- Custom Builtins in Scripts ---");
    let script = r#"
        for name in Alice Bob Charlie; do
            greet $name
        done
    "#;
    let result = bash.exec(script).await?;
    print!("{}", result.stdout);

    // Demo 7: Conditional execution
    println!("\n--- Conditional Execution ---");
    let result = bash
        .exec("kv get missing || echo 'Key not found, using default'")
        .await?;
    print!("{}", result.stdout);

    println!("\n=== Demo Complete ===");
    Ok(())
}
