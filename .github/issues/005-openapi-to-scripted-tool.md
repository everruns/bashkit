---
title: "RFC: OpenAPI spec → ScriptedTool auto-generation"
labels: ["enhancement", "scripted-tool", "rfc"]
---

## Summary

Automatically generate a ScriptedTool from an OpenAPI specification, where each API endpoint becomes a sub-tool with an HTTP callback. This is the "zero-code setup" equivalent of Cloudflare's Code Mode MCP approach.

## Motivation

Cloudflare loads an OpenAPI spec at startup and generates tools from it. Currently ScriptedTool requires compile-time registration of every tool + callback. For teams with existing OpenAPI specs, this is unnecessary friction.

With auto-generation: **any OpenAPI spec → one sandboxed code-mode MCP tool**.

## Sketch (not a design yet)

```rust
// Hypothetical API
let tool = ScriptedTool::from_openapi(
    include_str!("openapi.yaml"),
    OpenApiConfig {
        base_url: "https://api.example.com",
        auth: Auth::Bearer("...".into()),
        timeout: Duration::from_secs(30),
    },
).build();

// Generates tools like:
// GET /users/{id}     → get_users_by_id --id <integer>
// POST /orders        → post_orders --body <json>
// DELETE /users/{id}  → delete_users_by_id --id <integer>
```

## Open questions

1. **Naming convention:** How to derive bash-friendly command names from `operationId` or path? (`GET /users/{id}` → `get_user`? `users_get`? Configurable?)
2. **Request body:** How to pass JSON bodies from bash? `--body '{"name":"Alice"}'`? Stdin?
3. **Auth patterns:** Bearer, API key, OAuth2 — which to support initially?
4. **Response mapping:** Return raw JSON? Status code handling?
5. **Async callbacks:** Current `ToolCallback` is sync (`Fn(&ToolArgs) -> Result<String, String>`). HTTP calls are async. Need `AsyncToolCallback`?
6. **Dependencies:** Would need `reqwest` (or similar) + OpenAPI parser crate. Evaluate: `oas3`, `openapiv3`, `openapi`. Which is maintained and lightweight?
7. **Feature flag:** Separate feature `openapi` to avoid pulling HTTP deps into core.
8. **Scope:** Separate crate (`bashkit-openapi`) or feature in main crate?

## Prior art

- Cloudflare Code Mode MCP (JS, Workers runtime)
- [openapi-mcp](https://github.com/modelcontextprotocol/servers/tree/main/src/openapi) — generic OpenAPI → MCP server
- Swagger Codegen / OpenAPI Generator — but generates full SDKs, not tool definitions

## Next steps

This is an **RFC / thinking issue**. Before implementation:

1. Prototype with a small OpenAPI spec (e.g., Petstore) to validate naming + body handling
2. Evaluate OpenAPI parser crates for correctness and maintenance
3. Decide sync vs async callback approach
4. Decide crate structure

## Effort

Large — new crate or large feature. HTTP client, OpenAPI parsing, naming heuristics, auth.
