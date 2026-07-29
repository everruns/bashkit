# Analyzing a script before running it

`analyze()` parses a script and reports what it *statically* refers to — the
commands it invokes, their arguments, the files it redirects to, and the
functions it defines. It never executes anything.

It exists for the question every agent host has to answer: **the model produced
this command — do I run it, or ask the user first?**

Available in all three packages:

| | Call |
|---|---|
| Rust | `bash.analyze(script)?` |
| Node | `bash.analyze(script)` |
| Python | `bash.analyze(script)` |

## The shape

```typescript
const analysis = bash.analyze("cat notes.txt | grep -i todo > out.txt");

analysis.commandNames;              // ["cat", "grep"]
analysis.commands[1].name;          // "grep"
analysis.commands[1].args;          // ["-i", "todo"]
analysis.commands[1].context;       // "direct"
analysis.redirects[0].path;         // "out.txt"
analysis.redirects[0].isWrite;      // true
analysis.isOpaque;                  // false
```

Every word is **literal-or-null**. A command name or argument is reported only
when it is fully literal; anything containing an expansion is `null`:

```python
analysis = bash.analyze('rm "$target" /tmp/fixed')
analysis.commands[0].name        # "rm"
analysis.commands[0].args[0]     # None  — "$target" is not knowable statically
analysis.commands[0].args[1]     # "/tmp/fixed"
```

Partial reconstruction is deliberately not offered: `"/tmp/$name.txt"` reports
`None`, not `"/tmp/"`. A half-expanded path reads like a path but is not one.

## Analysis is advisory, not a boundary

A script's real behavior is only knowable at runtime. Static analysis cannot see
through:

- dynamic dispatch — `$cmd`, `${arr[0]}`, `$(echo rm) -rf /`
- interpreter re-entry — `eval`, `source`, `.`, `bash -c '…'`
- functions and aliases that rebind a name
- arguments built from variables

The API reports all of these as **unknown**, never as safe, and rolls them into
one flag:

```typescript
bash.analyze("$(echo rm) -rf /data").isOpaque;  // true (dynamic name)
bash.analyze('eval "$payload"').isOpaque;       // true (eval)
bash.analyze("echo x;".repeat(5000)).isOpaque;  // true (walk truncated)
```

**If you gate on an allowlist, you must check `isOpaque`.** "Nothing outside my
allowlist appeared in `commands`" plus "not opaque" is a decision. Without the
second half it is a bypass — `c=rm; $c -rf /data` contains no disallowed
command name.

A script that does not parse raises/throws. Treat that as *deny or prompt*, not
as "no commands".

Enforcement stays where it always was: a command that is not a registered
builtin cannot run, the network allowlist gates egress, the mount policy gates
the host filesystem, and (in Rust) the `before_tool` hook fires with the
**resolved** command name at dispatch time. Analysis decides whether to ask; the
sandbox decides what can happen. See the
[threat model](./security.md) entry TM-ESC-032.

## Permission prompts

The common agent loop, in TypeScript:

```typescript
const READ_ONLY = new Set(["ls", "cat", "head", "grep", "wc", "echo", "pwd"]);

function needsApproval(script: string): string | null {
  let analysis;
  try {
    analysis = bash.analyze(script);
  } catch {
    return "script does not parse";
  }
  if (analysis.isOpaque) return "script builds commands dynamically";

  for (const command of analysis.commands) {
    if (command.isAssignmentOnly) continue;
    if (command.name == null) return "unknown command";
    if (!READ_ONLY.has(command.name)) return `\`${command.name}\` may modify state`;
  }
  const write = analysis.redirects.find((r) => r.isWrite);
  if (write) return `writes to ${write.path ?? "a computed path"}`;
  return null; // safe to run unattended
}
```

The same in Python:

```python
READ_ONLY = {"ls", "cat", "head", "grep", "wc", "echo", "pwd"}

def needs_approval(bash, script):
    try:
        analysis = bash.analyze(script)
    except BashError:
        return "script does not parse"
    if analysis.is_opaque:
        return "script builds commands dynamically"
    for command in analysis.commands:
        if command.is_assignment_only:
            continue
        if command.name is None:
            return "unknown command"
        if command.name not in READ_ONLY:
            return f"`{command.name}` may modify state"
    write = next((r for r in analysis.redirects if r.is_write), None)
    if write:
        return f"writes to {write.path or 'a computed path'}"
    return None
```

## Fine-grained keys for your own builtin

When you expose a domain CLI as a [custom builtin](./custom_builtins_js.md), the
useful permission is not "may run `mydata`" but "may run `mydata record
delete`". Derive it from the analyzed arguments:

```typescript
function permissionKey(script: string): string | null {
  const command = bash.analyze(script).commands.find((c) => c.name === "mydata");
  const [resource, action] = command?.args ?? [];
  if (resource == null || action == null) return null;   // computed → prompt
  if (["list", "get", "query"].includes(action)) return null;  // read-only
  return `mydata:${resource}:${action}`;
}

permissionKey("mydata record query 42");   // null
permissionKey("mydata record delete 42");  // "mydata:record:delete"
permissionKey("cat x | mydata doc write 7"); // "mydata:doc:write"
```

Custom builtins are ordinary command names to the parser, so they analyze like
anything else — no registration step.

## Contexts

Commands inside substitutions and function bodies are walked, not skipped:
`echo $(rm -rf /)` must never look like a bare `echo`. Each command says where
it sits:

```python
analysis = bash.analyze("cleanup() { rm -rf /data; }\necho $(date)")

analysis.functions               # ["cleanup"]
analysis.commands[0].context     # "function_body" — runs only if called
analysis.commands[1].context     # "substitution"  — the $(date)
analysis.commands[2].context     # "direct"        — the echo
```

Filter to `direct` for "what happens now"; use all of them for "anything this
script could do".

## Redirects

`>`, `>|`, and `&>` report `write`; `>>` reports `append`; `<` reports `read`.
Fd duplications (`2>&1`), here-documents, and here-strings name no file and are
omitted. `path` is `null` when the target is computed (`> $out`) — which, for a
mount-boundary check, means "ask".

## Known gap: wrapper commands

Commands that run *other* commands named in their arguments — `xargs`, `env`,
`timeout`, `find -exec`, `awk 'system(…)'` — analyze as ordinary commands and
are not flagged. If you allowlist one of them, treat its arguments as commands
yourself. `bash -c` and `sh -c` *are* flagged, via `hasEval` / `has_eval`.

## Audit logging

Recording intent before execution is useful on its own: when a script fails
halfway, the log still says what it was going to do.

```python
analysis = bash.analyze(script)
audit.record(
    commands=analysis.command_names,
    writes=[r.path for r in analysis.redirects if r.is_write],
    opaque=analysis.is_opaque,
)
result = bash.execute_sync(script)
```

## Limits

`analyze()` uses the instance's configured parser limits and input-size limit —
a script too large or too deep to execute is also too large or too deep to
analyze. The walk records at most 4096 commands plus redirects; past that,
`truncated` is set and `isOpaque` becomes true.

## See also

- [LLM tools](./llm-tools.md) — exposing the sandbox to an agent framework
- [Custom builtins (JS)](./custom_builtins_js.md) — the commands you expose
- [Security](./security.md) — sandbox boundaries and the threat model
