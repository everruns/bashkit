//! Minimal agent example using BashKit as a Bash tool
//!
//! This demonstrates how to expose bash execution as a tool for an AI agent.
//! The agent writes poems and processes them using available builtins.
//!
//! Run with: cargo run --example agent_tool

use bashkit::{Bash, InMemoryFs};
use serde::Serialize;
use std::sync::Arc;

/// Tool definition exposed to the agent (matches typical LLM tool schemas)
#[derive(Debug, Serialize)]
struct BashToolDef {
    name: &'static str,
    description: &'static str,
    input_schema: InputSchema,
}

#[derive(Debug, Serialize)]
struct InputSchema {
    #[serde(rename = "type")]
    schema_type: &'static str,
    properties: Properties,
    required: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct Properties {
    command: PropertyDef,
}

#[derive(Debug, Serialize)]
struct PropertyDef {
    #[serde(rename = "type")]
    prop_type: &'static str,
    description: &'static str,
}

impl BashToolDef {
    fn new() -> Self {
        Self {
            name: "bash",
            description: "Execute bash commands in a sandboxed session. Variables, functions, and state persist between calls.",
            input_schema: InputSchema {
                schema_type: "object",
                properties: Properties {
                    command: PropertyDef {
                        prop_type: "string",
                        description: "The bash command to execute",
                    },
                },
                required: vec!["command"],
            },
        }
    }
}

/// Tool result back to agent
#[derive(Debug, Serialize)]
struct ToolResult {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

/// Simple agent that uses BashKit as its Bash tool
struct Agent {
    bash: Bash,
}

impl Agent {
    fn new() -> Self {
        // Use in-memory filesystem for sandboxing
        let fs = Arc::new(InMemoryFs::new());
        Self {
            bash: Bash::builder().fs(fs).build(),
        }
    }

    /// Execute a bash command - this is the "Bash tool" implementation
    async fn call_bash_tool(&mut self, command: &str) -> ToolResult {
        match self.bash.exec(command).await {
            Ok(result) => ToolResult {
                stdout: result.stdout,
                stderr: result.stderr,
                exit_code: result.exit_code,
            },
            Err(e) => ToolResult {
                stdout: String::new(),
                stderr: e.to_string(),
                exit_code: 1,
            },
        }
    }
}

/// Simulates what an LLM agent would do - plan and execute tasks via tool calls
async fn run_agent_simulation(agent: &mut Agent) {
    println!("Agent: I'll write poems to files and demonstrate text processing.\n");

    // These represent tool calls the LLM would make
    let tool_calls: Vec<(&str, &str)> = vec![
        // 1. Write a nature poem
        (
            "Writing nature poem to file",
            r#"echo 'Tall oaks stand in morning light,
Their branches reaching, holding tight,
To memories of seasons past,
In roots so deep, forever cast.' > /nature.txt"#,
        ),
        // 2. Write a space poem
        (
            "Writing space poem to file",
            r#"echo 'A million suns burn far away,
In cosmic dance they twist and sway,
Each one a world we may not know,
Yet still they shine, they burn, they glow.' > /space.txt"#,
        ),
        // 3. Write an ocean poem
        (
            "Writing ocean poem to file",
            r#"echo 'The waves roll in with endless grace,
Each one a whisper, soft embrace,
They crash and foam upon the shore,
Then pull away to come once more.' > /ocean.txt"#,
        ),
        // 4. Define a helper function (demonstrates state persistence)
        (
            "Defining display_poem function",
            r#"display_poem() {
    echo "=== $1 ==="
    cat "$1"
    echo ""
}"#,
        ),
        // 5. Display all poems using the function
        ("Displaying nature poem", "display_poem /nature.txt"),
        ("Displaying space poem", "display_poem /space.txt"),
        ("Displaying ocean poem", "display_poem /ocean.txt"),
        // 6. Use grep to find poems about light
        (
            "Searching for poems mentioning 'light'",
            "grep -l light /*.txt",
        ),
        // 7. Count lines across all poems
        (
            "Counting total lines in all poems",
            r#"TOTAL=0; for f in /*.txt; do LINES=$(cat "$f" | grep -c '.'); TOTAL=$((TOTAL + LINES)); done; echo "Total lines: $TOTAL""#,
        ),
        // 8. Store result in variable (demonstrates persistence)
        (
            "Storing poem count in variable",
            "POEM_COUNT=3; echo \"Poems written: $POEM_COUNT\"",
        ),
    ];

    // Execute each tool call
    for (i, (description, command)) in tool_calls.iter().enumerate() {
        println!("--- Tool Call {} ---", i + 1);
        println!("Agent intent: {}", description);

        // Show first line of command (truncate multiline)
        let cmd_preview = command.lines().next().unwrap_or(command);
        if command.lines().count() > 1 {
            println!("Command: {}...", cmd_preview);
        } else {
            println!("Command: {}", cmd_preview);
        }

        let result = agent.call_bash_tool(command).await;

        if !result.stdout.is_empty() {
            println!("Output:\n{}", result.stdout);
        }
        if !result.stderr.is_empty() {
            println!("Stderr: {}", result.stderr);
        }
        if result.exit_code != 0 {
            println!("Exit code: {}", result.exit_code);
        }
        println!();
    }

    // Demonstrate that state persists
    println!("--- Verifying Session State ---");
    let result = agent
        .call_bash_tool("echo \"POEM_COUNT is still: $POEM_COUNT\"")
        .await;
    println!("Output: {}", result.stdout);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Agent with Bash Tool Example ===\n");

    // Show tool definition (what would be sent to LLM)
    let tool_def = BashToolDef::new();
    println!("Tool definition for LLM:");
    println!("{}\n", serde_json::to_string_pretty(&tool_def)?);

    // Create and run agent
    let mut agent = Agent::new();
    run_agent_simulation(&mut agent).await;

    println!("=== Agent completed successfully ===");
    Ok(())
}
