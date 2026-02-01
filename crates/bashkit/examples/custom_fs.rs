//! Custom filesystem example
//!
//! Demonstrates using virtual filesystems with BashKit.
//! Run with: cargo run --example custom_fs

use bashkit::{Bash, FileSystem, InMemoryFs, MountableFs, OverlayFs};
use std::path::Path;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== InMemoryFs Example ===\n");
    inmemory_example().await?;

    println!("\n=== OverlayFs Example ===\n");
    overlay_example().await?;

    println!("\n=== MountableFs Example ===\n");
    mountable_example().await?;

    Ok(())
}

async fn inmemory_example() -> anyhow::Result<()> {
    // All files are stored in memory - no real filesystem access
    let fs = Arc::new(InMemoryFs::new());
    let mut bash = Bash::builder().fs(fs).build();

    // Create and read files
    bash.exec("echo 'Hello from memory!' > /tmp/test.txt").await?;
    let result = bash.exec("cat /tmp/test.txt").await?;
    println!("File contents: {}", result.stdout);

    // Files persist across commands
    bash.exec("echo 'Second line' >> /tmp/test.txt").await?;
    let result = bash.exec("cat /tmp/test.txt").await?;
    println!("After append:\n{}", result.stdout);

    Ok(())
}

async fn overlay_example() -> anyhow::Result<()> {
    // OverlayFs layers a writable layer on top of a base
    let base = Arc::new(InMemoryFs::new());

    // Pre-populate the base filesystem
    base.write_file(Path::new("/data/config.txt"), b"base config").await?;

    let overlay = Arc::new(OverlayFs::new(base));
    let mut bash = Bash::builder().fs(overlay).build();

    // Read from base layer
    let result = bash.exec("cat /data/config.txt").await?;
    println!("Base config: {}", result.stdout);

    // Writes go to the overlay layer
    bash.exec("echo 'overlay config' > /data/config.txt").await?;
    let result = bash.exec("cat /data/config.txt").await?;
    println!("After overlay write: {}", result.stdout);

    Ok(())
}

async fn mountable_example() -> anyhow::Result<()> {
    // MountableFs allows mounting different filesystems at different paths
    let root = Arc::new(InMemoryFs::new());
    let data_fs = Arc::new(InMemoryFs::new());

    // Pre-populate data filesystem
    data_fs.write_file(Path::new("/users.json"), br#"["alice", "bob"]"#).await?;

    let mountable = MountableFs::new(root);
    mountable.mount("/mnt/data", data_fs)?;

    let mut bash = Bash::builder().fs(Arc::new(mountable)).build();

    // Access mounted filesystem
    let result = bash.exec("cat /mnt/data/users.json | jq '.[]'").await?;
    println!("Users:\n{}", result.stdout);

    // Write to root filesystem
    bash.exec("echo 'root file' > /root.txt").await?;
    let result = bash.exec("cat /root.txt").await?;
    println!("Root file: {}", result.stdout);

    Ok(())
}
