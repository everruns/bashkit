# In-process terminal

Bashkit can run an interactive shell session on an in-memory terminal. You send
keystrokes and read back what the screen shows, as plain text, without a real
PTY, process, or host terminal. Full-screen programs work too: the built-in
`vi` edits files in the virtual filesystem.

Use it when a one-shot `exec()` is not enough:

- an LLM agent that should operate a shell the way a person does, including
  editing a file in `vi`, and read the screen after each step;
- a browser or desktop app that renders a terminal (for example with xterm.js)
  backed by the sandbox;
- tests that assert on what a user would see.

The terminal is behind the `terminal` cargo feature, off by default:

```bash
cargo add bashkit --features terminal
```

## Quick start

```rust
use bashkit::Bash;
use bashkit::terminal::{Terminal, TerminalSize, TerminalStatus};

#[tokio::main]
async fn main() {
    let mut term = Terminal::with_size(Bash::builder(), TerminalSize::new(24, 80));
    term.run_until_idle().await;            // draws the first prompt

    term.send("vi /tmp/notes.txt\r");       // type a command, press Enter
    term.run_until_idle().await;            // runs until vi waits for keys
    println!("{}", term.screen_text());     // the editor screen, as text

    term.send("ihello\x1b:wq\r");           // insert, Escape, save and quit
    term.run_until_idle().await;

    let saved = term.fs().read_file("/tmp/notes.txt".as_ref()).await.unwrap();
    assert_eq!(saved, b"hello\n");

    term.send("exit\r");
    assert_eq!(term.run_until_idle().await, TerminalStatus::Exited(0));
}
```

A runnable version lives in
[`crates/bashkit/examples/terminal_vi.rs`](../crates/bashkit/examples/terminal_vi.rs):
`cargo run --example terminal_vi --features terminal`.

## How it works

`Terminal` owns a `Bash` and a virtual terminal device. Nothing runs in the
background:

1. `send(bytes)` queues input, exactly as if typed.
2. `run_until_idle()` runs the session until it is blocked waiting for input
   with nothing queued, then returns `TerminalStatus::Idle`, or
   `TerminalStatus::Exited(code)` once the shell exits (`exit`, or Ctrl-D on an
   empty line).
3. You read the result and decide what to type next.

"Idle" is the moment a person would look at the screen, so an agent loop is
simply send, run, read. `run_until_idle()` is cancellation-safe: wrap it in
`tokio::time::timeout` to stop waiting on a long command, send Ctrl-C, and call
it again.

### Reading results

| Method | Returns |
|--------|---------|
| `screen_text()` | The visible screen as plain text, one line per row, trailing blanks trimmed |
| `cursor()` | Cursor `(row, col)`, zero-based |
| `is_alternate_screen()` | `true` while a full-screen program such as `vi` is open |
| `take_output()` | Raw bytes (with escape sequences) produced since the last call, for a renderer like xterm.js |
| `fs()` | The session's virtual filesystem, to read files commands or `vi` wrote |
| `exit_code()` | The shell's exit code once it has exited |

For most agent use, `screen_text()` plus `fs()` is all you need: the screen
shows what happened, and the filesystem holds what was saved.

### Sending keys

| Key | Bytes |
|-----|-------|
| Enter | `\r` |
| Escape | `\x1b` |
| Backspace | `\x7f` |
| Ctrl-C / Ctrl-D | `\x03` / `\x04` |
| Arrow up/down/right/left | `\x1b[A` `\x1b[B` `\x1b[C` `\x1b[D` |

At the prompt the terminal behaves like a normal line-mode terminal: typed
characters echo, Backspace, Ctrl-U (kill line) and Ctrl-W (kill word) edit the
line, Ctrl-C discards it, and an incomplete command (`for i in 1 2; do`) shows
the `PS2` prompt and waits for more lines. Shell state persists between lines,
as in any `Bash` session. `PS1` and `PS2` are honoured (`\u \h \w \W \$`); the
default prompt is `$ `.

`[ -t 0 ]` is true inside the session, and `COLUMNS`, `LINES` and `TERM` are
set. `resize()` changes the size; a running `vi` redraws.

## vi

`vi [FILE]` opens the editor on the alternate screen; quitting restores the
shell screen. It supports:

- **Modes:** normal, insert, and the `:` / `/` command line.
- **Movement:** `h j k l`, arrows, `w b e`, `0 ^ $`, `gg`, `G`, `NG`, `:N`,
  Enter, `+ -`. Counts work (`3j`, `2dd`).
- **Editing:** `i a I A o O`, `x X`, `dd dw de db d$ d0 D`, `cc cw C s S`,
  `yy yw Y`, `p P`, `r`, `J`, `u`, Ctrl-R.
- **Search and replace:** `/pattern`, `n N`, `:s/pat/rep/`, `:%s/pat/rep/g`
  (regex patterns; `&` in the replacement is the match).
- **Files:** `:w`, `:w FILE`, `:q`, `:q!`, `:wq`, `:x`, `ZZ`, `ZQ`.
  `:q` refuses to discard unsaved changes, as in vim.

Not supported: visual mode, named registers, macros, splits, vimrc, and `:!`
(no shell escape from inside the editor). See L-TERM-001 in the
[limitations](../knowledge/operations/limitations.md#terminal).

`vi` needs a terminal. Under plain `Bash::exec()` it exits 1 with
`vi: not a terminal`.

## less and more

`less` and `more` page interactively inside a terminal session, from files or
a pipe (`git log | less`, `seq 1 1000 | more`).

- **less** uses the alternate screen. Keys: `q` quit, space/`f`/PageDown next
  page, `b`/PageUp previous page, `j`/Enter/Down and `k`/Up by line, `d`/`u`
  half page, `g`/`G` top/bottom, `/pattern` and `?pattern` search (regex),
  `n`/`N` repeat. The status line shows the file name, `:` or `(END)`. `-F`
  prints input that fits on one screen and exits.
- **more** scrolls on the normal screen with a `--More--(NN%)` prompt: space
  for the next page, Enter for the next line, `q` to stop. Input that fits on
  one screen is printed directly.

Control characters in content show in caret notation (`^[`), so a file cannot
send escape sequences to your terminal.

Outside a terminal session (`Bash::exec()`, `BashTool`, the CLI), `less` and
`more` behave like `cat` and never wait for input, so existing scripts and
tools are unaffected. Inside a session they page even when stdout is
redirected (`less file > out`).

## Limits and security

Everything runs inside the normal sandbox: the same virtual filesystem, the
same execution limits, no host processes. A few terminal-specific rules apply:

- **Timeouts.** Time spent waiting for keystrokes does not count against
  `ExecutionLimits::timeout`, so a `vi` session is not killed while an agent
  thinks. CPU work and `sleep` still count.
- **Bounded buffers.** Unread input is capped at 1 MiB (`send` returns how many
  bytes it accepted), retained raw output at the newest 4 MiB, and the `vi`
  buffer at 8 MiB.
- **Ctrl-C** stops a running command at the next command boundary, so a single
  builtin such as `sleep 5` finishes first.
- **Command stdin is not the terminal.** `read` with no input gets end-of-file
  instead of waiting for typed text. Only `vi` reads keystrokes directly.

See TM-DOS-119 and TM-DOS-120 in the [threat model](../crates/bashkit/docs/threat-model.md).

## See also

- [CLI](cli.md): the `bashkit` binary's own interactive REPL on your real terminal
- [Snapshotting](snapshotting.md): persist and restore a session's state
- [Virtual filesystem](filesystem.md): where `vi` reads and writes files
