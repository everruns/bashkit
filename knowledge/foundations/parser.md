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
A quote-start word followed by an unquoted expansion also uses this encoding
when quoted text contains glob syntax or a quoted expansion could produce it:
`"$prefix"$empty` and `'literal*'$(printf '')` keep the quoted glob literal.
Expansion escaping and its byte-budget charge use each part's quote flag;
unquoted expansion output still participates in glob and pattern matching.
The shared marker builder preserves empty quoted spans and ends adjacent
variable names; sentinel-escaped marker bytes remain literal data.

**Quoted segments of mixed words.** A word that starts quoted and continues
with an unquoted expansion (`'a b'$x`, `"$y"$x`) also wraps its quoted spans
in `\x1e`/`\x1f`; `parse_word` turns them into `Word::part_quoted`, and field
splitting protects quoted parts using markers it picks from characters absent
from the data. No in-band marker reaches expansion, so a value holding any
byte (`\x01`, `\x02` included) stays data.
The same markers are used when a single-quoted word continues with a
double-quoted expansion (`'a'"$1"`, otherwise kept as one literal word) and
when an empty quoted segment sits beside an expansion (`"$@"""`, `''"$@"`).
Empty ranges are never merged into a neighbour, and `parse_word` keeps the
empty quoted part next to a quoted `@` expansion, so with no positional
parameters those words still make one empty field. A word whose parts are
all quoted is marked `Word::quoted` (no field splitting).

**`$` inside double quotes.** A `$` before `'` or before the closing `"` starts
no expansion and is NUL-escaped, so `"$'q'"` is the text `$'q'` and `"$"'q'`
is `$q`. Here-document bodies keep `$'...'` as written. Function listings
(`type`, `declare -f`) print `$'...'` as the single-quoted text it decodes to
and `$"..."` as `"..."`, as bash does (`normalize_raw_word`).

**Extglob groups.** A group (`@(...)`, `!(...)`, ...) is read whole; quotes
inside it make their text literal (`@(a|'b)')` has the alternative `b)`), and
`\|` stays a literal bar. An unbalanced group is plain text.

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
bash's report (`Error::syntax_report`: `bash: -c: line N: ...` or
`script.sh: line N: ...`) on stderr and exit 2, so both cases exit 2 like
bash. `bash -n` keeps whole-script rejection.

**Error wording.** `Parser::error` rewrites grammar errors (empty bodies,
missing keywords, stray tokens) the way bash's yacc parser words them, by the
token the parse stopped at: `syntax error near unexpected token `T'` (a line
end is `newline`; the report adds the offending source line), or, when input
ran out inside a construct, `syntax error: unexpected end of file` on line
`lines + 1`. An empty `if`/`elif`/`while`/`until` condition fails at
`then`/`do`, like bash. An unclosed `(` group in a `[[ =~ ]]` operand is read
to end of input and gives bash's two diagnostics; a further diagnostic is a
`line N: text` line inside the message. The REPL treats `unexpected end of
file` as incomplete input.
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
While the re-injected rest is read, the lexer position walks that line again
from where it was first read (`Lexer::heredoc_resume` holds the input
position after the body, restored when the rest is used up), so spans,
`$LINENO` and `Script::command_starts` after a heredoc match the source
(`cat <<EOF; echo $LINENO` is the `cat` line, as in bash).
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
offsets, because re-injected text (alias values, a heredoc's rest of line)
is read twice.
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
Quote continuations use the same backquote reader as ordinary words, so
`'x'` followed immediately by a backquoted command is one mixed word.
Unquoted substitution output still splits; static analysis reports nested
commands in substitution context, dispatch hooks still gate them, and nested
execution shares the parent's budget. Missing closing backquotes fail parsing.

**Process substitution end.** `<(...)`/`>(...)` bodies end where the shared
`subst_scan` scanner closes them (`Lexer::skip_subst_body`), so
`<(case a in a) ...;; esac)` keeps the pattern's `)`; the body is still
sliced from the source, not copied (TM-DOS-021). A substitution whose
source touches word text (`x<(true)`, `<(true)x`, `2<(true)`, `x=<(true)`)
joins that word or assignment value (`Parser::join_adjacent_word`, by span
adjacency), as bash reads it as one word.

**Substitutions in `${x:-...}` operands.** Operand expansion is sync, so the
`$(...)` parts of a `:-`/`:=`/`:?`/`:+` operand run ahead in
`prefetch_operand_substs` (async) and are consumed in order. They run only
when the operator uses the operand, so an unused default never executes.
`${x:-$(echo })}`: the lexer and `read_brace_operand` let a substitution own
its braces.

**Aliases expand at parse time, per line.** The interpreter hands the
parser a `ParseOptions` (alias table under `expand_aliases`, `extglob`).
When a command word is a plain (unquoted, unexpanded) alias name, the parser
pushes the value to the front of the lexer's reinject buffer (alias text does
not move source positions) and lexes on, so a value may hold `{`, `(`,
keywords or half a loop. `alias_stack` stops recursion, a value ending in a
blank makes the next word a candidate, and every expansion ticks the parser's
fuel (TM-DOS-030/031). A here-doc started inside an alias reads its body from
the next real input line. The script records each top-level command's start
(`Script::command_starts`, with the source; both `#[serde(skip)]`, so the
snapshot AST format is unchanged). When `alias`/`unalias`/`shopt extglob`
changes the options mid-script, the body loop re-parses the rest from the
next command that starts a new line (`reread_script_rest`), as bash reads
line by line: `alias e=..; e` on one line misses `e`, the next line sees
it. At the top level `;`/`&` followed by a newline ends the command, so the
next line can be re-read. `eval` and `source` go through the same loop.

**`!(` needs extglob.** `!(` lexes as an extglob group only when `extglob` is
on (a re-read after `shopt -s extglob` picks it up); otherwise it is `!` then
a subshell. The right operand of `==`/`!=`/`=` in `[[ ]]` always lexes
extglob, like bash.

**Compound array arguments.** `name=(...)` as an argument is a declaration
operand only after an unquoted literal `declare`, `typeset`, `local`,
`export`, `readonly`, `let` or `eval`; `builtin declare a=(x)` and
`command declare a=(x)` are syntax errors, as in bash.

## Alternatives Considered

- PEG (pest, pom): rejected, bash grammar is context-sensitive, here-docs awkward, manual parser gives better errors.
- Tree-sitter: rejected, incremental parsing overkill, large dep, harder to customize.

## See also

- [Bashkit Architecture](architecture.md), where the parser sits in the execution flow
- [Known Limitations](../operations/limitations.md), unsupported syntax, recorded as L-* entries
- [Script Analysis](../integrations/script-analysis.md), static introspection built on the AST
- [Testing Strategy](../operations/testing.md), differential testing against real Bash
