---
title: "feat(scripted_tool): `discover` command with tags, search, and categories"
labels: ["enhancement", "scripted-tool"]
depends_on: ["001-runtime-schema-introspection"]
---

## Summary

Add a built-in `discover` command and tagging system to ScriptedTool, enabling LLMs to progressively explore large tool sets without loading all documentation into context.

## Motivation

Cloudflare's Code Mode MCP handles ~1,300 API endpoints behind a single tool. The LLM uses `search()` to find relevant endpoints before calling them. Our `system_prompt()` approach works for small tool sets but doesn't scale to 50+ tools. Progressive discovery lets the LLM narrow down tools at runtime.

**Depends on:** #001 (runtime schema introspection via `help` builtin) — `discover` finds tools, `help` gives full details.

## Design

### ToolDef extensions

```rust
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub tags: Vec<String>,        // NEW: categorical tags
    pub category: Option<String>, // NEW: grouping category
}

impl ToolDef {
    pub fn with_tags(self, tags: &[&str]) -> Self;
    pub fn with_category(self, category: &str) -> Self;
}
```

### Built-in `discover` command

```bash
# List all categories
discover --categories
# payments (4 tools)
# users (3 tools)
# inventory (2 tools)

# List tools in a category
discover --category payments
# create_charge    Create a payment charge
# refund           Issue a refund
# list_charges     List charges for customer
# get_balance      Get account balance

# Search by keyword (fuzzy match on name + description)
discover --search "user"
# get_user         Fetch user by ID
# list_users       List users with filters
# update_user      Update user profile

# Filter by tag
discover --tag admin
# delete_user      Delete a user account (admin only)
# set_permissions  Set user permissions

# JSON output for programmatic use
discover --category payments --json
# [{"name":"create_charge","description":"...","tags":["payments","billing"]}]
```

### system_prompt() with discovery

When `compact_prompt(true)` is set (from #001):

```markdown
# ecommerce_api

Input: {"commands": "<bash script>"}

## Tool discovery

This tool has 47 sub-commands across 8 categories. Use these to explore:

- `discover --categories` — list all categories
- `discover --search <keyword>` — fuzzy search tools
- `discover --tag <tag>` — filter by tag
- `help <tool>` — full usage and parameters
- `help <tool> --json` — machine-readable schema

## Quick reference (most common)

- `get_user`, `list_orders`, `get_inventory` (use `help` for params)
```

### Implementation

1. Extend `ToolDef` with `tags: Vec<String>` and `category: Option<String>`
2. Add `DiscoverBuiltin` struct in `execute.rs`, holding tool metadata
3. Register as builtin `discover` alongside `help` and user tools
4. Fuzzy search: simple substring match on name + description (no extra deps)
5. `compact_prompt` mode emits category summary + discovery tips instead of full tool listing

## Acceptance criteria

- [ ] `ToolDef::with_tags()` and `ToolDef::with_category()` builder methods
- [ ] `discover --categories` lists categories with tool counts
- [ ] `discover --category <name>` lists tools in category
- [ ] `discover --search <keyword>` fuzzy-matches name + description
- [ ] `discover --tag <tag>` filters by tag
- [ ] `discover --json` for machine-readable output
- [ ] `compact_prompt(true)` uses discovery-based system prompt
- [ ] Tests for all discovery modes
- [ ] Update spec 014

## Effort

Medium — new builtin + ToolDef extensions. Fuzzy search is simple substring match, no external crate needed.
