//! Live mount/unmount example
//!
//! Demonstrates attaching and detaching filesystems on a running Bash
//! instance without rebuilding the interpreter or losing shell state.
//!
//! Run with: cargo run --example live_mounts

use bashkit::{Bash, FileSystem, InMemoryFs};
use std::path::Path;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Live Mount/Unmount Example ===\n");

    basic_live_mount().await?;
    println!();
    state_preservation().await?;
    println!();
    hot_swap_mount().await?;

    Ok(())
}

/// Mount a filesystem on a running interpreter.
async fn basic_live_mount() -> anyhow::Result<()> {
    println!("--- Basic Live Mount ---");

    let mut bash = Bash::new();

    // Create a data filesystem and populate it
    let data_fs = Arc::new(InMemoryFs::new());
    data_fs
        .write_file(
            Path::new("/users.json"),
            br#"[{"name": "alice"}, {"name": "bob"}]"#,
        )
        .await?;
    data_fs
        .write_file(Path::new("/readme.txt"), b"Welcome to the data store")
        .await?;

    // Mount it live — no builder, no rebuild
    bash.mount("/mnt/data", data_fs)?;

    let result = bash.exec("cat /mnt/data/readme.txt").await?;
    println!("Data store: {}", result.stdout);

    let result = bash.exec("ls /mnt/data").await?;
    println!("Files: {}", result.stdout);

    // Unmount when done
    bash.unmount("/mnt/data")?;
    println!("Unmounted /mnt/data");

    let result = bash.exec("ls /mnt/data 2>&1; echo \"exit=$?\"").await?;
    println!("After unmount: {}", result.stdout.trim());

    Ok(())
}

/// Shell state (env vars, cwd) is preserved across mount operations.
async fn state_preservation() -> anyhow::Result<()> {
    println!("--- State Preservation ---");

    let mut bash = Bash::builder().env("APP_ENV", "production").build();

    // Set up shell state
    bash.exec("export SESSION_ID=abc123").await?;
    bash.exec("cd /tmp").await?;
    bash.exec("counter=0").await?;

    println!("Before mount:");
    let result = bash
        .exec("echo env=$APP_ENV session=$SESSION_ID cwd=$(pwd)")
        .await?;
    print!("  {}", result.stdout);

    // Mount a plugin filesystem
    let plugin_fs = Arc::new(InMemoryFs::new());
    plugin_fs
        .write_file(Path::new("/init.sh"), b"echo 'Plugin loaded'")
        .await?;
    bash.mount("/plugins/auth", plugin_fs)?;

    println!("After mount:");
    let result = bash
        .exec("echo env=$APP_ENV session=$SESSION_ID cwd=$(pwd)")
        .await?;
    print!("  {}", result.stdout);

    // Use the mount
    let result = bash.exec("source /plugins/auth/init.sh").await?;
    println!("  Plugin: {}", result.stdout.trim());

    // Unmount — state still preserved
    bash.unmount("/plugins/auth")?;

    println!("After unmount:");
    let result = bash
        .exec("echo env=$APP_ENV session=$SESSION_ID cwd=$(pwd)")
        .await?;
    print!("  {}", result.stdout);

    Ok(())
}

/// Hot-swap a mounted filesystem to simulate a rolling update.
async fn hot_swap_mount() -> anyhow::Result<()> {
    println!("--- Hot-Swap Mount ---");

    let mut bash = Bash::new();

    // Version 1
    let v1 = Arc::new(InMemoryFs::new());
    v1.write_file(Path::new("/version"), b"1.0.0").await?;
    v1.write_file(Path::new("/greeting"), b"Hello from v1")
        .await?;

    bash.mount("/app", v1)?;
    let result = bash.exec("cat /app/version").await?;
    println!("App version: {}", result.stdout.trim());

    // Hot-swap to version 2 — just re-mount at the same path
    let v2 = Arc::new(InMemoryFs::new());
    v2.write_file(Path::new("/version"), b"2.0.0").await?;
    v2.write_file(Path::new("/greeting"), b"Hello from v2")
        .await?;

    bash.mount("/app", v2)?;
    let result = bash.exec("cat /app/version").await?;
    println!("App version: {}", result.stdout.trim());

    let result = bash.exec("cat /app/greeting").await?;
    println!("Greeting: {}", result.stdout.trim());

    Ok(())
}
