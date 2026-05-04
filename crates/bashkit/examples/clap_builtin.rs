//! Clap-backed custom builtin example.
//!
//! Run with: cargo run --example clap_builtin

use async_trait::async_trait;
use bashkit::clap::Parser;
use bashkit::{Bash, BashkitContext, ClapBuiltin};

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
async fn main() -> anyhow::Result<()> {
    let mut bash = Bash::builder().builtin("greet", Box::new(Greet)).build();

    let result = bash.exec("greet --name Alice --shout").await?;
    assert_eq!(result.stdout, "HELLO, ALICE!\n");
    print!("{}", result.stdout);

    let help = bash.exec("greet --help").await?;
    assert_eq!(help.exit_code, 0);
    assert!(help.stdout.contains("Usage: greet"));

    Ok(())
}
