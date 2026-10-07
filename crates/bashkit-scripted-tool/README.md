# bashkit-scripted-tool

ScriptedTool for [bashkit](https://crates.io/crates/bashkit): register
`ToolDef` + callback pairs and expose them to an LLM as one tool that accepts a
bash script. Each registered tool becomes a command in a logic-only shell (no
filesystem, no script execution), so a model can chain many calls in one turn
with pipes, variables, loops and `jq`.

```toml
[dependencies]
bashkit = "0.18"
bashkit-scripted-tool = { version = "0.18", features = ["jq"] }
```

```rust,ignore
use bashkit::Tool;
use bashkit_scripted_tool::{ScriptedTool, ToolArgs, ToolDef};

let tool = ScriptedTool::builder("api")
    .tool_fn(ToolDef::new("greet", "Greet a user"), |args: &ToolArgs| {
        Ok(format!("hello {}\n", args.param_str("name").unwrap_or("world")))
    })
    .build();
```

Features:

- `jq`: admit the `jq` builtin in the logic-only shell.
- `python`, `typescript`: expose a `ToolRegistry` to embedded Python and
  TypeScript as host functions (`ToolRegistry::install`).
- `tracing`: debug logs for sanitized callback errors.

This crate used to be the `scripted_tool` feature of `bashkit`. See the
[migration guide](https://github.com/everruns/bashkit/blob/main/docs/migrating-scripted-tool.md).
