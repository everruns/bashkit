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
time_command  → 'time' time_option* pipeline
simple_command → (assignment)* word (word | redirect)*
redirect      → ('>' | '>>' | '<' | '<<' | '<<<') word
               | NUMBER ('>' | '<') word
```

`time` is a reserved-word compound command, not a normal builtin. Its body is
therefore a pipeline AST and can contain groups, functions, nested `time`, and
redirections without converting shell syntax back into strings. Bashkit accepts
Bash/POSIX `-p` plus `--` and the practical GNU report flags `-f/--format`,
`-o/--output`, `-a/--append`, and `-v/--verbose` on this grammar node.

### Context-Aware Lexing

Handles bash's context-sensitivity:
- `$var` in double quotes: expand; in single quotes: literal
- Word splitting after expansion
- Glob patterns (*, ?, [])
- Brace expansion: `{a,b,c}` and `{1..5}` vs brace groups `{ cmd; }`
- Tilde expansion: `~` at start of word expands to `$HOME`

**Quoted glob characters.** `"*"`, `'?'` and `\[` are literal; only
unquoted `*?[` (and brace syntax) glob. Token kinds encode this: a word whose
quoted text has glob characters but no unquoted glob is a `QuotedWord` (never
globs). A word with both is a `QuotedGlobWord`: the lexer backslash-escapes
the quoted glob characters in place and wraps quoted ranges in `\x1e`/`\x1f`
markers (so `"$x"zz*` ends the variable name at the quote). Consumers:
globbing uses the escaped text and drops the escapes when nothing matches;
`expand_word` returns unescaped text for non-glob uses; assignments and
script analysis unescape literal parts; `case` and `[[ == ]]` build patterns
with `expand_pattern_word`, which escapes fully quoted words.

**Metacharacters vs reserved words.** Only space, tab, newline, `|`, `&`, `;`,
`(`, `)`, `<` and `>` delimit a word. `{` and `}` do not: they are reserved
words, recognized as such only when they stand alone. So `echo a}b` prints
`a}b`, and `}b` is a command named `}b`. The lexer decides this with
`is_brace_group_start` on the opening side and `right_brace_stands_alone` on
the closing side; both word readers (`read_word`, `read_word_starting_with`)
keep a `}` that has no opener inside the word. `for`/`select` `in` lists apply
the same distinction to `do`/`done`/`in` — see `for_in_reserved_word_tests`.

Treating `}` as a metacharacter is not just a cosmetic difference: it splits
`v=a}b`, `for i in a}b`, and `case x}` at a point where the grammar expects a
terminator, so they fail to parse rather than printing the wrong thing.
Regressions: `close_brace_word_tests` and the lexer unit tests, all pinned
against GNU bash 5.2.

### Arithmetic Expressions

`interpreter/arithmetic.rs` tokenizes the expression, then evaluates it with a
precedence-climbing parser following bash's `expr.c` grammar (comma, assignment
ops, `?:`, `||`, `&&`, bitwise, equality, relational, shifts, `+ -`, `* / %`,
right-associative `**`, unary, pre/post `++ --`). Numbers take `0x`, octal and
`base#digits` (bases 2..64). Short-circuit branches parse without evaluating
(`noeval`); writes are collected and applied after a successful evaluation.

The body of a `(( ... ))` command is read from source as raw text
(`Lexer::read_arith_raw`), so `<<=` or `>>` never become redirect tokens.

Errors follow bash: `division by 0`, `syntax error`, `bad array subscript`,
`exponent less than 0`, `expression recursion level exceeded`, invalid base.
In `$((...))` or an arithmetic subscript the error aborts the rest of the
current line (`ControlFlow::Abort` / `Error::LineAbort`): at the top level the
shell continues with the next line, inside a function or subshell the whole
compound stops, status 1. `((...))` and `let` just return status 1.

### Error Recovery

Errors carry line/column, expected vs. found token, and parse context.

A nested parse must never silently vanish. `parse_word` is infallible (the
interpreter also calls it for lazy parameter expansion), so a `$(...)` body that
fails to parse still pushes its `CommandSubstitution` part, with empty commands
, and stashes the inner error in `Parser::deferred_error`, which `parse_script`
turns into a hard parse error. Both halves matter: the retained part keeps the
word non-literal so surrounding literals cannot splice (`a$(|)b` must not become
the command `ab`, which `analysis` would report to a host permission gate, see
TM-ESC-032), and the deferred error rejects the script the way bash does.
Process substitution keeps its part for the same reason, and hard-errors on
budget failures (TM-DOS-021).

**Partial execution before a syntax error.** Bash reads and runs a script line
by line, so `echo a` on line 1 runs before `if then` on line 2 is reported
(exit 2). `Parser::parse_recovering` returns the complete top-level commands
that ended on lines *before* the line where the failing command starts, plus
the error; `Bash::exec` and child `bash`/`sh` run them and then report the
error via `Script::trailing_error` (stderr + exit 2), unless `exit` or
`set -e` stopped the script first. With nothing runnable before the error,
`exec` still returns `Err(Parse)`. `bash -n` keeps whole-script rejection.
Deferred `$(...)` errors carry no reliable position, so they never run a
prefix.

**End of `$(...)`.** `parser/subst_scan.rs` is the one scanner the lexer and
`parse_word` share to find the closing `)`: it tracks quotes, escapes,
backticks, nested `$(`, comments and heredoc bodies (`<<`, `<<-`, quoted
delimiters; `<<<` and arithmetic `<<` excluded). Known gap: a bare `case`
pattern `)` inside `$(...)` still closes early.

## Alternatives Considered

- PEG (pest, pom): rejected, bash grammar is context-sensitive, here-docs awkward, manual parser gives better errors.
- Tree-sitter: rejected, incremental parsing overkill, large dep, harder to customize.

## See also

- [Bashkit Architecture](architecture.md), where the parser sits in the execution flow
- [Known Limitations](../operations/limitations.md), unsupported syntax, recorded as L-* entries
- [Script Analysis](../integrations/script-analysis.md), static introspection built on the AST
- [Testing Strategy](../operations/testing.md), differential testing against real Bash
