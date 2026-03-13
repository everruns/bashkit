---
title: "feat(scripted_tool): optional persistent state across execute() calls"
labels: ["enhancement", "scripted-tool"]
---

## Summary

Explore whether ScriptedTool should support optional state persistence across multiple `execute()` calls, allowing multi-turn workflows where later scripts build on earlier results.

## Context

Currently each `execute()` creates a fresh `Bash` interpreter — no state carries over. This is a security feature (clean sandbox per call). But it means multi-turn workflows require the LLM to re-fetch data it already retrieved in a prior call.

Cloudflare's two-tool pattern (`search()` then `execute()`) implicitly creates cross-call state: the LLM learns from `search()` what to pass to `execute()`. ScriptedTool's single-call model is actually more efficient for single-turn workflows, but lacks a multi-turn story.

## Can we do this?

**Yes, with careful scoping.** The key question is what "state" means:

### Option A: Persistent VFS

Keep the virtual filesystem across calls. Scripts can write files in one call and read them in the next.

```bash
# Call 1
users=$(get_user --id 1)
echo "$users" > /tmp/user_cache.json

# Call 2 (separate execute() call)
cat /tmp/user_cache.json | jq '.name'  # Works because VFS persists
```

Implementation: Store `Vfs` on `ScriptedTool` struct, pass to each new `Bash` instance.

Pros: Natural bash idiom (files as state). Simple mental model.
Cons: State can grow unbounded. Security: scripts from different "turns" share a filesystem.

### Option B: Key-value store via builtins

Add `state_set` / `state_get` builtins backed by a `HashMap<String, String>` on `ScriptedTool`:

```bash
# Call 1
state_set user_name "Alice"

# Call 2
name=$(state_get user_name)
echo "Hello, $name"
```

Pros: Explicit, auditable, bounded.
Cons: New API surface. Not standard bash.

### Option C: Environment variable carry-over

Persist exported environment variables across calls:

```bash
# Call 1
export CACHED_USER='{"id":1,"name":"Alice"}'

# Call 2
echo "$CACHED_USER" | jq '.name'
```

Pros: Standard bash pattern.
Cons: Env vars are strings only. Size limits.

### Option D: Don't — let the LLM handle it

The LLM already sees stdout from each call. It can pass relevant data from one call's output into the next call's script. This is what Cloudflare does (LLM carries state in context).

Pros: Zero implementation. No security concerns.
Cons: Token cost (LLM must include data in prompts). Works less well for large intermediate results.

## Recommendation

**Start with Option D (status quo) + document the pattern.** The LLM-as-state-carrier approach works well for most cases and avoids security complexity.

If real-world usage shows pain points (large intermediate results, excessive token cost), implement **Option A (persistent VFS)** behind `ScriptedToolBuilder::persistent(true)`:

```rust
ScriptedTool::builder("api")
    .persistent(true)  // VFS persists across execute() calls
    .build()
```

Default: `false` (current behavior, fresh interpreter per call).

## Acceptance criteria

- [ ] Document the LLM-as-state-carrier pattern in spec 014
- [ ] If implementing persistent VFS:
  - [ ] `ScriptedToolBuilder::persistent(bool)` flag
  - [ ] VFS stored on `ScriptedTool`, cloned into each `Bash` instance
  - [ ] Resource limits on VFS size to prevent unbounded growth
  - [ ] Tests: write in call 1, read in call 2
  - [ ] Security review: what are the implications of cross-call VFS?

## Effort

Small (document pattern) to Medium (persistent VFS implementation).
