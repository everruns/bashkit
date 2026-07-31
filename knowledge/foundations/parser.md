---
type: Subsystem Design
title: Parser
description: Bash syntax parser and lexer architecture and compatibility decisions.
tags:
  - bashkit
  - parser
  - bash
---

# Parser Design

## Status
Implemented

## Decision

Recursive descent parser with a context-aware lexer.

```
Input → Lexer → Tokens → Parser → AST
```

Token types, AST structures, and parser grammar live in
`crates/bashkit/src/parser/`. They evolve as features are added.

### Parser Rules (Simplified)

```
script        → command_list EOF
command_list  → pipeline (('&&' | '||' | ';' | '&') pipeline)*
pipeline      → command ('|' command)*
command       → simple_command | compound_command | function_def
simple_command → (assignment)* word (word | redirect)*
redirect      → ('>' | '>>' | '<' | '<<' | '<<<') word
               | NUMBER ('>' | '<') word
```

### Context-Aware Lexing

Handles bash's context-sensitivity:
- `$var` in double quotes: expand; in single quotes: literal
- Word splitting after expansion
- Glob patterns (*, ?, [])
- Brace expansion: `{a,b,c}` and `{1..5}` vs brace groups `{ cmd; }`
- Tilde expansion: `~` at start of word expands to `$HOME`

### Arithmetic Expressions

`$((expr))` supports: `+`, `-`, `*`, `/`, `%`, comparisons, logical `&&`/`||`
(short-circuit), bitwise operators, ternary `?:`, variable references.

### Error Recovery

Errors carry line/column, expected vs. found token, and parse context.

## Alternatives Considered

- PEG (pest, pom): rejected — bash grammar is context-sensitive, here-docs awkward, manual parser gives better errors.
- Tree-sitter: rejected — incremental parsing overkill, large dep, harder to customize.

## See also

- [Bashkit Architecture](architecture.md) — where the parser sits in the execution flow
- [Known Limitations](../operations/limitations.md) — unsupported syntax, recorded as L-* entries
- [Script Analysis](../integrations/script-analysis.md) — static introspection built on the AST
- [Testing Strategy](../operations/testing.md) — differential testing against real Bash
