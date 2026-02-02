//! Sandbox Identity Configuration Example
//!
//! Demonstrates how to configure custom username and hostname for the sandbox.
//! This is useful for simulating specific environments or user contexts.
//!
//! Run with: cargo run --example sandbox_identity

use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Default Sandbox Identity ===\n");

    // Default sandbox identity
    let mut bash = Bash::new();

    let result = bash.exec("whoami").await?;
    println!("whoami: {}", result.stdout.trim());

    let result = bash.exec("hostname").await?;
    println!("hostname: {}", result.stdout.trim());

    let result = bash.exec("id").await?;
    println!("id: {}", result.stdout.trim());

    let result = bash.exec("uname -n").await?;
    println!("uname -n: {}", result.stdout.trim());

    println!("\n=== Custom Sandbox Identity ===\n");

    // Custom username and hostname
    let mut bash = Bash::builder()
        .username("deploy")
        .hostname("prod-server-01")
        .build();

    let result = bash.exec("whoami").await?;
    println!("whoami: {}", result.stdout.trim());

    let result = bash.exec("hostname").await?;
    println!("hostname: {}", result.stdout.trim());

    let result = bash.exec("id").await?;
    println!("id: {}", result.stdout.trim());

    let result = bash.exec("uname -n").await?;
    println!("uname -n: {}", result.stdout.trim());

    // USER env var is automatically set
    let result = bash.exec("echo $USER").await?;
    println!("$USER: {}", result.stdout.trim());

    println!("\n=== Multi-Tenant Isolation ===\n");

    // Each tenant gets their own identity
    let mut tenant_a = Bash::builder()
        .username("alice")
        .hostname("tenant-a.example.com")
        .build();

    let mut tenant_b = Bash::builder()
        .username("bob")
        .hostname("tenant-b.example.com")
        .build();

    let result_a = tenant_a.exec("whoami && hostname").await?;
    let result_b = tenant_b.exec("whoami && hostname").await?;

    println!(
        "Tenant A: {}",
        result_a.stdout.replace('\n', ", ").trim_end_matches(", ")
    );
    println!(
        "Tenant B: {}",
        result_b.stdout.replace('\n', ", ").trim_end_matches(", ")
    );

    Ok(())
}
