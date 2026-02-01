//! Resource limits example
//!
//! Demonstrates setting execution limits to prevent runaway scripts.
//! Run with: cargo run --example resource_limits

use bashkit::{Bash, ExecutionLimits};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Command Limit ===\n");
    command_limit_example().await;

    println!("\n=== Loop Iteration Limit ===\n");
    loop_limit_example().await;

    println!("\n=== Function Depth Limit ===\n");
    function_depth_example().await;

    println!("\n=== Combined Limits ===\n");
    combined_limits_example().await?;

    Ok(())
}

async fn command_limit_example() {
    let limits = ExecutionLimits::new().max_commands(3);
    let mut bash = Bash::builder().limits(limits).build();

    // This will fail - trying to run 5 commands with limit of 3
    let result = bash.exec("echo 1; echo 2; echo 3; echo 4; echo 5").await;

    match result {
        Ok(_) => println!("Unexpected success"),
        Err(e) => println!("Blocked as expected: {}", e),
    }
}

async fn loop_limit_example() {
    let limits = ExecutionLimits::new().max_loop_iterations(5);
    let mut bash = Bash::builder().limits(limits).build();

    // This will fail - trying to loop 100 times with limit of 5
    let result = bash.exec("for i in $(seq 1 100); do echo $i; done").await;

    match result {
        Ok(_) => println!("Unexpected success"),
        Err(e) => println!("Blocked as expected: {}", e),
    }
}

async fn function_depth_example() {
    let limits = ExecutionLimits::new().max_function_depth(3);
    let mut bash = Bash::builder().limits(limits).build();

    // This will fail - recursive function exceeds depth limit
    let script = r#"
        recurse() {
            echo "depth: $1"
            recurse $(($1 + 1))
        }
        recurse 1
    "#;

    let result = bash.exec(script).await;

    match result {
        Ok(_) => println!("Unexpected success"),
        Err(e) => println!("Blocked as expected: {}", e),
    }
}

async fn combined_limits_example() -> anyhow::Result<()> {
    // Set reasonable limits for a production environment
    let limits = ExecutionLimits::new()
        .max_commands(1000)
        .max_loop_iterations(10000)
        .max_function_depth(50);

    let mut bash = Bash::builder()
        .limits(limits)
        .env("USER", "sandbox")
        .cwd("/home/sandbox")
        .build();

    // Normal scripts run fine within limits
    let result = bash
        .exec(
            r#"
            count=0
            for i in 1 2 3 4 5; do
                count=$((count + 1))
            done
            echo "Counted to $count"
        "#,
        )
        .await?;

    println!("Result: {}", result.stdout);
    Ok(())
}
