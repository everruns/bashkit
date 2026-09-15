//! Clap-backed builtin inside a ScriptedTool.
//!
//! Run with:
//!   cargo run --example scripted_tool_clap_builtin --features scripted_tool
//!
//! `ToolDef` + `parse_flags` covers `--key value` argument shapes. A control-plane
//! command tree that wants positionals, short flags, `--`, or required-argument
//! checking needs a real CLI parser. `ScriptedToolBuilder::builtin` registers a
//! raw-argv `Builtin` — including any `ClapBuiltin` — alongside the `ToolDef`
//! tools, without giving up the logic-only shell's disabled filesystem.

use async_trait::async_trait;
use bashkit::clap::Parser;
use bashkit::{BashkitContext, ClapBuiltin, ScriptedTool, Tool, ToolArgs, ToolDef};

#[derive(Parser)]
#[command(name = "get_agent", about = "Fetch one agent by id")]
struct GetAgentArgs {
    /// Positional id — `parse_flags` cannot express this at all.
    id: String,

    /// Short flag — `parse_flags` has no short-flag syntax.
    #[arg(short, long, default_value = "text")]
    format: String,
}

struct GetAgent;

#[async_trait]
impl ClapBuiltin for GetAgent {
    type Args = GetAgentArgs;

    async fn execute_clap(
        &self,
        args: Self::Args,
        ctx: &mut BashkitContext<'_>,
    ) -> bashkit::Result<()> {
        ctx.write_stdout(format!("agent={} format={}\n", args.id, args.format));
        Ok(())
    }
}

async fn run(tool: &ScriptedTool, commands: &str) -> anyhow::Result<(String, String, i64)> {
    let output = tool
        .execution(serde_json::json!({ "commands": commands }))?
        .execute()
        .await?;
    let result = &output.result;
    Ok((
        result["stdout"].as_str().unwrap_or_default().to_string(),
        result["stderr"].as_str().unwrap_or_default().to_string(),
        result["exit_code"].as_i64().unwrap_or_default(),
    ))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let tool = ScriptedTool::builder("control_plane")
        .short_description("Control-plane command tree")
        // Raw-argv builtin: clap owns parsing for this command.
        .builtin("get_agent", Box::new(GetAgent))
        // ToolDef command: schema-driven flags, now rejecting typos.
        .tool_fn(
            ToolDef::new("list_agents", "List agents").with_schema(serde_json::json!({
                "type": "object",
                "properties": { "limit": { "type": "integer" } },
                "additionalProperties": false
            })),
            |args: &ToolArgs| Ok(format!("limit={}\n", args.param_i64("limit").unwrap_or(10))),
        )
        .build();

    println!("=== clap builtin inside a ScriptedTool ===\n");

    // Positional argument — impossible through `parse_flags`.
    let (stdout, _, code) = run(&tool, "get_agent a-42").await?;
    println!("$ get_agent a-42\n{stdout}");
    assert_eq!(code, 0);
    assert_eq!(stdout, "agent=a-42 format=text\n");

    // Short flag — impossible through `parse_flags`.
    let (stdout, _, _) = run(&tool, "get_agent a-42 -f json").await?;
    println!("$ get_agent a-42 -f json\n{stdout}");
    assert_eq!(stdout, "agent=a-42 format=json\n");

    // Missing required argument fails at parse time, with usage.
    let (_, stderr, code) = run(&tool, "get_agent").await?;
    println!("$ get_agent\n{stderr}");
    assert_ne!(code, 0);
    assert!(stderr.contains("Usage: get_agent"));
    assert!(!stderr.contains('\u{1b}'), "no ANSI in tool output");

    // Composes with shell logic and the ToolDef commands.
    let (stdout, _, _) = run(
        &tool,
        r#"for id in a-1 a-2; do get_agent "$id" -f json; done; list_agents --limit 2"#,
    )
    .await?;
    println!("$ <loop over get_agent, then list_agents>\n{stdout}");
    assert_eq!(
        stdout,
        "agent=a-1 format=json\nagent=a-2 format=json\nlimit=2\n"
    );

    // `additionalProperties: false` makes a typo an error rather than a
    // silently dropped argument — the same rule the tool registry already
    // applies to structured calls.
    let (_, stderr, code) = run(&tool, "list_agents --limti 2").await?;
    println!("$ list_agents --limti 2\n{stderr}");
    assert_ne!(code, 0);
    assert!(stderr.contains("unknown flag"));

    // The logic-only filesystem posture is unchanged by the custom builtin.
    let (_, stderr, code) = run(&tool, "cat /etc/passwd").await?;
    println!("$ cat /etc/passwd\n{stderr}");
    assert_ne!(code, 0);

    println!("All assertions passed.");
    Ok(())
}
