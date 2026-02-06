//! Git workflow example
//!
//! Demonstrates sandboxed git operations in Bashkit's virtual filesystem.
//!
//! Run with: cargo run --example git_workflow --features git

use bashkit::{Bash, GitConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Bashkit Git Workflow Example ===\n");

    // Create a Bash instance with git support
    let mut bash = Bash::builder()
        .git(
            GitConfig::new()
                .author("Example Bot", "bot@example.com")
                .allow_remote("https://github.com/example/"),
        )
        .build();

    // Initialize a repository
    println!("1. Initialize repository:");
    let result = bash.exec("git init /project").await?;
    println!("{}", result.stdout);

    // Create some files
    println!("2. Create project files:");
    bash.exec("echo '# My Project' > /project/README.md")
        .await?;
    bash.exec("echo 'fn main() { println!(\"Hello\"); }' > /project/main.rs")
        .await?;
    let result = bash.exec("ls -la /project").await?;
    println!("{}", result.stdout);

    // Check status (untracked files)
    println!("3. Check status (untracked files):");
    let result = bash.exec("cd /project && git status").await?;
    println!("{}", result.stdout);

    // Stage files
    println!("4. Stage files:");
    bash.exec("cd /project && git add README.md main.rs")
        .await?;
    let result = bash.exec("cd /project && git status").await?;
    println!("{}", result.stdout);

    // Commit
    println!("5. Create initial commit:");
    let result = bash
        .exec("cd /project && git commit -m 'Initial commit'")
        .await?;
    println!("{}", result.stdout);

    // Check status (clean)
    println!("6. Check status (clean working tree):");
    let result = bash.exec("cd /project && git status").await?;
    println!("{}", result.stdout);

    // View log
    println!("7. View commit history:");
    let result = bash.exec("cd /project && git log").await?;
    println!("{}", result.stdout);

    // Create a feature branch
    println!("8. Create and switch to feature branch:");
    let result = bash.exec("cd /project && git checkout -b feature").await?;
    println!("{}", result.stdout);

    // Make changes on feature branch
    println!("9. Make changes on feature branch:");
    bash.exec("echo 'New feature code' >> /project/main.rs")
        .await?;
    bash.exec("cd /project && git add main.rs").await?;
    let result = bash
        .exec("cd /project && git commit -m 'Add feature'")
        .await?;
    println!("{}", result.stdout);

    // List branches
    println!("10. List branches:");
    let result = bash.exec("cd /project && git branch").await?;
    println!("{}", result.stdout);

    // Switch back to master
    println!("11. Switch back to master:");
    let result = bash.exec("cd /project && git checkout master").await?;
    println!("{}", result.stdout);

    // View full log
    println!("12. View log on master:");
    let result = bash.exec("cd /project && git log").await?;
    println!("{}", result.stdout);

    // Show git config
    println!("13. Show git config:");
    let result = bash
        .exec("cd /project && git config user.name && git config user.email")
        .await?;
    println!("Author: {}", result.stdout);

    // Demonstrate remote operations (sandbox mode)
    println!("14. Add remote (URL validation):");
    let result = bash
        .exec("cd /project && git remote add origin https://github.com/example/repo.git")
        .await?;
    if result.exit_code == 0 {
        println!("Remote added successfully");
    }
    let result = bash.exec("cd /project && git remote -v").await?;
    println!("{}", result.stdout);

    // Try push (will show sandbox message)
    println!("15. Attempt push (sandbox mode):");
    let result = bash.exec("cd /project && git push origin master").await?;
    println!("{}", result.stderr);

    println!("=== Example Complete ===");
    Ok(())
}
