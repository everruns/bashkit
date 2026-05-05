---
name: bashkit
description: Use when a user wants to run Bashkit, write scripts for Bashkit, use Bashkit as an LLM tool/runtime, call Bashkit from Rust/Python/JavaScript/TypeScript, use its CLI, understand supported builtins/languages, or find practical Bashkit examples, docs, packages, and resources.
---

# Bashkit

Bashkit is a virtual Bash interpreter for sandboxed, in-process shell execution.

## How to Use This Skill

Answer with runnable examples first. Prefer the smallest working example for the user's language or interface.

When details may have changed, check the official resource links in `references/resources.md` or the local Bashkit repo before giving exact version/package claims.

Load only the reference needed for the request:

- CLI usage: `references/cli.md`
- Rust API: `references/rust.md`
- Python API: `references/python.md`
- JavaScript/TypeScript API: `references/typescript.md`
- Builtins and shell support: `references/builtins.md`
- LLM tools and agent integrations: `references/llm-tools.md`
- Copy-paste examples: `references/examples.md`
- Official links and package pages: `references/resources.md`

## Response Rules

- Treat Bashkit as a sandboxed shell runtime, not a normal host shell.
- State when host filesystem or network access requires explicit opt-in.
- For CLI examples, use `bashkit -c '...'` unless script-file or REPL mode is needed.
- For library examples, show persistent state across calls when useful.
- For package installation, include the official package name and link source when relevant.
- Do not promise full Bash parity unless the docs say the feature is supported.
- If the user asks whether something is supported, check `references/builtins.md`, docs.rs, or the repo docs.
