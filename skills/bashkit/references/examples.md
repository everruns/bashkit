# Bashkit Examples

Use these as starting points. Adjust paths, package managers, and feature flags for the user's environment.

## CLI

```bash
bashkit -c 'echo "hello world" | tr a-z A-Z'
```

```bash
bashkit -c '
mkdir -p /repo
cd /repo
echo "# demo" > README.md
git init
git add README.md
git commit -m "init"
git log --oneline
'
```

```bash
bashkit -c "sqlite :memory: 'SELECT 1 + 2'"
```

## Rust

```rust
use bashkit::Bash;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut bash = Bash::new();
    bash.exec("printf 'name,score\nada,42\n' > /tmp/scores.csv").await?;
    let result = bash.exec("tail -n +2 /tmp/scores.csv | cut -d, -f1").await?;
    print!("{}", result.stdout);
    Ok(())
}
```

## Python

```python
from bashkit import Bash

bash = Bash()
bash.execute_sync("printf 'name,score\\nada,42\\n' > /tmp/scores.csv")
result = bash.execute_sync("tail -n +2 /tmp/scores.csv | cut -d, -f1")
print(result.stdout)
```

## JavaScript/TypeScript

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();
bash.executeSync("printf 'name,score\\nada,42\\n' > /tmp/scores.csv");
const result = bash.executeSync("tail -n +2 /tmp/scores.csv | cut -d, -f1");
console.log(result.stdout);
```

## Repository Examples

- CLI and app examples: https://github.com/everruns/bashkit/tree/main/examples
- Rust examples: https://github.com/everruns/bashkit/tree/main/crates/bashkit/examples
- Browser/WASM example: https://github.com/everruns/bashkit/tree/main/examples/browser
- Bashkit PI example: https://github.com/everruns/bashkit/tree/main/examples/bashkit-pi
