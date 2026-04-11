# 018: Interactive Shell Mode

## Status
Phase 1: Implemented
Phase 2: Implemented
Phase 3: Implemented

## Decision

Bashkit provides an interactive REPL mode via `bashkit` (no arguments).
Uses `rustyline` for line editing — lightweight, MIT-licensed, no heavy
transitive deps (no SQLite, no crossterm). Fits bashkit's isolation-first
design.

### Feature Flag

Interactive mode is behind the `interactive` feature flag (default on
for the CLI binary, compiled out in library mode):

```toml
[features]
default = ["python", "interactive"]
interactive = ["dep:rustyline", "dep:terminal_size", "dep:signal-hook"]
```

Build without interactive:
```bash
cargo build -p bashkit-cli --no-default-features
```

### Invocation

```bash
bashkit                            # Interactive REPL (VFS only)
bashkit --mount-rw /path/to/work   # REPL with real filesystem access
```

### Features

| Feature | Status | Phase |
|---------|--------|-------|
| Read-eval-print loop | Implemented | 1 |
| Multiline input (continuation) | Implemented | 1 |
| Ctrl-C clears current line | Implemented | 1 |
| Ctrl-D exits shell | Implemented | 1 |
| `exit [N]` builtin | Implemented (pre-existing) | 1 |
| Streaming output | Implemented | 1 |
| TTY detection (`[ -t 0 ]`) | Implemented | 1 |
| Readline editing (emacs/vi keys) | Implemented (rustyline) | 1 |
| PS1/PS2 custom prompt | Implemented | 2 |
| Tab completion (builtins, paths, vars) | Implemented | 2 |
| History hints (fish-style) | Implemented | 2 |
| Syntax highlighting (hint coloring) | Implemented | 2 |
| Ctrl-C interrupts running commands | Implemented (signal-hook) | 2 |
| Terminal width detection | Implemented (terminal_size) | 2 |
| `~/.bashkitrc` startup file | Implemented | 2 |
| COLUMNS/LINES/SHLVL env vars | Implemented | 2 |
| Feature-gated (`interactive` flag) | Implemented | 3 |
| Command history (in-memory) | Implemented | 1 |
| Persistent history file | Implemented (~/.bashkit_history) | 2 |

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
  builtins (100+), aliases
- **Argument position**: VFS paths (files and directories)
- **`$` prefix**: environment and shell variables
- Directories show trailing `/`

Uses rustyline `Completer` trait with `CompletionType::List` (shows
all matches on tab).

#### History Hints

Fish-style inline suggestions from history. Shows the most recent
matching history entry as dimmed text to the right of the cursor.
Accept with right arrow.

#### Ctrl-C During Execution

Uses `signal-hook` to register a SIGINT handler that sets bashkit's
`cancellation_token()`. A background tokio task polls the signal flag
every 50ms and propagates to the cancel token. After cancellation,
the token is reset for the next command.

#### Multiline Detection

When a command fails to parse with known incomplete-input errors,
the REPL shows PS2 and appends the next line. Detected patterns:

- `"unterminated"` — open quotes, command substitution
- `"unexpected end of input"` — incomplete constructs
- `"syntax error: empty"` — empty body/clause
- `"expected 'fi'"` / `"expected 'done'"` / `"expected 'esac'"` — missing closers
- `"expected '}' to close brace group"` — open functions

#### Startup File

Sources `~/.bashkitrc` from the VFS on startup (if it exists).
Use `--mount-rw` to make a real host directory available, then
create `.bashkitrc` with aliases, PS1, etc.

#### Environment Variables

Interactive mode sets:
- `COLUMNS` — terminal width (from `terminal_size` crate)
- `LINES` — terminal height
- `SHLVL` — incremented from parent (or 1)

#### Terminal Width Detection

Uses `terminal_size` crate instead of hardcoded 80 columns.
Width is detected at startup and set via `$COLUMNS`.

### Dependencies

```toml
# In bashkit-cli/Cargo.toml (all optional, gated by "interactive" feature)
rustyline = { version = "18", optional = true }
terminal_size = { version = "0.4", optional = true }
signal-hook = { version = "0.4", optional = true }
```

All MIT-licensed, all in `deny.toml` allowlist.

### Security

Interactive mode reuses the existing sandbox. No new attack surface:

- VFS isolation preserved (unless `--mount-rw` explicitly used)
- All execution limits still enforced
- No real process spawning
- Panic hook still sanitizes error output

### Not Implemented (By Design)

| Feature | Rationale |
|---------|-----------|
| Job control (`bg`/`fg`/`jobs`) | No real processes — by design |
| History expansion (`!!`, `!N`) | Complexity vs value tradeoff |
| `exec` builtin | Excluded for security |

### Testing

| Test | Count | Purpose |
|------|-------|---------|
| `is_incomplete_input` | 5 | Parse error pattern detection |
| `expand_ps1` | 8 | PS1 escape expansion |
| Prompt integration | 1 | Default prompt format |
| Exec/state | 5 | Streaming, persistence, TTY, rc file |
| Error result | 1 | Error code propagation |

Tests compile only when `interactive` feature is enabled. Run:
```bash
cargo test -p bashkit-cli             # with interactive (default)
cargo test -p bashkit-cli --no-default-features  # without
```

### Verification

```bash
# Build with interactive support (default)
cargo build -p bashkit-cli

# Build without interactive (library-only deps)
cargo build -p bashkit-cli --no-default-features

# Smoke test
echo 'echo hello' | bashkit

# Interactive session
bashkit
```

## See Also

- `specs/001-architecture.md` - Core interpreter architecture
- `specs/005-builtins.md` - Builtin command reference
- `specs/009-implementation-status.md` - Feature status
