# Script Analysis

`Bash::analyze` tells you what a script *refers to* before you run it: which
commands it invokes, with which arguments, which files it redirects to, and
which functions it defines. It parses; it does not execute.

It exists for one question hosts keep having to answer: **"the model wants to
run this — do I need to ask the user first?"**

**See also:**
- [Hooks](./hooks.md) — runtime interception, the enforcement counterpart
- [Threat Model](./threat-model.md) — what analysis can and cannot see
- [Custom Builtins](./custom_builtins.md) — the commands you expose to a model

## Quick Start

```rust
use bashkit::Bash;

# fn main() -> bashkit::Result<()> {
let bash = Bash::new();
let analysis = bash.analyze("cat notes.txt | grep -i todo > out.txt")?;

assert_eq!(analysis.command_names(), ["cat", "grep"]);
assert_eq!(analysis.redirects[0].path.as_deref(), Some("out.txt"));
assert!(analysis.redirects[0].mode.is_write());
assert!(!analysis.is_opaque());
# Ok(())
# }
```

## Analysis is advisory, not a boundary

Static analysis cannot see through dynamic dispatch, interpreter re-entry
(`eval`, `source`, `bash -c`), functions, or aliases. This API never reports
those as safe — it reports them as *unknown*:

```rust
use bashkit::Bash;

# fn main() -> bashkit::Result<()> {
let bash = Bash::new();

// The name is computed, so it is not statically known.
let analysis = bash.analyze("$(echo rm) -rf /data")?;
assert!(analysis.commands.iter().any(|c| c.name.is_none()));
assert!(analysis.has_dynamic_commands);
assert!(analysis.is_opaque());

// eval hides its payload entirely.
let analysis = bash.analyze(r#"eval "$payload""#)?;
assert!(analysis.has_interpreter_reentry);
assert!(analysis.is_opaque());
# Ok(())
# }
```

`is_opaque()` is true when the script hides work: a dynamic command name, an
interpreter re-entry (`eval`, `source`, `.`, a nested `bash`/`sh`), or a walk that hit the
node budget. **A permission check built
on an allowlist must consult it.** "No command outside my allowlist" plus "not
opaque" is a decision; "no command outside my allowlist" alone is a bypass.

Enforcement lives elsewhere and still applies: a command that is not a
registered builtin cannot run, [`NetworkAllowlist`](crate::NetworkAllowlist)
gates egress, the mount policy gates the host filesystem, and the
[`before_tool`](crate::BashBuilder::before_tool) hook fires with the **resolved**
command name at execution time. The pairing below is the recommended shape.

## Use cases

### 1. Approve-before-run prompts

The common agent loop: classify the script, run it if everything is on the
allowlist, otherwise ask the user.

```rust
use bashkit::{Bash, ScriptAnalysis};

const READ_ONLY: &[&str] = &["ls", "cat", "head", "grep", "wc", "echo", "pwd"];

enum Decision {
    Allow,
    Ask(String),
}

fn classify(analysis: &ScriptAnalysis) -> Decision {
    if analysis.is_opaque() {
        return Decision::Ask("script builds commands dynamically".into());
    }
    for command in &analysis.commands {
        let Some(name) = command.name.as_deref() else {
            return Decision::Ask("unknown command".into());
        };
        if !READ_ONLY.contains(&name) {
            return Decision::Ask(format!("`{name}` may modify state"));
        }
    }
    if let Some(write) = analysis.redirects.iter().find(|r| r.mode.is_write()) {
        let path = write.path.as_deref().unwrap_or("a computed path");
        return Decision::Ask(format!("writes to {path}"));
    }
    Decision::Allow
}

# fn main() -> bashkit::Result<()> {
let bash = Bash::new();
assert!(matches!(classify(&bash.analyze("cat a | grep b")?), Decision::Allow));
assert!(matches!(classify(&bash.analyze("rm -rf /data")?), Decision::Ask(_)));
assert!(matches!(classify(&bash.analyze("echo x > f")?), Decision::Ask(_)));
assert!(matches!(classify(&bash.analyze("$c")?), Decision::Ask(_)));
# Ok(())
# }
```

### 2. Fine-grained permission keys for your own builtin

If you expose a domain CLI as a custom builtin, the interesting permission is
not "may run `mydata`" but "may run `mydata record delete`". Derive the key
from the analyzed arguments:

```rust
use bashkit::Bash;

fn permission_key(analysis: &bashkit::ScriptAnalysis) -> Option<String> {
    let command = analysis.commands_named("mydata").next()?;
    let resource = command.args.first()?.as_deref()?;
    let action = command.args.get(1)?.as_deref()?;
    match action {
        "list" | "get" | "query" => None, // read-only, no prompt
        _ => Some(format!("mydata:{resource}:{action}")),
    }
}

# fn main() -> bashkit::Result<()> {
let bash = Bash::new();
assert_eq!(permission_key(&bash.analyze("mydata record query 1")?), None);
assert_eq!(
    permission_key(&bash.analyze("mydata record delete 1")?).as_deref(),
    Some("mydata:record:delete"),
);
# Ok(())
# }
```

### 3. Audit log before execution

Record intent, not just outcome — useful when a script fails halfway and you
need to know what it was going to do.

```rust
use bashkit::Bash;

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::new();
let script = "echo start; echo done > /tmp/log";

let analysis = bash.analyze(script)?;
let intent = format!(
    "commands={:?} writes={:?}",
    analysis.command_names(),
    analysis
        .redirects
        .iter()
        .filter(|r| r.mode.is_write())
        .map(|r| r.path.clone())
        .collect::<Vec<_>>(),
);
assert_eq!(intent, r#"commands=["echo"] writes=[Some("/tmp/log")]"#);

let result = bash.exec(script).await?;
assert_eq!(result.exit_code, 0);
# Ok(())
# }
```

### 4. Analysis for the prompt, hook for the boundary

Analysis decides *whether to ask*; the hook decides *whether it runs*. The hook
sees the resolved name, so it catches what static analysis cannot.

```rust
use bashkit::{Bash, hooks::{HookAction, ToolEvent}};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder()
    .before_tool(Box::new(|event: ToolEvent| {
        if event.name == "rm" {
            return HookAction::Cancel("rm is not permitted".into());
        }
        HookAction::Continue(event)
    }))
    .build();

// Static analysis cannot resolve this name...
let analysis = bash.analyze("c=rm; $c -rf /data")?;
assert!(analysis.has_dynamic_commands);

// ...but the hook sees `rm` when it actually dispatches.
let result = bash.exec("c=rm; $c -rf /data").await?;
assert_ne!(result.exit_code, 0);
# Ok(())
# }
```

## What you get back

[`ScriptAnalysis`](crate::ScriptAnalysis):

| Field | Meaning |
|---|---|
| `commands` | Every simple command, in source order |
| `redirects` | Every file redirect target, in source order |
| `functions` | Function names the script defines |
| `has_dynamic_commands` | Some command name is not statically known |
| `has_command_substitution` | Contains `$(…)`, backticks, or `<(…)` |
| `has_interpreter_reentry` | Hands a script to the interpreter: `eval`, `source`, `.`, nested `bash`/`sh` |
| `truncated` | Node budget hit; lists are incomplete |

[`AnalyzedCommand`](crate::AnalyzedCommand) carries `name`, `args`, `context`,
and `assignments`. Both `name` and each entry of `args` are `Option<String>`:
`Some` only when the word is **fully literal**.

```rust
use bashkit::Bash;

# fn main() -> bashkit::Result<()> {
let bash = Bash::new();
let analysis = bash.analyze(r#"rm "$target" /tmp/fixed"#)?;
let command = &analysis.commands[0];

assert_eq!(command.name.as_deref(), Some("rm"));
assert_eq!(command.args[0], None);                     // "$target" — unknown
assert_eq!(command.args[1].as_deref(), Some("/tmp/fixed"));
assert_eq!(command.literal_args(), None);              // not all args are literal
# Ok(())
# }
```

Partial reconstruction is deliberately not offered: `"/tmp/$name.txt"` reports
`None`, not `"/tmp/"`. A half-expanded path reads like a path but is not one.

## Contexts

Commands inside substitutions and function bodies are walked, not skipped —
`echo $(rm -rf /)` must never look like a bare `echo`. Each command carries a
[`CommandContext`](crate::CommandContext) so you can tell them apart:

```rust
use bashkit::{Bash, CommandContext};

# fn main() -> bashkit::Result<()> {
let bash = Bash::new();
let analysis = bash.analyze("cleanup() { rm -rf /data; }\necho $(date)")?;

assert_eq!(analysis.functions, ["cleanup"]);
assert_eq!(analysis.commands[0].name.as_deref(), Some("rm"));
assert_eq!(analysis.commands[0].context, CommandContext::FunctionBody);
assert_eq!(analysis.commands[1].context, CommandContext::Substitution); // date
assert_eq!(analysis.commands[2].context, CommandContext::Direct);       // echo
# Ok(())
# }
```

- `Direct` — runs when the script runs (possibly inside an `if` or loop)
- `Substitution` — inside `$(…)`, backticks, or `<(…)`
- `FunctionBody` — runs only if the function is called

A host that wants "what happens now" filters to `Direct`; one that wants
"anything this script could do" uses all of them. Note that a function body can
rebind a name you consider safe, which is one more reason `is_opaque()` and the
`before_tool` backstop matter.

One gap to own: wrapper commands that run *other* commands named in their
arguments — `xargs`, `env`, `timeout`, `find -exec` — are not flagged. They
analyze as ordinary commands, so if you allowlist one, treat its arguments as
commands yourself.

## Redirects

`>`, `>|`, and `&>` report `Write`; `>>` reports `Append`; `<` reports `Read`.
Fd duplications (`2>&1`), here-documents, and here-strings name no file and are
omitted.

```rust
use bashkit::Bash;

# fn main() -> bashkit::Result<()> {
let bash = Bash::new();
let analysis = bash.analyze("cat < in >> log 2>&1")?;
assert_eq!(analysis.redirects.len(), 2);
assert!(analysis.redirects[1].mode.is_write());
# Ok(())
# }
```

## Errors and limits

A script that does not parse returns an error. Do not treat that as an empty
analysis — an unparseable script is not a script with no commands:

```rust
use bashkit::Bash;

# fn main() {
let bash = Bash::new();
assert!(bash.analyze("if true; then").is_err());
# }
```

`Bash::analyze` uses the instance's parser limits, so a script that would fail
to parse at execution time also fails to analyze. The walk records at most
[`MAX_ANALYSIS_NODES`](crate::analysis::MAX_ANALYSIS_NODES) (4096) commands plus
redirects and sets `truncated` beyond that; `is_opaque()` includes `truncated`
for exactly that reason.

For analysis without a `Bash` instance, use
[`analysis::analyze`](crate::analysis::analyze) or
[`analysis::analyze_with_limits`](crate::analysis::analyze_with_limits).

## Other languages

Node:

```javascript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();
const analysis = bash.analyze("cat notes.txt | grep -i todo");
// { commands: [{ name: "cat", args: [...], context: "direct", assignments: [] }, ...],
//   redirects: [], functions: [], hasDynamicCommands: false, ... }
```

Python:

```python
from bashkit import Bash

bash = Bash()
analysis = bash.analyze("cat notes.txt | grep -i todo")
assert [c.name for c in analysis.commands] == ["cat", "grep"]
assert not analysis.is_opaque
```
