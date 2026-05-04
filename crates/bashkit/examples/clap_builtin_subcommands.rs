//! Clap-backed custom builtin with subcommands.
//!
//! Run with: cargo run --example clap_builtin_subcommands

use async_trait::async_trait;
use bashkit::clap::{Parser, Subcommand};
use bashkit::{Bash, BashkitContext, ClapBuiltin};

#[derive(Parser)]
#[command(name = "kv", about = "Small key-value command")]
struct KvArgs {
    #[command(subcommand)]
    command: KvCommand,
}

#[derive(Subcommand)]
enum KvCommand {
    Get { key: String },
    Set { key: String, value: String },
}

struct Kv;

#[async_trait]
impl ClapBuiltin for Kv {
    type Args = KvArgs;

    async fn execute_clap(
        &self,
        args: Self::Args,
        ctx: &mut BashkitContext<'_>,
    ) -> bashkit::Result<()> {
        match args.command {
            KvCommand::Get { key } => {
                let value = ctx
                    .variables()
                    .get(&format!("KV_{key}"))
                    .cloned()
                    .unwrap_or_default();
                ctx.write_stdout(format!("{value}\n"));
            }
            KvCommand::Set { key, value } => {
                ctx.variables().insert(format!("KV_{key}"), value);
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut bash = Bash::builder().builtin("kv", Box::new(Kv)).build();

    let result = bash.exec("kv set color blue && kv get color").await?;
    assert_eq!(result.stdout, "blue\n");
    print!("{}", result.stdout);

    let bad = bash.exec("kv delete color").await?;
    assert_eq!(bad.exit_code, 2);
    assert!(bad.stderr.contains("unrecognized subcommand"));

    Ok(())
}
