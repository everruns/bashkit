//! Basic Bashkit usage example
//!
//! Run with: cargo run --example basic

use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a new Bash instance
    let mut bash = Bash::new();

    // Execute a simple command
    let result = bash.exec("echo 'Hello, Bashkit!'").await?;
    println!("Output: {}", result.stdout);

    // Variable assignment and expansion
    let result = bash.exec("NAME=World; echo \"Hello, $NAME!\"").await?;
    println!("Output: {}", result.stdout);

    // Pipelines
    let result = bash
        .exec("echo -e 'apple\\nbanana\\ncherry' | grep a")
        .await?;
    println!("Filtered: {}", result.stdout);

    // Command substitution
    let result = bash
        .exec("FILES=$(echo one two three); echo \"Files: $FILES\"")
        .await?;
    println!("Output: {}", result.stdout);

    // Arithmetic
    let result = bash.exec("echo \"2 + 2 = $((2 + 2))\"").await?;
    println!("Output: {}", result.stdout);

    // Control flow
    let script = r#"
        for fruit in apple banana cherry; do
            echo "I like $fruit"
        done
    "#;
    let result = bash.exec(script).await?;
    println!("Loop output:\n{}", result.stdout);

    Ok(())
}
