# Clap Builtins

`ClapBuiltin` lets Rust applications define custom Bashkit commands with
`#[derive(clap::Parser)]` argument structs. Handlers receive a
`BashkitContext` that captures stdout, stderr, and exit code for the virtual
shell. `clap` is an unconditional dependency of `bashkit`, so this trait is
always available.

## Basic Usage

```rust
use bashkit::clap::Parser;
use bashkit::{Bash, BashkitContext, ClapBuiltin, async_trait};

#[derive(Parser)]
#[command(name = "greet", about = "Print a greeting")]
struct GreetArgs {
    #[arg(short, long, default_value = "World")]
    name: String,

    #[arg(short, long)]
    shout: bool,
}

struct Greet;

#[async_trait]
impl ClapBuiltin for Greet {
    type Args = GreetArgs;

    async fn execute_clap(
        &self,
        args: Self::Args,
        ctx: &mut BashkitContext<'_>,
    ) -> bashkit::Result<()> {
        let greeting = format!("Hello, {}!", args.name);
        let greeting = if args.shout {
            greeting.to_uppercase()
        } else {
            greeting
        };
        ctx.write_stdout(format!("{greeting}\n"));
        Ok(())
    }
}

#[tokio::main]
async fn main() -> bashkit::Result<()> {
    let mut bash = Bash::builder().builtin("greet", Box::new(Greet)).build();

    let result = bash.exec("greet --name Alice --shout").await?;
    assert_eq!(result.stdout, "HELLO, ALICE!\n");

    let help = bash.exec("greet --help").await?;
    assert_eq!(help.exit_code, 0);
    assert!(help.stdout.contains("Usage: greet"));
    assert!(help.stderr.is_empty());

    let error = bash.exec("greet --unknown").await?;
    assert_eq!(error.exit_code, 2);
    assert!(error.stderr.contains("unexpected argument"));
    Ok(())
}
```

## Subcommands

Use normal clap subcommands for nested command surfaces.

```rust
use bashkit::clap::{Parser, Subcommand};
use bashkit::{Bash, BashkitContext, ClapBuiltin, async_trait};

#[derive(Parser)]
#[command(name = "math")]
struct MathArgs {
    #[command(subcommand)]
    command: MathCommand,
}

#[derive(Subcommand)]
enum MathCommand {
    Add { left: i64, right: i64 },
    StdinLen,
}

struct Math;

#[async_trait]
impl ClapBuiltin for Math {
    type Args = MathArgs;

    async fn execute_clap(
        &self,
        args: Self::Args,
        ctx: &mut BashkitContext<'_>,
    ) -> bashkit::Result<()> {
        let value = match args.command {
            MathCommand::Add { left, right } => left + right,
            MathCommand::StdinLen => ctx.stdin().unwrap_or("").len() as i64,
        };
        ctx.write_stdout(format!("{value}\n"));
        Ok(())
    }
}

#[tokio::main]
async fn main() -> bashkit::Result<()> {
    let mut bash = Bash::builder().builtin("math", Box::new(Math)).build();

    let sum = bash.exec("math add 20 22").await?;
    assert_eq!(sum.stdout, "42\n");

    let stdin_len = bash.exec("printf abc | math stdin-len").await?;
    assert_eq!(stdin_len.stdout, "3\n");
    Ok(())
}
```

## Behavior

- The clap parser receives only the command arguments, not the shell command
  name from the script.
- `BashkitContext::write_stdout()` and `write_stderr()` append virtual command
  output; direct `println!`/`eprintln!` writes to the host process instead.
- Set non-zero command status with `ctx.set_exit_code(code)` or
  `ctx.fail(message, code)`.
- `--help` and `--version` return exit code `0` with clap output on stdout.
- Parse failures return clap's exit code, usually `2`, with diagnostics on
  stderr.
- Diagnostics are capped at 1 KB so custom builtins keep Bashkit's builtin
  error-output safety guarantees.

## See Also

- [`crates/bashkit/examples/clap_builtin.rs`](../examples/clap_builtin.rs)
- [`crates/bashkit/examples/clap_builtin_subcommands.rs`](../examples/clap_builtin_subcommands.rs)
- [`crates/bashkit/docs/custom_builtins.md`](./custom_builtins.md)
