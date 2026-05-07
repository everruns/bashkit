# LLM Tools and Agent Integrations

Use this when the user wants Bashkit as a tool runtime for an agent or LLM app.

## Rust Tool Contract

```rust
use bashkit::{BashTool, Tool};
use futures::StreamExt;

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

Optional integrations:

```python
pip install 'bashkit[langchain]'
pip install 'bashkit[pydantic-ai]'
pip install 'bashkit[deepagents]'
```

## JavaScript/TypeScript

Use `BashTool` or the framework adapters exported by `@everruns/bashkit`.

```typescript
import { BashTool } from "@everruns/bashkit";

const tool = new BashTool();
console.log(tool.description());
console.log(tool.systemPrompt());

const result = await tool.execute("echo 'Hello!'");
console.log(result.stdout);
```

Check current package docs for adapter names:

- OpenAI adapter: `@everruns/bashkit/openai`
- Anthropic adapter: `@everruns/bashkit/anthropic`
- Vercel AI SDK adapter: `@everruns/bashkit/ai`
- LangChain adapter: `@everruns/bashkit/langchain`


## Reference

- Rust API docs: https://docs.rs/bashkit/latest/bashkit/
- Python examples: https://github.com/everruns/bashkit/tree/main/examples
- JavaScript examples: https://github.com/everruns/bashkit/tree/main/examples
- CLI docs: https://github.com/everruns/bashkit/blob/main/docs/cli.md
