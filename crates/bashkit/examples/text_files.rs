//! Text file pre-population example
//!
//! Demonstrates using `text_file()` and `readonly_text()` builder methods
//! to pre-populate the virtual filesystem before running scripts.
//!
//! Run with: cargo run --example text_files

use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Text File Pre-population Example ===\n");

    basic_text_files().await?;
    println!();

    readonly_config_files().await?;
    println!();

    json_data_files().await?;
    println!();

    mixed_permissions().await?;

    Ok(())
}

/// Basic text file usage
async fn basic_text_files() -> anyhow::Result<()> {
    println!("--- Basic Text Files ---");

    // Pre-populate files using the builder
    let mut bash = Bash::builder()
        .text_file(
            "/config/app.conf",
            "debug=true\nport=8080\nhost=localhost\n",
        )
        .text_file("/data/greeting.txt", "Hello from pre-populated file!")
        .build();

    // Scripts can read the pre-populated files
    println!("Reading config file:");
    let result = bash.exec("cat /config/app.conf").await?;
    print!("{}", result.stdout);

    println!("\nGreeting file:");
    let result = bash.exec("cat /data/greeting.txt").await?;
    println!("{}", result.stdout);

    // Scripts can also modify writable files
    bash.exec("echo 'log_level=info' >> /config/app.conf")
        .await?;
    println!("After appending to config:");
    let result = bash.exec("cat /config/app.conf").await?;
    print!("{}", result.stdout);

    Ok(())
}

/// Readonly configuration files
async fn readonly_config_files() -> anyhow::Result<()> {
    println!("--- Readonly Configuration Files ---");

    // Use readonly_text for files that shouldn't be modified
    let bash = Bash::builder()
        .readonly_text("/etc/version", "1.2.3")
        .readonly_text(
            "/etc/system.conf",
            "# System configuration (readonly)\nmode=production\n",
        )
        .build();

    // Read the readonly files
    println!("System version:");
    let mut bash_mut = bash;
    let result = bash_mut.exec("cat /etc/version").await?;
    println!("{}", result.stdout);

    // Check file permissions
    let stat = bash_mut
        .fs()
        .stat(std::path::Path::new("/etc/version"))
        .await?;
    println!("Permission mode: {:o} (readonly)", stat.mode);

    Ok(())
}

/// JSON data files for script processing
async fn json_data_files() -> anyhow::Result<()> {
    println!("--- JSON Data Files ---");

    let mut bash = Bash::builder()
        .text_file(
            "/data/users.json",
            r#"[{"name": "alice", "role": "admin"}, {"name": "bob", "role": "user"}]"#,
        )
        .text_file(
            "/data/config.json",
            r#"{"api_url": "https://api.example.com", "timeout": 30}"#,
        )
        .build();

    // Process JSON with jq
    println!("User data:");
    let result = bash.exec("cat /data/users.json | jq '.[0].name'").await?;
    print!("First user: {}", result.stdout);

    println!("Config data:");
    let result = bash.exec("cat /data/config.json | jq '.timeout'").await?;
    print!("Timeout: {}", result.stdout);

    Ok(())
}

/// Mix of writable and readonly files
async fn mixed_permissions() -> anyhow::Result<()> {
    println!("--- Mixed Permissions ---");

    let bash = Bash::builder()
        // Readonly system files
        .readonly_text("/etc/hostname", "sandbox-host")
        .readonly_text("/etc/os-release", "NAME=\"BashKit\"\nVERSION=\"1.0\"\n")
        // Writable workspace files
        .text_file("/workspace/notes.txt", "Initial notes\n")
        .text_file(
            "/workspace/data.csv",
            "id,name,value\n1,foo,100\n2,bar,200\n",
        )
        .build();

    let mut bash = bash;

    // Read system info
    println!("Hostname:");
    let result = bash.exec("cat /etc/hostname").await?;
    println!("{}", result.stdout);

    // Process CSV data
    println!("CSV data:");
    let result = bash.exec("cat /workspace/data.csv | head -2").await?;
    print!("{}", result.stdout);

    // Modify workspace file
    bash.exec("echo '3,baz,300' >> /workspace/data.csv").await?;
    println!("\nAfter adding row:");
    let result = bash.exec("cat /workspace/data.csv").await?;
    print!("{}", result.stdout);

    Ok(())
}
