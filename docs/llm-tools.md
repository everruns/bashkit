# Bashkit as an LLM Tool

`BashTool` wraps the Bashkit sandbox as a ready-made tool for agent frameworks.
It exposes everything a model needs to call a shell safely: discovery metadata
(name, description, input schema), a system prompt describing the sandbox,
streaming output, and execution that runs against the same in-process virtual
filesystem and resource limits as the rest of Bashkit.

Use this when you want an agent or LLM app to run shell commands without giving
it a real host shell.

## Rust tool contract

```rust
use bashkit::{BashTool, Tool};
use futures_util::StreamExt;

# #[tokio::main]
# async fn main() -> anyhow::Result<()> {
let tool = BashTool::builder()
    .username("agent")
    .hostname("sandbox")
    .build();

println!("{}", tool.description());
println!("{}", tool.system_prompt());

let execution = tool.execution(serde_json::json!({
    "commands": "printf 'hello\nworld\n'"
}))?;
let mut stream = execution.output_stream().expect("stream available");

let handle = tokio::spawn(async move { execution.execute().await });
while let Some(chunk) = stream.next().await {
    println!("{}: {}", chunk.kind, chunk.data);
}

let output = handle.await??;
assert_eq!(output.result["stdout"], "hello\nworld\n");
# Ok(())
# }
```

The `Tool` trait gives you the pieces a framework needs: `description()` and
`system_prompt()` for prompting, an input schema for tool-call validation, and
streaming `chunk.kind` / `chunk.data` events while the command runs.

## Python

```python
from bashkit import BashTool

tool = BashTool()
print(tool.input_schema())
print(tool.description())
print(tool.system_prompt())

result = tool.execute_sync("echo 'Hello!'")
print(result.stdout)
```

The Python package ships optional extras with adapters for popular frameworks:

```bash
pip install 'bashkit[langchain]'
pip install 'bashkit[pydantic-ai]'
pip install 'bashkit[deepagents]'
```

## TypeScript / JavaScript

```typescript
import { BashTool } from "@everruns/bashkit";

const tool = new BashTool();
console.log(tool.description());
console.log(tool.systemPrompt());

const result = await tool.execute("echo 'Hello!'");
console.log(result.stdout);
```

Framework adapters are exported as subpaths of `@everruns/bashkit`:

- OpenAI adapter: `@everruns/bashkit/openai`
- Anthropic adapter: `@everruns/bashkit/anthropic`
- Vercel AI SDK adapter: `@everruns/bashkit/ai`
- LangChain adapter: `@everruns/bashkit/langchain`

## Sandbox guarantees

A `BashTool` runs in the same sandbox as the core interpreter, so tool calls
inherit Bashkit's safety model: an in-memory virtual filesystem, resource
limits on commands and output, and a default-deny network allowlist. The model
cannot escape to the host unless you explicitly mount a real filesystem or
allow a domain.

## Next steps

- [Hooks](hooks.md) — observe, rewrite, or cancel tool calls and HTTP requests.
- [Security](security.md) — the sandbox boundaries every tool call runs inside.
- Examples: [agent and tool flows](https://github.com/everruns/bashkit/tree/main/examples).
- Full API reference: [docs.rs/bashkit](https://docs.rs/bashkit/latest/bashkit/).
