//! Real filesystem read-write example
//!
//! Demonstrates mounting a host directory with read-write access.
//! Scripts can both read and modify host files.
//!
//! WARNING: This breaks the sandbox boundary. Only use with trusted scripts.
//!
//! Run with: cargo run --example realfs_readwrite --features realfs

use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    std::fs::write(tmp.path().join("data.txt"), "original content\n")?;
    std::fs::create_dir(tmp.path().join("output"))?;

    println!("=== RealFs ReadWrite Example ===\n");
    println!("Host directory: {}\n", tmp.path().display());

    // Mount with read-write access at /workspace
    let mut bash = Bash::builder()
        .mount_real_readwrite_at(tmp.path(), "/workspace")
        .build();

    // Read host files
    let result = bash.exec("cat /workspace/data.txt").await?;
    println!("Original: {}", result.stdout);

    // Modify host files from within the virtual bash session
    bash.exec("echo 'modified by bashkit' >> /workspace/data.txt")
        .await?;
    let result = bash.exec("cat /workspace/data.txt").await?;
    println!("After append:\n{}", result.stdout);

    // Create new files on the host
    bash.exec("echo 'report generated' > /workspace/output/report.txt")
        .await?;

    // Verify changes are visible on the host
    let data = std::fs::read_to_string(tmp.path().join("data.txt"))?;
    println!("Host sees: {}", data);
    assert!(data.contains("modified by bashkit"));

    let report = std::fs::read_to_string(tmp.path().join("output/report.txt"))?;
    println!("Host report: {}", report);
    assert_eq!(report, "report generated\n");

    println!("Host files successfully modified from virtual bash!");

    Ok(())
}
