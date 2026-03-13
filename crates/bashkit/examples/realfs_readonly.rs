//! Real filesystem readonly example
//!
//! Demonstrates mounting a host directory as readonly in the VFS.
//! Scripts can read host files but cannot modify them.
//!
//! Run with: cargo run --example realfs_readonly --features realfs

use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a temp directory with some files to mount
    let tmp = tempfile::tempdir()?;
    std::fs::write(
        tmp.path().join("README.md"),
        "# My Project\nHello from host!\n",
    )?;
    std::fs::create_dir(tmp.path().join("src"))?;
    std::fs::write(
        tmp.path().join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )?;
    std::fs::write(tmp.path().join("config.toml"), "[server]\nport = 8080\n")?;

    println!("=== RealFs Readonly Example ===\n");
    println!("Host directory: {}\n", tmp.path().display());

    // Use case 1: overlay at VFS root
    println!("--- Overlay at root ---");
    {
        let mut bash = Bash::builder().mount_real_readonly(tmp.path()).build();

        let result = bash.exec("cat /README.md").await?;
        println!("README.md:\n{}", result.stdout);

        let result = bash.exec("ls /src").await?;
        println!("ls /src: {}", result.stdout);

        let result = bash.exec("cat /src/main.rs").await?;
        println!("src/main.rs:\n{}", result.stdout);

        // Writes go to the overlay (in-memory), not the host
        bash.exec("echo 'new file' > /output.txt").await?;
        let result = bash.exec("cat /output.txt").await?;
        println!("Written to VFS overlay: {}", result.stdout);

        // Host file is unchanged
        assert!(!tmp.path().join("output.txt").exists());
        println!("Host directory unchanged (no output.txt on host)");
    }

    println!();

    // Use case 2: mount at a specific VFS path
    println!("--- Mount at /mnt/project ---");
    {
        let mut bash = Bash::builder()
            .mount_real_readonly_at(tmp.path(), "/mnt/project")
            .build();

        let result = bash.exec("cat /mnt/project/README.md").await?;
        println!("README.md:\n{}", result.stdout);

        let result = bash.exec("cat /mnt/project/config.toml").await?;
        println!("config.toml:\n{}", result.stdout);

        // VFS root is still the default InMemoryFs
        let result = bash.exec("ls /tmp").await?;
        println!("/tmp exists in VFS: {}", result.exit_code == 0);
    }

    // Verify host files are untouched
    let readme = std::fs::read_to_string(tmp.path().join("README.md"))?;
    assert_eq!(readme, "# My Project\nHello from host!\n");
    println!("\nHost files verified unchanged.");

    Ok(())
}
