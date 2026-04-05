//! SSH Supabase example — `ssh supabase.sh`
//!
//! Connects to Supabase's public SSH service, exactly like running
//! `ssh supabase.sh` in a terminal. No credentials needed.
//!
//! Run with: cargo run --example ssh_supabase --features ssh

use bashkit::{Bash, SshConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Bashkit: ssh supabase.sh ===\n");

    let mut bash = Bash::builder()
        .ssh(SshConfig::new().allow("supabase.sh"))
        .build();

    println!("$ ssh supabase.sh\n");
    let result = bash.exec("ssh supabase.sh").await?;

    print!("{}", result.stdout);
    if !result.stderr.is_empty() {
        eprint!("{}", result.stderr);
    }

    println!("\nexit code: {}", result.exit_code);
    println!("\n=== Done ===");
    Ok(())
}
