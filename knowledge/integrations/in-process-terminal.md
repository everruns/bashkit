---
type: Subsystem Design
title: In-Process Terminal
description: PTY-like in-memory terminal around a Bash session, with a screen model, raw-mode device, and the vi builtin.
tags:
  - bashkit
  - terminal
  - vi
  - interactive
---

# In-Process Terminal

## Status

Implemented behind the `terminal` cargo feature (off by default). Python
exposes it as `bashkit.Terminal`, the NAPI package as `Terminal` (both
packages enable the feature), and the browser wasm package as `Terminal`
(`crates/bashkit-wasm/src/terminal.rs`, see
[WebAssembly Package](../runtimes/browser-package.md)), which powers the
bashkit.sh `/playground`. C bindings are not wired yet.

Code: `crates/bashkit/src/terminal/` (`Terminal`, the `Tty` device),
`crates/bashkit/src/builtins/vi.rs`, `InputWaitClock` in
`crates/bashkit/src/time_compat/mod.rs`. Public guide: `docs/terminal.md`.

## Problem

`Bash::exec()` is one-shot: a script in, stdout/stderr out. Two consumers need
more: LLM agents that should operate a shell like a person (including
full-screen editing) and UIs that render a terminal (xterm.js). Both need
keystrokes in, a screen out, and programs that read keys while they run. The
[Interactive Shell](interactive-shell.md) REPL does this only for the CLI on a
real host terminal via rustyline.

## Decision: pull-driven session future

`Terminal` owns one boxed `Send` future (the shell loop, which owns the `Bash`).
It runs only inside `Terminal::run_until_idle()`, which returns when the
session is blocked reading the device with an empty input queue (`Idle`) or the
shell exits. No spawned task, no channel to a background thread.

Why: "idle" is exactly when an agent should read the screen and decide the next
keys, so the API maps directly onto an agent loop. It works unchanged on
multi-thread, current-thread, and single-threaded wasm runtimes. Dropping
`run_until_idle()` mid-way (under `tokio::time::timeout`) leaves the session
future in place, which is how a host sends Ctrl-C to a long command.

Idle detection: the device read sets `waiting = true` and fires a
`tokio::sync::Notify`; `run_until_idle` selects on the session future and that
notify, and re-checks `waiting && queue empty` because notify permits can be
stale. `send()` clears `waiting`.

## Decision: vt100 crate as the screen model, not libghostty

The screen model (`screen_text()`, cursor, alternate screen) is the `vt100`
crate: MIT, pure Rust, three small deps (`vte`, `unicode-width`, `itoa`),
builds for wasm32-unknown-unknown and wasip2 (CI checks both with the feature
on). libghostty-vt was considered and rejected: it needs a Zig toolchain and
C FFI, which would break the wasm, PyPI and npm builds, for a feature whose
screen needs are basic (cells, cursor, alternate screen). Revisit only if
fidelity gaps show up that vt100 cannot close.

## Decision: device handle via execution extensions

The shell loop passes the `Tty` handle on every `exec_with_options` call
through `ExecutionExtensions`. Builtins reach it with
`ctx.execution_extension::<Tty>()`; it is revoked when the execution ends, and
nested `bash -c` children see it too. Builtins write screen output straight to
the device (like a real program writing to `/dev/tty`), not through
`ExecResult.stdout`, so `vi` redraws reach the screen immediately even though
normal command output is streamed per command.

`Tty` is `pub(crate)`: only internal builtins can be full-screen programs for
now. Exposing it to custom builtins is a deliberate later step.

## Decision: three read paths for agents

`screen_text()` (what a person sees), `history_text()` (screen plus 1000 rows
of vt100 scrollback, read by paging the scrollback offset and restoring it),
and `take_transcript()` (one `CommandRecord` per finished command line:
command, exact stdout+stderr from the streaming callback, exit code).
`activity()` reports `Prompt` / `ContinuationPrompt` / `Running { command }` /
`Exited`, set by the shell loop. Why: agents should not parse prompts out of
screen text to learn exit codes or recover scrolled-off output. Full-screen
programs write to the device, not the streaming callback, so they never land
in the transcript. Transcript is drained (like `take_output`) and bounded
(64 KiB output per record, 1 MiB total, oldest dropped).

## Decision: TerminalTool is its own tool, not a `Tool` impl

`terminal::TerminalTool` (`terminal/tool.rs`) exposes one session to an LLM:
`call({"input", "wait_ms"})` returns screen, activity, `full_screen`,
`waiting_for_input` and the drained transcript. It does not implement the
`Tool` trait ([Tool Contract](tool-contract.md)): that contract is one isolated
shell per call with `{"commands"}` input, while a terminal session carries
state, open programs and the screen across calls. It mirrors the metadata
names (`name`, `description`, `system_prompt`, `input_schema`,
`output_schema`, `tool_definition`) so hosts register it the same way.
Keys use Vim notation (`ihi<Esc>:wq<Enter>`, `<C-c>`, `<lt>`), unknown
`<...>` tokens are typed literally, so heredocs (`<<EOF`) pass through. Each
call waits up to `wait_ms` (default 5 s, max 60 s); an unfinished command is
reported as `running`, never killed. Input per call is capped at 64 KiB.

`wait_for` (regex, multi-line, 1 KiB pattern and 1 MiB compiled cap) returns
early once command output printed during the call matches. It matches a tap of
command stdout/stderr (newest 256 KiB), not the screen: the screen also holds
the echoed keystrokes, so `echo ready<Enter>` would match itself. Text is
ANSI-stripped first. The call polls `run_until_idle` in 50 ms slices; the
reply carries `matched`. `screen: "changes"` returns only rows that differ
from the previous call plus the cursor, `"none"` drops the screen; every call
updates the baseline. Long agent sessions resend a 24-row screen otherwise.

## Decision: sessions are tool-level tabs sharing one VFS

`TerminalTool::with_sessions(size, factory)` holds up to 8 `Terminal`s
(TM-DOS-119); the `session` argument picks one, creating it on first use from
`factory().fs(main_fs)`. Why a factory: `BashBuilder` is not `Clone`, and
each session needs its own interpreter (shell state is per tab, like real
terminal tabs on one machine). Why shared files: tabs are only useful if a
command in one sees what another wrote. While waiting on the target session
the tool polls the others' `run_until_idle` in a `join_all` beside it, so
background tabs progress only during tool calls (nothing runs between calls,
same as a single session). `close` drops a tab (not `main`); an exited tab
restarts on the next call with non-empty input (`restarted: true`).
`TerminalTool::new` stays single-session and does not advertise `session` in
its schema. Python/JS bindings always use `with_sessions`.

## Decision: line discipline split

- Cooked mode (prompt): the shell loop's own small line editor (echo, cursor
  movement with Left/Right/Home/End/^A/^E, Backspace/Delete, ^U, ^K, ^W, Up/Down
  history of the last 500 lines, ^C discards line, ^D on empty line exits).
  Redraws assume one cell per char and step back with CSI D, which does not
  cross a soft-wrapped row; editing a line longer than the terminal width can
  misplace the cursor. Command output is
  post-processed `\n` to `\r\n` (ONLCR). Multiline detection reuses the CLI
  REPL's parse-error heuristics.
- Raw mode: set by `vi` through a drop guard (restored on cancel too); bytes go
  to the reader untouched and Ctrl-C is not an interrupt.
- While a command runs in cooked mode, a sent `0x03` sets the interpreter's
  cancellation token and fires an interrupt `Notify` instead of being queued
  (ISIG). `exec` races the command future against that notify and drops it
  mid-command, the same way the execution timeout does, then runs
  `clear_cancelled_execution_state` (L-TERM-003 lifted). The shell loop
  prints `^C` and `$?` becomes 130. An interactive session carries `$?`
  across lines (`Bash::carry_exit_code` seeds it after the per-exec reset).
- Command stdin is still a value fixed before the command starts (L-TERM-002),
  but `read` and `select` with no stdin read a typed line from the device
  themselves (`terminal::read_input`: the cooked line editor, or a no-echo /
  N-chars loop for `read -s` / `-n`). `read -p` writes its prompt to the
  terminal, `-t` returns 142 on timeout, Ctrl-D on an empty line is EOF,
  Ctrl-C interrupts the command. Piped or redirected stdin still wins.
  Other commands (`cat`, `head`) still get EOF; a reader-backed fd 0 is not
  built (shared with L-CLI-002).
- While such a read waits, `Tty::reading_line` is set (drop guard) and
  `Terminal::input_prompt()` returns the cursor-row text left of the cursor,
  which covers `read -p`, `echo -n 'Q? '; read` and `select`'s `PS3`.
  `TerminalTool` reports activity `input` with `input_prompt`, so agents can
  tell a question from a busy command without screen heuristics.
- A reader dropped mid-wait clears the idle flag (guard in `read_event`),
  and `run_until_idle` polls an idle session once more before returning, so
  an expired `read -t` deadline fires on the next host call.

## Decision: input wait excluded from the execution timeout

`ExecutionLimits::timeout` wraps the whole `exec()`. A `vi` session is one
command, so it would be killed after the timeout while waiting for a person or
agent. `Terminal` installs an `InputWaitClock` on its `Bash`; the device read
pauses it while blocked, and `exec_impl` uses
`time_compat::timeout_excluding_input_wait`, which re-arms the portable timer
with the remaining active budget. Busy work and `sleep` still time out.
Plain `Bash` instances have no clock and take the old path. TM-DOS-120.

## vi builtin

Subset editor sized for config/source edits (L-TERM-001): modes, counts,
motions, `d c y` with `w e b $ 0` and doubled forms, `p P x X r J o O`,
undo/redo, `/` search, `:s` / `:%s` (regex, `&` = match), `:w :q :wq :x ZZ ZQ`,
`:N`. Lines are `Vec<char>`, tabs render to 8-column stops, long lines scroll
horizontally. Modified state is a buffer hash compared with the last
load/write, so entering insert mode and leaving without changes is not dirty.

Bounds (TM-DOS-119): 8 MiB buffer (load/insert/put/substitute), 16 MiB undo
history, 1 MiB compiled regex. Writes go through the session VFS.

Without a terminal, `vi` exits 1 (`vi: not a terminal`). `:cq` quits with
status 1.

## Decision: $EDITOR flows run vi in-process

Builtins that need an editor call `builtins::edit_file(ctx, path)`, which runs
the vi editor on the session terminal from inside the calling builtin (no
re-entry into the shell, no subprocess). It returns `None` outside a terminal
so callers keep their non-interactive behavior. `$VISUAL` then `$EDITOR` are
honored only to the extent of naming `vi`/`vim`/`nano` (any path, args ignored);
any other editor is reported as unavailable instead of silently swapping in
vi. A session sets `EDITOR=vi` when unset. Consumers: `git commit` without
`-m` (edits `.git/COMMIT_EDITMSG` with git's template, `#` lines dropped,
empty message or `:cq` aborts with exit 1). `git rebase -i` and `crontab -e`
are not consumers because neither command exists in bashkit.

## nano builtin

`builtins/nano.rs`, `terminal` feature, subset editor (L-TERM-005): modeless
typing, cursor keys and `^A ^E ^Y ^V`, `^O` (prompts for the file name,
pre-filled), `^X` (asks `Save modified buffer?` when dirty), `^K` cut (repeat
appends) and `^U` paste whole lines, `^W` search (wraps), `^G`, `^C`. Title
bar, status line and two help rows like GNU nano. Same bounds and VFS rules as
`vi`; control chars render in caret notation. `$EDITOR=nano` is accepted by
`builtins::edit_file`, so `git commit` can use it.

## less / more

`builtins/pager.rs`. Interactive only when the `Tty` extension is present
(i.e. inside a `Terminal`); otherwise `less` keeps its cat-like path and
`more` delegates to it, so `exec()`, `BashTool` and CLI callers can never
block on a pager. `more` is registered only with the `terminal` feature.
Content is read up front (VFS/stdin limits apply), wrapped to the terminal
width, with control characters rendered in caret notation so file content
cannot inject escape sequences into the host terminal. `less`: alternate
screen, paging/line/half-page/top/bottom keys, regex `/ ?` search with
`n N`, `-F`. `more`: normal screen, `--More--(NN%)`, space/Enter/q, prints
short input directly. Key decoding and the raw/alternate-screen guard are
shared with `vi` in `terminal/keys.rs`. Pagers cannot tell whether stdout is
redirected (L-TERM-004).

## man / watch

`man` (`builtins/man.rs`, always registered) is a `PlanDriver`: it runs
`CMD --help` for registered program builtins, `help CMD` for bash builtins
(`exit --help` would exit the shell; functions never run), and shows the
first non-empty successful text, in `less` via `pager::page_text` when a
`Tty` is present. No page: `No manual entry for CMD`, status 16. Decision:
no man page corpus; `--help` is the one source of truth per command.

`watch` (`builtins/pipeline.rs`) stays a one-shot notice outside a terminal.
With a `Tty` it returns a `PlanDriver` that enters the alternate screen
(cooked mode, so Ctrl-C still interrupts), runs `bash -c CMD` (or argv with
`-x`), redraws title + output clipped to the screen with control chars in
caret notation, sleeps the interval (floor 0.1 s) and repeats. Ctrl-C drops
the driver, whose `Drop` leaves the alternate screen. `-g` returns once the
output differs from the first run, which gives agents a "wait until changed"
primitive. Runs and sleeps count against the execution timeout (TM-DOS-120
only exempts input waits). `top` is not provided: there is no process table
worth showing (background jobs run synchronously).

## Tests

- Unit: `terminal::tests` (line discipline, prompts, resize, exit, timeout
  exclusion, Ctrl-C, transcript, activity, scrollback bounds) and
  `terminal::tool::tests` (key notation, multi-call vi edit, running/finish,
  exit, argument errors, definition). `builtins::vi::tests` (editing commands end-to-end
  through a `Terminal`), and `builtins::pager::tests` (paging, search, and
  `pagers_are_cat_like_without_terminal`).
- Integration: `tests/integration/terminal_tests.rs` (agent-style config edit,
  raw output replay, resize redraw, drop mid-edit) and `l_term_*` evidence
  tests.
- Examples: `examples/terminal_vi.rs` and `examples/terminal_record.rs`
  (asciicast recording from `take_output()`), both run in CI.
- Docs site: `site/public/casts/terminal-demo.cast` is that example's output,
  played on /docs/terminal by `site/src/components/TerminalRecording.astro`
  (lazy-loaded asciinema-player). Re-record when terminal output changes.
- CI step "Run terminal tests" runs these with `--features terminal`; the main
  test step compiles them out.

## Follow-ups

- C bindings. Shipped: Python `bashkit.Terminal`
  (`crates/bashkit-python/src/terminal.rs`, sync API on a per-instance
  current-thread runtime, wraps `TerminalTool`), NAPI `Terminal`
  (`crates/bashkit-js/src/terminal.rs`, async `runUntilIdle`/`call` that
  drive the session in 20 ms slices so sync `send("\x03")` and `screenText()`
  interleave with a long command), and browser wasm `Terminal`.
- Reader-backed stdin so `cat`/`head` block on the terminal too (lifts
  L-TERM-002 and helps L-CLI-002).
- Expose the device to custom builtins for host-defined TUIs; `stty`/`tput`.
- Snapshot/restore of a terminal session (shell state already snapshots).

## See also

- [Interactive Shell](interactive-shell.md), the CLI REPL on a real terminal
- [Builtin Commands](../foundations/builtins.md), builtin trait and execution extensions
- [Limitations](../operations/limitations.md), L-TERM-001..004
- [Threat Model](../security/threat-model.md), TM-DOS-119, TM-DOS-120
