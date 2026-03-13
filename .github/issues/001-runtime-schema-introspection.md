---
title: "feat(scripted_tool): built-in `help <tool> --json` for runtime schema introspection"
labels: ["enhancement", "scripted-tool"]
---

## Summary

Add a built-in `help` command to ScriptedTool that lets LLMs query tool schemas at runtime, instead of requiring all schema details in the upfront `system_prompt()`.

## Motivation

Inspired by [Cloudflare Code Mode MCP](https://blog.cloudflare.com/code-mode-mcp/): their `search()` tool lets LLMs explore API specs at runtime rather than loading everything into context. Currently `system_prompt()` emits full schemas for all registered tools. For small sets (4-10) this is fine; for large sets (50+) it blows up the context window.

Runtime introspection lets `system_prompt()` emit only tool names + one-liners, deferring full schemas to on-demand queries within the bash script.

## Design

### Built-in `help` command

Registered automatically alongside user tools in `ToolBuiltinAdapter`:

```bash
# Human-readable (default)
help get_user
# get_user - Fetch user by ID
# Usage: get_user --id <integer> [--include-orders <boolean>]
# ...

# Machine-readable JSON (for jq pipelines)
help get_user --json
# {"name":"get_user","description":"Fetch user by ID","input_schema":{"type":"object","properties":{"id":{"type":"integer"},...}}}

# List all tools
help --list
# get_user     Fetch user by ID
# list_orders  List orders for user
# ...

# Discover enum values
help get_user --json | jq '.input_schema.properties.role.enum'
# ["admin", "user", "guest"]
```

### Implementation

1. Add a `HelpBuiltin` struct in `execute.rs` that holds `Vec<RegisteredTool>` (clone of tool defs)
2. Register it as builtin `help` alongside user tools during `execute()`
3. Flag parsing: `--json` for JSON output, `--list` for summary, bare name for man-page style
4. `system_prompt()` gains a new tip: "Use `help <tool> --json` for full parameter details"

### system_prompt() changes

For tools with schemas, `system_prompt()` can optionally emit a compact form:

```markdown
## Available tool commands

- `get_user`: Fetch user by ID (use `help get_user` for params)
- `list_orders`: List orders for user (use `help list_orders` for params)
```

Add a `ScriptedToolBuilder::compact_prompt(bool)` option to control this. Default: `false` (backward compat).

## Acceptance criteria

- [ ] `help <tool>` prints human-readable usage
- [ ] `help <tool> --json` prints JSON schema to stdout (pipeable to jq)
- [ ] `help --list` prints all tool names + descriptions
- [ ] `compact_prompt(true)` omits full schemas from `system_prompt()`, adds help tip
- [ ] Tests for all modes
- [ ] Update spec 014

## Effort

Small — schema data already exists in `ToolDef.input_schema`. Just needs a new builtin.
