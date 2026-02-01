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

    println!("\n=== Direct Filesystem Access Example ===\n");
    direct_fs_access_example().await?;

    println!("\n=== Binary File Handling Example ===\n");
    binary_file_example().await?;

    Ok(())
}

async fn inmemory_example() -> anyhow::Result<()> {
    // All files are stored in memory - no real filesystem access
    let fs = Arc::new(InMemoryFs::new());
    let mut bash = Bash::builder().fs(fs).build();

    // Create and read files
    bash.exec("echo 'Hello from memory!' > /tmp/test.txt")
        .await?;
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

    // Pre-populate the base filesystem (create parent directory first)
    base.mkdir(Path::new("/data"), true).await?;
    base.write_file(Path::new("/data/config.txt"), b"base config")
        .await?;

    let overlay = Arc::new(OverlayFs::new(base));
    let mut bash = Bash::builder().fs(overlay).build();

    // Read from base layer
    let result = bash.exec("cat /data/config.txt").await?;
    println!("Base config: {}", result.stdout);

    // Writes go to the overlay layer
    bash.exec("echo 'overlay config' > /data/config.txt")
        .await?;
    let result = bash.exec("cat /data/config.txt").await?;
    println!("After overlay write: {}", result.stdout);

    Ok(())
}

async fn mountable_example() -> anyhow::Result<()> {
    // MountableFs allows mounting different filesystems at different paths
    let root = Arc::new(InMemoryFs::new());
    let data_fs = Arc::new(InMemoryFs::new());

    // Pre-populate data filesystem
    data_fs
        .write_file(Path::new("/users.json"), br#"["alice", "bob"]"#)
        .await?;

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

async fn direct_fs_access_example() -> anyhow::Result<()> {
    // Bash exposes fs() for direct filesystem access
    let mut bash = Bash::new();
    let fs = bash.fs();

    // Create directories and pre-populate files before running scripts
    fs.mkdir(Path::new("/config"), false).await?;
    fs.mkdir(Path::new("/output"), false).await?;
    fs.write_file(Path::new("/config/app.conf"), b"debug=true\nport=8080\n")
        .await?;

    // Bash script can access pre-populated files
    let result = bash.exec("cat /config/app.conf").await?;
    println!("Config from bash: {}", result.stdout);

    // Run a bash script that creates output
    bash.exec("echo 'processed' > /output/result.txt").await?;

    // Read the output directly without going through bash
    let output = bash.fs().read_file(Path::new("/output/result.txt")).await?;
    println!("Output bytes: {:?}", output);
    println!("Output text: {}", String::from_utf8_lossy(&output));

    // Check file metadata
    let stat = bash.fs().stat(Path::new("/output/result.txt")).await?;
    println!("File size: {} bytes", stat.size);

    // List directory contents
    let entries = bash.fs().read_dir(Path::new("/output")).await?;
    for entry in entries {
        println!("  - {} ({:?})", entry.name, entry.metadata.file_type);
    }

    Ok(())
}

async fn binary_file_example() -> anyhow::Result<()> {
    // The filesystem fully supports binary data with null bytes and high bytes
    let bash = Bash::new();
    let fs = bash.fs();

    // Create directory for binary files
    fs.mkdir(Path::new("/data"), false).await?;

    // Write binary data directly (e.g., a simple binary header)
    let binary_header = vec![
        0x89, 0x50, 0x4E, 0x47, // PNG magic bytes
        0x0D, 0x0A, 0x1A, 0x0A, // PNG header continuation
        0x00, 0x00, 0x00, 0x00, // Some null bytes
        0xFF, 0xFE, 0xFD, 0xFC, // High bytes
    ];
    fs.write_file(Path::new("/data/test.bin"), &binary_header)
        .await?;
    println!("Wrote {} bytes of binary data", binary_header.len());

    // Read it back and verify
    let read_back = fs.read_file(Path::new("/data/test.bin")).await?;
    assert_eq!(read_back, binary_header);
    println!("Binary data verified: {:02X?}", &read_back[..4]);

    // Check file stats
    let stat = fs.stat(Path::new("/data/test.bin")).await?;
    println!("File size: {} bytes", stat.size);

    // Append more binary data
    fs.append_file(Path::new("/data/test.bin"), &[0xDE, 0xAD, 0xBE, 0xEF])
        .await?;
    let final_content = fs.read_file(Path::new("/data/test.bin")).await?;
    println!("After append: {} bytes total", final_content.len());

    // Copy binary file
    fs.copy(
        Path::new("/data/test.bin"),
        Path::new("/data/test_copy.bin"),
    )
    .await?;
    let copied = fs.read_file(Path::new("/data/test_copy.bin")).await?;
    assert_eq!(copied, final_content);
    println!("Binary file copied successfully");

    Ok(())
}
