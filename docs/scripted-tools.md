# Scripted tool orchestration

Give an LLM ten tools and a ten-step task, and you pay for ten round-trips —
each call is a separate turn, with the model re-reading context every time.
`ScriptedTool` collapses that into one call: the model writes a single bash
script that invokes your tools, pipes their output through `jq`, loops, branches,
and returns one composed result.

Each tool you register becomes a **builtin command** inside a locked-down bash
interpreter. The LLM orchestrates them with the full shell grammar it already
knows — variables, pipelines, `for`, `if` — instead of a sequence of isolated
tool calls.

<svg viewBox="0 0 720 196" role="img" aria-label="One LLM call runs a bash script that invokes multiple registered tools" xmlns="http://www.w3.org/2000/svg" style="max-width:100%;height:auto;margin:1rem 0;">
  <rect x="0.5" y="0.5" width="719" height="195" fill="#ffffff" stroke="#0a1636" stroke-opacity="0.12"/>
  <g font-family="ui-monospace,monospace" font-size="13" fill="#0a1636">
    <rect x="24" y="74" width="120" height="48" rx="4" fill="#f5f5f5" stroke="#0a1636" stroke-opacity="0.3"/>
    <text x="84" y="94" text-anchor="middle">LLM</text>
    <text x="84" y="111" text-anchor="middle" fill="#404040" font-size="11">one call</text>

    <rect x="214" y="58" width="170" height="80" rx="4" fill="#fff" stroke="#d4a43a" stroke-width="1.5"/>
    <text x="299" y="84" text-anchor="middle">bash script</text>
    <text x="299" y="104" text-anchor="middle" fill="#404040" font-size="11">pipes · vars · loops</text>
    <text x="299" y="121" text-anchor="middle" fill="#404040" font-size="11">logic-only shell</text>

    <g font-size="12">
      <rect x="468" y="20" width="228" height="34" rx="4" fill="#f5f5f5" stroke="#0a1636" stroke-opacity="0.3"/>
      <text x="482" y="42" fill="#0a1636">get_user --id 1  →  callback</text>
      <rect x="468" y="80" width="228" height="34" rx="4" fill="#f5f5f5" stroke="#0a1636" stroke-opacity="0.3"/>
      <text x="482" y="102" fill="#0a1636">list_orders --user_id 1</text>
      <rect x="468" y="140" width="228" height="34" rx="4" fill="#f5f5f5" stroke="#0a1636" stroke-opacity="0.3"/>
      <text x="482" y="162" fill="#0a1636">create_discount --pct 10</text>
    </g>

    <g stroke="#0a1636" stroke-opacity="0.5" fill="none">
      <path d="M144 98 H214" marker-end="url(#ar3)"/>
      <path d="M384 92 C 420 92, 430 37, 468 37" marker-end="url(#ar3)"/>
      <path d="M384 98 H468 L468 97" marker-end="url(#ar3)"/>
      <path d="M384 104 C 420 104, 430 157, 468 157" marker-end="url(#ar3)"/>
    </g>
  </g>
  <defs>
    <marker id="ar3" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="#0a1636" fill-opacity="0.5"/>
    </marker>
  </defs>
</svg>

## Building one

A tool is a `ToolDef` (name, description, JSON-Schema input) paired with a
callback that returns stdout on success or an error string on failure:

```rust,no_run
use bashkit::{ScriptedTool, ToolArgs, ToolDef, Tool};

fn get_user(args: &ToolArgs) -> Result<String, String> {
    let id = args.param_i64("id").ok_or("missing --id")?;
    Ok(format!(r#"{{"id":{id},"name":"Ada","tier":"gold"}}"#))
}

# #[tokio::main]
# async fn main() -> anyhow::Result<()> {
let def = ToolDef::new("get_user", "Fetch user by ID").with_schema(serde_json::json!({
    "type": "object",
    "properties": { "id": {"type": "integer", "description": "User ID"} },
    "required": ["id"],
}));

let tool = ScriptedTool::builder("ecommerce_api")
    .short_description("User, order, and inventory tools")
    .tool_fn(def, get_user)
    .build();

// The LLM sends one script; tools compose with pipes and jq.
let out = tool
    .execution(serde_json::json!({ "commands": "get_user --id 1 | jq -r '.name'" }))?
    .execute()
    .await?;
assert_eq!(out.result["stdout"], "Ada\n");
# Ok(())
# }
```

Flags parse from the schema: `--id 1` becomes `{"id": 1}` (coerced per the
schema's property types). Use `.async_tool_fn(def, cb)` for async callbacks —
sync and async tools mix freely in one `ScriptedTool`. The full e-commerce demo
lives in
[`examples/scripted_tool.rs`](https://github.com/everruns/bashkit/blob/main/crates/bashkit/examples/scripted_tool.rs).

## Code mode, not a file shell

`ScriptedTool` always runs in **logic mode**: bash is the control-flow and
data-transformation language, not a [virtual filesystem](filesystem.md) shell.
This is a deliberate, narrower sandbox than [`BashTool`](llm-tools.md).

| Kept | Rejected |
|------|----------|
| variables, arrays, functions, arithmetic | file commands (`cat`, `ls`, `cp`, `rm`, `mkdir`, …) |
| `if` / `case` / `for` / `while` | path execution (`/tmp/x.sh`, `$PATH` lookup) |
| pipelines, heredocs, command substitution | file redirection (`>`, `>>`, `<`) except `/dev/null` |
| your tool commands + `help` + `discover` | process substitution |
| stdin transforms: `jq`, `grep`, `sed`, `awk`, `sort`, `cut`, `tr`, `wc`, `head`, `tail`, `seq`, `expr` | |

Reach for [`BashTool`](llm-tools.md) instead when a virtual filesystem is part of
the task.

## Runtime discovery

The LLM doesn't need every schema in its context up front. Two built-in commands
let it explore at runtime:

- `help --list`, `help <tool>`, `help <tool> --json` — names, usage, and
  machine-readable schemas (enum values, required fields).
- `discover --categories | --category X | --tag Y | --search text` — filter by
  the `tags` / `category` you set on each `ToolDef`.

For large tool sets, `ScriptedToolBuilder::compact_prompt(true)` shrinks the
system prompt to names + one-liners and defers full schemas to `help`.

`ScriptingToolSet` formalises this: in `WithDiscovery` mode it exposes a compact
script tool **plus** a companion discover tool, so the model browses schemas
before writing a script — ideal alongside other tools or for 50+ tool sets.

## Safety

`ScriptedTool` inherits every sandbox guarantee (resource limits, no network
unless configured) and adds a disabled filesystem backend and reduced builtin
surface. Each `execute()` gets a **fresh** interpreter, so there is no state
bleed between calls; persistence is your callbacks' concern (capture an `Arc`).
Callback error strings are sanitised by default so host-side secrets, paths, and
stack traces never reach script-visible stderr.

## See also

- [Bashkit as an LLM tool](llm-tools.md) — the filesystem-backed `BashTool`.
- [Virtual filesystem](filesystem.md) — why logic mode disables file access.
- Spec: [`specs/scripted-tool-orchestration.md`](https://github.com/everruns/bashkit/blob/main/specs/scripted-tool-orchestration.md).
