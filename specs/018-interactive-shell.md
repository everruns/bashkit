# 018: Interactive Shell Mode

## Status
Phase 1: Implemented

## Decision

Bashkit provides an interactive REPL mode via `bashkit` (no arguments).
Uses `rustyline` for line editing — lightweight, MIT-licensed, no heavy
transitive deps (no SQLite, no crossterm, no serde). Fits bashkit's
isolation-first design.

### Invocation

```bash
bashkit                            # Interactive REPL (VFS only)
bashkit --mount-rw /path/to/work   # REPL with real filesystem access
```

### Features

| Feature | Status |
|---------|--------|
| Read-eval-print loop | Implemented |
| Prompt with cwd | Implemented |
| Readline editing (emacs/vi keys) | Implemented (rustyline) |
| Command history (in-memory) | Implemented |
| Ctrl-C interrupts current line | Implemented |
| Ctrl-D exits shell | Implemented |
| Multiline input (continuation) | Implemented |
| `exit [N]` builtin | Implemented (pre-existing) |
| Streaming output | Implemented |
| TTY detection (`[ -t 0 ]`) | Implemented |

### Design

#### Prompt

Default prompt: `bashkit:<cwd>$ ` (e.g. `bashkit:/home/user$ `).
Continuation prompt for multiline: `> `.

No PS1/PS2 customization in Phase 1 — keep it simple.

#### Multiline Detection

When a command fails to parse with "unterminated" or "unexpected end of
input" errors, the REPL shows a continuation prompt (`> `) and appends
the next line. This handles:

- Unterminated quotes (`echo "hello`)
- Open control structures (`if true; then`)
- Unterminated command substitution (`$(echo`)
- Backslash continuation (`echo \`)

#### Execution Limits

Interactive mode uses `ExecutionLimits::cli()` (same as `-c` and script
modes) with `SessionLimits::unlimited()`. No per-command timeout — user
has Ctrl-C.

#### TTY Configuration

All three FDs (stdin/stdout/stderr) report as terminals via `tty(fd, true)`.
This ensures `[ -t 0 ]`, `[ -t 1 ]`, `[ -t 2 ]` return true, matching
real shell behavior.

#### Output

Uses `exec_streaming()` with a callback that prints stdout/stderr
directly to the real terminal. This gives immediate output for loops
and long-running commands rather than buffering until completion.

#### Signal Handling

- **Ctrl-C**: rustyline returns `Err(Interrupted)` — clears current
  input, prints a new prompt. Does NOT kill the shell.
- **Ctrl-D**: rustyline returns `Err(Eof)` — exits the shell with
  the last command's exit code.
- Running commands: use `cancellation_token()` for future Ctrl-C
  during execution (Phase 2).

#### History

In-memory only via rustyline's `DefaultEditor`. No history file
persistence in Phase 1. Commands are added to rustyline history
and to bashkit's internal `HistoryEntry` tracking.

### Dependencies

```toml
# In bashkit-cli/Cargo.toml
rustyline = "18"
```

Rustyline's transitive deps: `libc`, `nix`, `unicode-segmentation`,
`unicode-width`, `utf8parse`, `memchr`, `log`. All MIT-licensed,
all in `deny.toml` allowlist.

### Security

Interactive mode reuses the existing sandbox. No new attack surface:

- VFS isolation preserved (unless `--mount-rw` explicitly used)
- All execution limits still enforced
- No real process spawning
- Panic hook still sanitizes error output

### Not Implemented (Future)

| Feature | Rationale |
|---------|-----------|
| PS1/PS2 prompt variables | Phase 2 — requires parameter expansion in prompt |
| Tab completion (paths, builtins) | Phase 2 — rustyline `Completer` trait |
| Syntax highlighting | Phase 2 — rustyline `Highlighter` trait |
| Persistent history file | Phase 2 — `~/.bashkit_history` |
| Job control (`bg`/`fg`/`jobs`) | By design — no real processes |
| `~/.bashkitrc` startup file | Phase 2 |
| Ctrl-C during command execution | Phase 2 — wire cancellation_token to signal handler |
| Terminal width detection | Phase 2 — `terminal_size` crate |

### Testing

| Test | Purpose |
|------|---------|
| Unit tests in `main.rs` | `CliMode::Interactive` detection |
| Integration (manual) | Launch `bashkit`, type commands, verify output |

Automated integration testing of interactive mode requires PTY
simulation (e.g. `expect` or `rexpect`). Deferred to Phase 2.

### Verification

```bash
# Build with interactive support
cargo build -p bashkit-cli

# Smoke test
echo 'echo hello' | bashkit

# Interactive session
bashkit
```

## See Also

- `specs/001-architecture.md` - Core interpreter architecture
- `specs/005-builtins.md` - Builtin command reference
- `specs/009-implementation-status.md` - Feature status
