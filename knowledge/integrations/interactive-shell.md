---
type: Subsystem Design
title: Interactive Shell
description: Interactive REPL design with rustyline-based line editing.
tags:
  - bashkit
  - cli
  - repl
---

# Interactive Shell Mode

## Status
Implemented

## Decision

Bashkit provides an interactive REPL via `bashkit` (no arguments); add
`--mount-rw /path` for real filesystem access. Uses `rustyline` for line
editing, lightweight, MIT-licensed, no heavy transitive deps (no SQLite, no
crossterm). Fits bashkit's isolation-first design.

### Feature Flag

Behind the `interactive` feature flag (default on for the CLI binary,
compiled out in library mode): `interactive = ["dep:rustyline",
"dep:terminal_size", "dep:signal-hook"]`. Build without:
`cargo build -p bashkit-cli --no-default-features`.

### Features (all implemented)

REPL with streaming output; multiline continuation; Ctrl-C clears line /
interrupts running commands; Ctrl-D exits; `exit [N]`; in-memory command
history + fish-style history hints; readline editing (emacs/vi); PS1/PS2
custom prompts; tab completion; syntax highlighting (hint coloring); TTY
detection (`[ -t 0 ]`); terminal width detection; `~/.bashkitrc` startup
file; COLUMNS/LINES/SHLVL env vars.

### Design

#### Custom Prompt (PS1/PS2)

Supports bash-compatible PS1 escapes:

| Escape | Meaning |
|--------|---------|
| `\u` | Username ($USER) |
| `\h` | Short hostname (up to first `.`) |
| `\H` | Full hostname |
| `\w` | Working directory (~ for $HOME) |
| `\W` | Basename of working directory |
| `\$` | `$` for normal user, `#` for root (EUID=0) |
| `\n` | Newline |
| `\r` | Carriage return |
| `\a` | Bell |
| `\e` | Escape (0x1b) |
| `\[` | Start non-printing sequence |
| `\]` | End non-printing sequence |
| `\\` | Literal backslash |

Default PS1: `\u@bashkit:\w\$ ` (e.g. `user@bashkit:~$ `)

PS2 defaults to `> ` for continuation lines. Both can be set via
`export PS1='...'` or `PS2='...'`.

#### Tab Completion

Completes based on context:

- **Command position** (start of line, after `;`/`|`/`&&`/`||`):
  builtins (live registry via `Bash::builtin_names()`), aliases
- **Argument position**: VFS paths (files and directories)
- **`$` prefix**: environment and shell variables
- Directories show trailing `/`

Uses rustyline `Completer` trait with `CompletionType::List` (shows
all matches on tab).

#### History Hints

Fish-style inline suggestions from history: most recent matching entry as
dimmed text right of cursor; accept with right arrow.

#### Ctrl-C During Execution

`signal-hook` registers a SIGINT handler that sets bashkit's
`cancellation_token()`. A background tokio task polls the signal flag every
50ms and propagates to the cancel token; token resets for the next command.

#### Exit Handling

The `exit` builtin fires an `on_exit` hook registered via
`BashBuilder::on_exit()`. The REPL registers a hook at build time that sets an
atomic flag, checked after each `exec()`. Works through the normal execution
pipeline, `echo bye; exit 1`, conditionals, and scripts all terminate the
session correctly.

#### Multiline Detection

When a command fails to parse with known incomplete-input errors,
the REPL shows PS2 and appends the next line. Detected patterns:

- `"unterminated"`, open quotes, command substitution
- `"unexpected end of input"`, incomplete constructs
- `"syntax error: empty"`, empty body/clause
- `"expected 'fi'"` / `"expected 'done'"` / `"expected 'esac'"`, missing closers
- `"expected '}' to close brace group"`, open functions

#### Startup File

Sources `~/.bashkitrc` from the VFS on startup (if it exists). Use
`--mount-rw` to make a real host directory available with a `.bashkitrc`.

#### Environment

Sets `COLUMNS`/`LINES` from the `terminal_size` crate (no hardcoded 80) and
`SHLVL` (incremented from parent, or 1).

### Dependencies

`rustyline` 18, `terminal_size` 0.4, `signal-hook` 0.4, all optional, gated
by `interactive`, all MIT-licensed, all in `deny.toml` allowlist.

### Security

Reuses the existing sandbox. No new attack surface:

- VFS isolation preserved (unless `--mount-rw` explicitly used)
- All execution limits still enforced
- No real process spawning
- Panic hook still sanitizes error output

### Not Implemented (By Design)

| Feature | Rationale |
|---------|-----------|
| Job control (`bg`/`fg`/`jobs`) | No real processes, by design |
| History expansion (`!!`, `!N`) | Complexity vs value tradeoff |
| Persistent history file on the host | Leaks info across sessions, breaks isolation; `HISTFILE` lives in the VFS only (see below) |
| `exec` builtin | Excluded for security |

### In-interpreter interactive builtins (`bash -i`, no feature flag)

Separate from the REPL above: the interpreter itself models bash's
interactive state for scripts and Oils spec cases, per interpreter, never
global.

- `bash -i` (and `--rcfile F`, `--norc`) starts a child with `$-` holding
  `i`, `set -o history`, default `PS1` `\s-\v\$ `, `PS2`, `PS4`,
  `HISTFILE=$HOME/.bash_history`, `HISTSIZE`/`HISTFILESIZE` 500, and sources
  `~/.bashrc` when it is a regular VFS file. The parent's history state is
  saved and restored around the child.
- A `LineReader` (keyed by the top-level script) records one history entry
  per input line before it runs; for `bash -i` reading a script from stdin it
  also runs `PROMPT_COMMAND` (keeping `$?`), prints the decoded `PS1` to
  stderr per line, and prints the prompt plus `exit` at end of input.
- `history`/`fc` (`interpreter/history.rs`), `complete`/`compgen`/`compopt`
  (`interpreter/completion.rs`), `bind` (`interpreter/readline_bind.rs`,
  defaults generated from bash 5.2 in `readline_defaults.rs`) and prompt
  decoding (`interpreter/prompt.rs`) are interpreter-dispatched so they can
  keep shell state. Completion and binding state is allocated on first use.
- Caps: TM-DOS-131 (history entries, `HISTFILE` read size) and TM-DOS-132
  (completion specs, binding changes). Gaps: limitations "Interactive
  builtins" row, L-HIST-001.

### Testing

Unit tests cover incomplete-input detection, PS1 expansion, prompt format,
exec/state (streaming, persistence, TTY, rc file), error propagation. Compile
only with the `interactive` feature: `cargo test -p bashkit-cli`
(`--no-default-features` to test without).

## See also

- [Bashkit Architecture](../foundations/architecture.md) - Core interpreter architecture
- [Builtin Commands](../foundations/builtins.md) - Builtin command reference
- [Known Limitations](../operations/limitations.md) - Intentional gaps and partial features
