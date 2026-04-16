# Parser Design

## Status
Implemented (core features)

## Decision

Bashkit uses a recursive descent parser with a context-aware lexer.

### Tokenization Flow

```
Input → Lexer → Tokens → Parser → AST
```

Token types, AST structures, and parser grammar are defined in
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

The lexer handles bash's context-sensitivity:
- `$var` in double quotes: expand variable
- `$var` in single quotes: literal text
- Word splitting after expansion
- Glob patterns (*, ?, [])
- Brace expansion: `{a,b,c}` and `{1..5}` vs brace groups `{ cmd; }`
- Tilde expansion: `~` at start of word expands to `$HOME`

### Arithmetic Expressions

`$((expr))` supports: `+`, `-`, `*`, `/`, `%`, comparisons, logical `&&`/`||`
(short-circuit), bitwise operators, ternary `?:`, variable references.

### Error Recovery

Parser produces errors with line/column numbers, expected vs. found token,
and context (what was being parsed).

## Alternatives Considered

### PEG parser (pest, pom)
Rejected: Bash grammar is context-sensitive, PEG can't handle here-docs well,
manual parser gives better error messages.

### Tree-sitter
Rejected: Designed for incremental parsing (overkill), large dependency,
harder to customize.

## Verification

```bash
cargo test --lib -- parser
```
