# bashkit-cli

Command-line interface for running bash scripts in a sandboxed virtual
filesystem. One binary, three modes.

## Modes

| Invocation | Mode |
|------------|------|
| `bashkit -c '…'` | Execute command string, print stdout/stderr, exit |
| `bashkit script.sh` | Execute script file |
| `bashkit` | Interactive shell (REPL) |

Mode is detected from arguments — `-c` wins, then positional script,
otherwise REPL.

## Install

From crates.io:

```bash
cargo install bashkit-cli
```

Prebuilt binary via [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall):

```bash
cargo binstall bashkit-cli
```

### Build features

| Feature | Default | Effect |
|---------|---------|--------|
| `interactive` | on | Interactive REPL (rustyline, signal-hook, terminal_size) |
| `python` | on | `python`/`python3` builtin via Monty |
| `realfs` | off | `--mount-ro` / `--mount-rw` host filesystem mounts |
| `scripted_tool` | off | Scripted tool orchestration |

Build without interactive (library-only deps):

```bash
cargo build -p bashkit-cli --no-default-features
```

## Defaults

Builtins enabled out of the box:

- **Git** (`git`) — local VFS operations (init, add, commit, log, …)
- **Python** (`python`, `python3`) — embedded via [Monty](https://github.com/pydantic/monty) (requires `python` feature)

Disabled by default (security):

- **HTTP** (`curl`, `wget`) — network access stays blocked unless explicitly enabled

Disable per-run:

| Flag | Effect |
|------|--------|
| `--http-allow-all` | Enable curl/wget with unrestricted outbound access (trusted scripts only) |
| `--no-http` | Force-disable curl/wget builtins |
| `--no-git` | Disable git builtin |
| `--no-python` | Disable python/python3 builtins |

## Execution limits

All modes start from `ExecutionLimits::cli()` — counting-based limits are
effectively unlimited, timeout is off (user has Ctrl-C). Memory guards
(function depth, AST depth, parser fuel) stay on.

Override with:

| Flag | Meaning |
|------|---------|
| `--max-commands N` | Max commands per run |
| `--max-loop-iterations N` | Max iterations in a single loop |
| `--max-total-loop-iterations N` | Max iterations across all loops |
| `--timeout SECONDS` | Wall-clock execution timeout |

## Host filesystem mounts (`realfs` feature)

By default the VFS is in-memory — scripts cannot reach the host. With
`realfs`:

| Flag | Effect |
|------|--------|
| `--mount-ro HOST[:VFS]` | Overlay host dir as read-only |
| `--mount-rw HOST[:VFS]` | Overlay host dir as read-write |

Omit `:VFS` to overlay at VFS root. Both flags repeat.

```bash
bashkit --mount-ro /path/to/project -c 'ls /'
bashkit --mount-ro /data:/mnt/data -c 'wc -l /mnt/data/*.csv'
bashkit --mount-rw /tmp/out:/mnt/out script.sh
```

**Warning.** `--mount-rw` breaks the sandbox boundary — scripts can modify
host files. Prefer `--mount-ro` unless writes are required.

## Interactive shell

Run `bashkit` with no arguments. The REPL uses `rustyline` for line editing
and reuses the same sandbox as `-c`. Lightweight deps, no SQLite, no
crossterm.

### Features

- Emacs / vi line editing, in-memory history (1 000 entries)
- Multiline input — unterminated quotes, `if`/`for`/`while`/`case`/functions
  reprompt with PS2 until closed
- Ctrl-C cancels the running command (propagates via the cancellation token);
  at an empty prompt it clears the line
- Ctrl-D exits the shell
- `exit [N]` exits via an `on_exit` hook (works from pipelines and
  conditionals: `echo bye; exit 1`)
- Streaming output — stdout/stderr flushed as produced
- TTY detection: `[ -t 0 ]`, `[ -t 1 ]`, `[ -t 2 ]` all return true
- Tab completion — builtins, aliases, `$VAR`, VFS paths (directories get
  trailing `/`)
- Fish-style history hints inline (dim gray); accept with right arrow
- `COLUMNS`, `LINES` exported from the real terminal size; `SHLVL`
  incremented from parent

### Prompt (PS1 / PS2)

Default `PS1`: `\u@bashkit:\w\$ ` (e.g. `user@bashkit:~$ `). Override with
`export PS1='…'`. `PS2` (continuation) defaults to `> `.

Supported escapes:

| Escape | Meaning |
|--------|---------|
| `\u` | Username (`$USER`) |
| `\h` | Short hostname (up to first `.`) |
| `\H` | Full hostname |
| `\w` | Working directory, `~` for `$HOME` |
| `\W` | Basename of cwd |
| `\$` | `$` for non-root, `#` if `EUID=0` |
| `\n` `\r` `\a` `\e` | Newline, CR, bell, ESC |
| `\[` `\]` | Non-printing sequence markers (ANSI codes) |
| `\\` | Literal backslash |

### Startup file

Sources `~/.bashkitrc` from the VFS on startup if present. Put it on the
host and expose it via `--mount-rw /path:/home/user` (or `--mount-ro` for a
read-only rc). Typical contents: aliases, `PS1`, environment.

### Not implemented (by design)

- Job control (`bg`/`fg`/`jobs`) — no real processes
- History expansion (`!!`, `!N`) — complexity vs. value
- Persistent history file — would leak across sessions, breaks isolation
- `exec` — excluded for security

## Examples

Text processing:

```bash
bashkit -c 'echo "hello world" | tr a-z A-Z'
# HELLO WORLD
```

Python (default):

```bash
bashkit -c 'python3 -c "print(2 + 2)"'
# 4
```

Git on the VFS:

```bash
bashkit -c '
git init /repo
cd /repo
echo "# readme" > README.md
git add README.md
git commit -m "init"
git log --oneline
'
```

Disable a builtin:

```bash
bashkit --no-python -c 'python --version'
# python: command not found
```

Run a script file:

```bash
bashkit script.sh arg1 arg2
```

Interactive shell:

```bash
bashkit
user@bashkit:~$ echo hi
hi
user@bashkit:~$ exit
```

Mount host workspace read-only and inspect:

```bash
bashkit --mount-ro "$PWD:/mnt/repo" -c 'wc -l /mnt/repo/**/*.rs'
```

Tighten limits for an untrusted script:

```bash
bashkit --max-commands 1000 --timeout 5 untrusted.sh
```

## Error handling

Stack backtraces are suppressed. Panics emit a single sanitized line
(`bashkit: internal error: …`) — no paths, line numbers, or dependency
versions.

## See also

- [`docs/security.md`](security.md) — threat model and mitigations
- [`README.md`](../README.md) — library usage and features
