---
title: "feat(mcp): expose ScriptedTool as MCP tool"
labels: ["enhancement", "mcp", "scripted-tool"]
---

## Summary

Integrate ScriptedTool with the existing MCP server so any `ScriptedTool` instance can be served over MCP's JSON-RPC protocol. Each ScriptedTool becomes one MCP tool (not one per sub-tool), preserving the code-mode benefit.

## Motivation

The current MCP server (`crates/bashkit-cli/src/mcp.rs`) exposes a single generic `bash` tool. It has no awareness of ScriptedTool. Cloudflare's [Code Mode MCP](https://blog.cloudflare.com/code-mode-mcp/) showed that collapsing N API endpoints into one code-accepting MCP tool dramatically reduces token cost and round-trips.

By exposing ScriptedTool over MCP, any MCP client (Claude Desktop, VS Code Copilot, Cursor, etc.) gets access to a sandboxed code-mode tool without custom integration work.

## Design

### MCP tools/list

Each registered ScriptedTool appears as one MCP tool:

```json
{
  "tools": [
    {
      "name": "ecommerce_api",
      "description": "E-commerce API orchestrator. Write bash scripts using: get_user, list_orders, get_inventory, create_discount. Use `help <cmd>` for details.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "commands": { "type": "string", "description": "Bash script using tool commands" }
        },
        "required": ["commands"]
      }
    }
  ]
}
```

### MCP tools/call

Routes to `ScriptedTool::execute()`:

```json
{"method": "tools/call", "params": {"name": "ecommerce_api", "arguments": {"commands": "get_user --id 42 | jq '.name'"}}}
```

### Implementation

1. Add `McpServer::register_scripted_tool(tool: ScriptedTool)` method
2. `tools/list` iterates registered ScriptedTools, using `Tool::input_schema()` and `Tool::short_description()`
3. `tools/call` dispatches by tool name, calls `ScriptedTool::execute()`
4. Keep existing `bash` tool for backward compat
5. Gate behind `scripted_tool` feature flag

### Architecture

```
McpServer
├── bash tool (existing, always available)
├── ecommerce_api (ScriptedTool)
├── analytics_api (ScriptedTool)
└── ...
```

### CLI

```bash
bashkit mcp --tool ecommerce.sh   # Load ScriptedTool from config/script
bashkit mcp                        # Just the generic bash tool (current behavior)
```

## Acceptance criteria

- [ ] `tools/list` includes registered ScriptedTools
- [ ] `tools/call` routes to correct ScriptedTool and returns stdout/stderr/exit_code
- [ ] Error handling: unknown tool name → JSON-RPC error
- [ ] Existing `bash` tool unaffected
- [ ] Integration test: full JSON-RPC round-trip (initialize → tools/list → tools/call)
- [ ] Update spec 014 with MCP section

## Effort

Medium — plumbing between existing MCP server and existing ScriptedTool. Main work is the routing and tool registration.
