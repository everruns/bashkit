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

**Brace expansion.** Braces are ordinary word characters to the lexer (a
word may start with `{` unless it is the `{` reserved word). Expansion runs
first, on the parsed word (`brace_expand_word`): unquoted literal text is
expanded, every other part (variables, substitutions, quoted segments) is an
opaque atom carried through as a private-use placeholder. So text produced by
an expansion never brace-expands (`y='{a,b}'; echo $y` prints `{a,b}`) and
`{1..$n}` stays literal. Quoted or backslash-escaped `{`, `}` and `,` in a
`QuotedGlobWord` arrive backslash-escaped and stay literal. An invalid group
keeps its `{` and later groups still expand (`{x}{a,b}`). Gap: bash expands
raw text, so `$v{1,2}` reads `$v1`; we keep the `$v` part.

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
`exec` still returns `Err(Parse)`; the `bashkit` CLI maps that to
`bash: syntax error: ...` on stderr and exit 2, so both cases exit 2 like
bash. `bash -n` keeps whole-script rejection.
Deferred `$(...)` errors carry no reliable position, so they never run a
prefix.

**End of `$(...)`.** `parser/subst_scan.rs` is the one scanner the lexer and
`parse_word` share to find the closing `)`: it tracks quotes, escapes,
backticks, nested `$(`, comments and heredoc bodies (`<<`, `<<-`, quoted
delimiters; `<<<` and arithmetic `<<` excluded). It also follows `case`
in command position (subject, `in`, pattern, body; `;;`/`;&`/`;;&` back to
pattern), so a pattern's `)` and an optional leading `(` do not change the
paren depth. Keywords are recognised only as whole unquoted words in command
position, so `echo case)` still closes.

**Here documents.** The lexer reads a heredoc body when the parser reaches
the delimiter, then re-injects the rest of the command line plus its line
break, so words and further redirects after it parse normally:
`cat <<A <<B` (the last one is stdin), `paste - <<A 3<<B` (`N<<` is
`Token::HereDocFd`; a non-zero fd does not feed stdin) and
`done <<A >out`. The fd-redirect lookahead reads re-injected text first.
`&>> file` parses as `>> file 2>&1`.

**`[[` lexing.** `[[` is the keyword only when followed by a word break
(space, tab, newline, `;`, `&`, `|`, `(`, `)`, or end); `[[:digit:]]*` is a
bracket-expression word.

**Function printing (`type`, `declare -f`).** `parser/print_cmd.rs` ports
bash's `print_cmd.c` layout over the AST: `name () ` / `{ ` header, 4-space
indent, `;` + newline between commands, `elif` printed as a nested
`else if`, `for x;` as `for x in "$@";`, a lone `[[ x ]]` operand as `-n x`,
`|&` as `2>&1 |`, and here-document bodies deferred to the end of their line
(including bash's quirk where the `;` after the command following a heredoc
is dropped). Words print as written: while parsing a function definition the
parser turns on lexer raw capture, and every word built from a token keeps
its source text in `Word::raw` (top-level words leave it `None`, so ordinary
scripts pay nothing). Raw text is captured from consumed chars, not span
offsets, because heredoc rest-of-line re-injection makes offsets drift.
Compound arrays print their elements joined by one space, `(( ))` and
`for (( ))` keep their text (`ArithmeticForCommand::raw`), and
`Redirect::heredoc_delim` keeps the delimiter (`'EOF'` when quoted). A word
without source text is reconstructed from its parts (never `Debug`,
TM-INF-022). Function bodies may be any compound command with redirections
(`f() ( ... )`, `f() if ...; fi`, `f() { ...; } >log`).

**Arithmetic commands.** `(( ... ))` and `for (( ...; ...; ... ))` are read
as raw text up to the matching `))` (`Lexer::read_dparen_body`), like bash's
`parse_dparen`, so `<<` is a shift (not a heredoc) and spacing survives;
evaluation uses `arith_exec_text` (trimmed, double quotes removed, as
`let "..."`).

**Here-document delimiters.** The delimiter's source text decides quoting:
any `'`, `"` or `\` in it (`<<'E'`, `<<\E`, `<<E"OF"`) makes the body
literal, and the delimiter is the quote-removed text. Several heredocs on one
command (`cat <<A <<'B'`) read their bodies in order. Lexer lookahead
(`Lexer::lookahead`) sees re-injected rest-of-line text first, so tokens
after a heredoc on the same line lex the same as anywhere else.

**`]]`, `|&`.** `]]` closes `[[` only as a whole word; `]]x` is a word, and
`]]` in argument position is the literal word. `a |& b` is `a 2>&1 | b`
(the `2>&1` is appended to `a`'s redirections).

**Backquotes parse late.** Bash parses a backquoted command when it runs, so
`` x=`fi` `` fails only that substitution (status 2). The lexer converts
backquotes to `$(...)`; a body that does not parse becomes
`$(eval 'body')`, which reports the syntax error at run time and never runs
anything else. `$(...)` keeps rejecting the script at parse time.

**Process substitution end.** `<(...)`/`>(...)` bodies end where the shared
`subst_scan` scanner closes them (`Lexer::skip_subst_body`), so
`<(case a in a) ...;; esac)` keeps the pattern's `)`; the body is still
sliced from the source, not copied (TM-DOS-021).

**Substitutions in `${x:-...}` operands.** Operand expansion is sync, so the
`$(...)` parts of a `:-`/`:=`/`:?`/`:+` operand run ahead in
`prefetch_operand_substs` (async) and are consumed in order. They run only
when the operator uses the operand, so an unused default never executes.
`${x:-$(echo })}`: the lexer and `read_brace_operand` let a substitution own
its braces.

## Alternatives Considered

- PEG (pest, pom): rejected, bash grammar is context-sensitive, here-docs awkward, manual parser gives better errors.
- Tree-sitter: rejected, incremental parsing overkill, large dep, harder to customize.

## See also

- [Bashkit Architecture](architecture.md), where the parser sits in the execution flow
- [Known Limitations](../operations/limitations.md), unsupported syntax, recorded as L-* entries
- [Script Analysis](../integrations/script-analysis.md), static introspection built on the AST
- [Testing Strategy](../operations/testing.md), differential testing against real Bash
