//! Text processing example
//!
//! Demonstrates using grep, sed, awk, and jq builtins.
//! Run with: cargo run --example text_processing

use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut bash = Bash::new();

    println!("=== grep ===\n");
    grep_examples(&mut bash).await?;

    println!("\n=== sed ===\n");
    sed_examples(&mut bash).await?;

    println!("\n=== awk ===\n");
    awk_examples(&mut bash).await?;

    println!("\n=== jq ===\n");
    jq_examples(&mut bash).await?;

    Ok(())
}

async fn grep_examples(bash: &mut Bash) -> anyhow::Result<()> {
    // Basic pattern matching
    let result = bash
        .exec("echo -e 'apple\\nbanana\\napricot\\ncherry' | grep '^a'")
        .await?;
    println!("Lines starting with 'a':\n{}", result.stdout);

    // Case-insensitive search
    let result = bash
        .exec("echo -e 'Hello\\nHELLO\\nhello' | grep -i hello")
        .await?;
    println!("Case-insensitive 'hello':\n{}", result.stdout);

    // Invert match
    let result = bash
        .exec("echo -e 'error: bad\\ninfo: ok\\nerror: fail' | grep -v error")
        .await?;
    println!("Lines without 'error':\n{}", result.stdout);

    Ok(())
}

async fn sed_examples(bash: &mut Bash) -> anyhow::Result<()> {
    // Simple substitution
    let result = bash
        .exec("echo 'hello world' | sed 's/world/bash/'")
        .await?;
    println!("Substitution: {}", result.stdout);

    // Global substitution
    let result = bash.exec("echo 'aaa bbb aaa' | sed 's/aaa/XXX/g'").await?;
    println!("Global replace: {}", result.stdout);

    // Delete lines matching pattern
    let result = bash
        .exec("echo -e 'keep\\ndelete\\nkeep' | sed '/delete/d'")
        .await?;
    println!("After delete:\n{}", result.stdout);

    Ok(())
}

async fn awk_examples(bash: &mut Bash) -> anyhow::Result<()> {
    // Print specific fields
    let result = bash
        .exec("echo 'John 25 Engineer' | awk '{print $1, $3}'")
        .await?;
    println!("Fields 1 and 3: {}", result.stdout);

    // Sum a column
    let result = bash
        .exec("echo -e '10\\n20\\n30' | awk '{sum += $1} END {print sum}'")
        .await?;
    println!("Sum: {}", result.stdout);

    // Pattern matching
    let result = bash
        .exec("echo -e 'error: bad\\ninfo: ok\\nerror: fail' | awk '/error/ {print $2}'")
        .await?;
    println!("Error details:\n{}", result.stdout);

    // Custom field separator
    let result = bash.exec("echo 'a,b,c' | awk -F, '{print $2}'").await?;
    println!("CSV field 2: {}", result.stdout);

    Ok(())
}

async fn jq_examples(bash: &mut Bash) -> anyhow::Result<()> {
    // Extract a field
    let result = bash
        .exec(r#"echo '{"name": "Alice", "age": 30}' | jq '.name'"#)
        .await?;
    println!("Name field: {}", result.stdout);

    // Array access
    let result = bash
        .exec(r#"echo '{"items": ["a", "b", "c"]}' | jq '.items[1]'"#)
        .await?;
    println!("Second item: {}", result.stdout);

    // Filter array
    let result = bash
        .exec(r#"echo '[1, 2, 3, 4, 5]' | jq '.[] | select(. > 3)'"#)
        .await?;
    println!("Numbers > 3:\n{}", result.stdout);

    // Transform data
    let result = bash
        .exec(r#"echo '{"users": [{"name": "Alice"}, {"name": "Bob"}]}' | jq '.users[].name'"#)
        .await?;
    println!("User names:\n{}", result.stdout);

    Ok(())
}
