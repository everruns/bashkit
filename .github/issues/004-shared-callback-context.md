---
title: "feat(scripted_tool): shared context across tool callbacks"
labels: ["enhancement", "scripted-tool"]
---

## Summary

Add a mechanism for ScriptedTool callbacks to share state (HTTP clients, auth tokens, database connections) without each callback capturing its own copy.

## Motivation

Cloudflare's Code Mode MCP injects an authenticated `cloudflare.request()` function. The LLM never sees raw API keys — auth is handled by the runtime.

ScriptedTool already supports this via closure captures and `.env()`, but there's no standardized pattern. Each callback must independently capture shared resources, leading to boilerplate and Arc-juggling.

## Design options (need discussion)

### Option A: TypeMap context

```rust
let client = Arc::new(build_authenticated_client());

ScriptedTool::builder("api")
    .context(client.clone())   // registers Arc<ReqwestClient> by type
    .tool(
        ToolDef::new("get_user", "..."),
        |args: &ToolArgs, ctx: &Context| {
            let client = ctx.get::<ReqwestClient>()?;
            // ...
        },
    )
    .build()
```

Pros: Type-safe, no string keys, familiar pattern (actix-web, axum).
Cons: Changes `ToolCallback` signature (breaking change to public API).

### Option B: Named context with Any

```rust
ScriptedTool::builder("api")
    .context("http_client", Arc::new(client) as Arc<dyn Any + Send + Sync>)
    .tool(
        ToolDef::new("get_user", "..."),
        |args: &ToolArgs, ctx: &Context| {
            let client = ctx.get::<ReqwestClient>("http_client")?;
            // ...
        },
    )
    .build()
```

Pros: Named, flexible.
Cons: Runtime type errors, string keys.

### Option C: Keep closures, document the pattern

Don't change the API. Instead, document the recommended pattern:

```rust
let client = Arc::new(build_authenticated_client());

let c = client.clone();
builder.tool(ToolDef::new("get_user", "..."), move |args| {
    let resp = c.get(&format!("/users/{}", args.param_i64("id").unwrap())).send()?;
    Ok(resp.text()?)
});
```

Pros: No API change, zero overhead.
Cons: Boilerplate with many tools, each needing `let c = client.clone();`.

### Recommendation

Start with **Option C** (document the pattern) and revisit Option A if users hit pain points. The `ToolCallback` signature is public API — changing it requires a major version bump per spec 009.

## Acceptance criteria

- [ ] Decide on approach (A, B, or C)
- [ ] If A or B: implement Context type, update ToolCallback signature, update spec 009 (breaking change)
- [ ] If C: add documented example showing shared auth client pattern
- [ ] Tests demonstrating shared state across multiple tool callbacks
- [ ] Update spec 014

## Effort

Small (Option C) to Medium (Option A/B).
