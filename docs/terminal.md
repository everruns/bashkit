# In-process terminal

Bashkit can run an interactive shell session on an in-memory terminal. You send
keystrokes and read back what the screen shows, as plain text, without a real
PTY, process, or host terminal. Full-screen programs work too: the built-in
`vi` edits files in the virtual filesystem.

This recording is the session's real output: every keystroke went through
`send()` and every byte shown came from `take_output()`
([how it was recorded](#recording-a-session)).

<div data-terminal-recording="/casts/terminal-demo.cast"></div>

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

### Recording a session

`take_output()` is a byte-exact stream, so a session can be recorded and
replayed in any terminal player.
[`crates/bashkit/examples/terminal_record.rs`](../crates/bashkit/examples/terminal_record.rs)
types a scenario key by key (a `vi` edit and `less` paging) and writes an
[asciicast v2](https://docs.asciinema.org/manual/asciicast/v2/) file:

```bash
cargo run --example terminal_record --features terminal -- demo.cast
asciinema play demo.cast
```

The recording at the top of this page is that file, served from
`site/public/casts/terminal-demo.cast`. Re-record it after changing terminal
output by running the example with that path.

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
| `history_text()` | The screen plus up to 1000 lines of scrollback above it, same format |
| `take_transcript()` | Commands finished since the last call: command line, exact output, exit code |
| `activity()` | `Prompt`, `ContinuationPrompt`, `Running { command }` (for example an open `vi`) or `Exited(code)` |
| `cursor()` | Cursor `(row, col)`, zero-based |
| `is_alternate_screen()` | `true` while a full-screen program such as `vi` is open |
| `take_output()` | Raw bytes (with escape sequences) produced since the last call, for a renderer like xterm.js |
| `fs()` | The session's virtual filesystem, to read files commands or `vi` wrote |
| `exit_code()` | The shell's exit code once it has exited |

For most agent use, `screen_text()` plus `fs()` is all you need: the screen
shows what happened, and the filesystem holds what was saved.

When an agent needs exact results instead of screen text, use the transcript.
Each `CommandRecord` holds the command line, its combined stdout and stderr
with plain `\n` line endings, and its exit code (`130` after Ctrl-C, `2` for a
syntax error). Output that scrolled off the screen is still there, and no
prompt scraping is needed:

```rust
let mut term = Terminal::new(Bash::builder());
term.send("ls /nope\r");
term.run_until_idle().await;

let record = &term.take_transcript()[0];
assert_eq!(record.command, "ls /nope");
assert_ne!(record.exit_code, 0);
assert_eq!(term.activity(), TerminalActivity::Prompt);
```

Full-screen programs (`vi`, `less`) draw straight to the terminal, so their
screens are not in the transcript; read them with `screen_text()`. Each record
keeps up to 64 KiB of output (`output_truncated` says when more was dropped),
and untaken records are capped at 1 MiB in total, oldest first.

### Sending keys

| Key | Bytes |
|-----|-------|
| Enter | `\r` |
| Escape | `\x1b` |
| Backspace | `\x7f` |
| Ctrl-C / Ctrl-D | `\x03` / `\x04` |
| Arrow up/down/right/left | `\x1b[A` `\x1b[B` `\x1b[C` `\x1b[D` |

At the prompt the terminal behaves like a normal line-mode terminal: typed
characters echo, Left/Right, Home/End (or Ctrl-A/Ctrl-E) move the cursor,
Backspace, Delete, Ctrl-U (kill to start), Ctrl-K (kill to end) and Ctrl-W
(kill word) edit the line, Up/Down recall earlier commands from this session,
Ctrl-C discards the line, and an incomplete command (`for i in 1 2; do`) shows
the `PS2` prompt and waits for more lines. Shell state persists between lines,
as in any `Bash` session. `PS1` and `PS2` are honoured (`\u \h \w \W \$`); the
default prompt is `$ `.

`[ -t 0 ]` is true inside the session, and `COLUMNS`, `LINES` and `TERM` are
set. `resize()` changes the size; a running `vi` redraws.

## As an LLM tool

`TerminalTool` wraps one session as a tool an agent can call repeatedly. Keys
go in one string in Vim notation, and each call returns the screen, what the
session is doing, and the commands that finished during the call:

```rust
use bashkit::Bash;
use bashkit::terminal::TerminalTool;
use serde_json::json;

let mut tool = TerminalTool::new(Bash::builder());
// Register tool.tool_definition() (OpenAI function format) and
// tool.system_prompt() with your model, then forward its calls:
let out = tool.call(json!({"input": "vi notes.txt<Enter>"})).await?;
// out["activity"] == "running", out["full_screen"] == true
let out = tool.call(json!({"input": "ihello<Esc>:wq<Enter>"})).await?;
// out["activity"] == "prompt", out["commands"][0]["exit_code"] == 0
```

| `input` token | Key |
|---------------|-----|
| plain text | typed as-is |
| `<Enter>` `<Esc>` `<Tab>` `<BS>` `<Del>` `<Space>` | the named key |
| `<Up>` `<Down>` `<Left>` `<Right>` `<Home>` `<End>` `<PageUp>` `<PageDown>` | cursor keys |
| `<C-c>`, `<C-d>`, any `<C-x>` | Ctrl plus a letter |
| `<lt>` | a literal `<` |

Anything else in angle brackets, such as `<foo>` or a heredoc's `<<`, is typed
literally.

The result has `screen`, `activity` (`prompt`, `continuation`, `running` or
`exited`), `running_command` or `exit_code` when they apply, `full_screen`
(true while `vi` or `less` is open), `waiting_for_input`, and `commands` (the
transcript records finished during the call). Each call waits up to `wait_ms`
(default 5 s, at most 60 s) for the session to need input. A command still
running after that is reported with `waiting_for_input: false`, not killed:
call again, with empty input, to keep waiting, or send `<C-c>`. One
`TerminalTool` is one session, so keep it for the whole conversation.
`call` takes at most 64 KiB of `input` per call.

Two optional arguments keep agent loops short:

- `wait_for`: a regex. The call returns as soon as output printed by commands
  during the call matches, even while the command keeps running, and sets
  `matched`. Typed keys and prompts are not matched, so
  `{"input": "./serve.sh<Enter>", "wait_for": "listening on"}` returns when the
  server prints its line. `^` and `$` match at line ends.
- `screen`: `"full"` (default) returns `screen`; `"changes"` returns
  `screen_changes` (`[{row, text}]`, only rows that differ from the previous
  call) and `cursor`; `"none"` returns no screen.

It is not a `BashTool`: that tool runs each call in a fresh shell, while a
terminal keeps the shell, open programs and the screen between calls.

## From Python

The `bashkit` Python package ships the terminal as `bashkit.Terminal`, with
the same calls as the Rust type plus the agent-style `call`:

```python
from bashkit import Terminal

t = Terminal(rows=24, cols=80, cwd="/tmp")
out = t.call("vi notes.txt<Enter>")            # Vim key notation
assert out["activity"] == "running" and out["full_screen"]
out = t.call("ihello<Esc>:wq<Enter>")
assert out["commands"][0]["exit_code"] == 0
assert t.fs().read_file("/tmp/notes.txt") == b"hello\n"

# Low level: raw keys, bounded waits, screen and transcript.
t.send("seq 1 100\r")
t.run_until_idle(timeout=5)                    # "idle", "exited" or "timeout"
print(t.screen_text(), t.history_text(), t.take_transcript(), t.activity())
```

`tool_definition()` and `system_prompt()` return the same tool metadata as
`TerminalTool`. Calls are synchronous and release the GIL while the session
runs; `timeout` bounds the wait.

## From JavaScript

The `@everruns/bashkit` package (Node, Bun, Deno) exports the same class:

```typescript
import { Terminal } from "@everruns/bashkit";

const term = new Terminal({ rows: 24, cols: 80, cwd: "/tmp" });
let out = await term.call("vi notes.txt<Enter>"); // Vim key notation
out = await term.call("ihello<Esc>:wq<Enter>");
console.log(out.commands[0].exit_code); // 0
console.log((await term.readFile("/tmp/notes.txt")).toString()); // "hello\n"

// Low level: raw keys, bounded waits, screen and transcript.
term.send("sleep 10; echo done\r");
await term.runUntilIdle(100); // "idle", "exited" or "timeout"
term.send("\x03"); // Ctrl-C while the command runs
await term.runUntilIdle();
console.log(term.screenText(), term.takeTranscript(), term.activity());
```

`runUntilIdle` and `call` are async and yield between short slices, so
`send`, `screenText` and the other sync methods work while a command runs.
`toolDefinition()` and `systemPrompt()` return the `TerminalTool` metadata.

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

## In the browser

The npm package [`@everruns/bashkit-wasm`](start-browser.md) exposes the same
session as a `Terminal` class with `send`, `runUntilIdle`, `takeOutput`,
`screenText`, `resize` and `fs`. Wire `takeOutput()` into
[xterm.js](https://xtermjs.org) and forward its `onData` keystrokes to `send`.
The [playground](https://bashkit.sh/playground) on bashkit.sh is built exactly
this way; its source is `site/src/playground/client.ts`.

## Limits and security

Everything runs inside the normal sandbox: the same virtual filesystem, the
same execution limits, no host processes. A few terminal-specific rules apply:

- **Timeouts.** Time spent waiting for keystrokes does not count against
  `ExecutionLimits::timeout`, so a `vi` session is not killed while an agent
  thinks. CPU work and `sleep` still count.
- **Bounded buffers.** Unread input is capped at 1 MiB (`send` returns how many
  bytes it accepted), retained raw output at the newest 4 MiB, and the `vi`
  buffer at 8 MiB.
- **Ctrl-C** stops a running command right away, even inside `sleep`, and
  sets `$?` to 130.
- **Command stdin is not the terminal.** `read` with no input gets end-of-file
  instead of waiting for typed text. Only `vi` reads keystrokes directly.

See TM-DOS-119 and TM-DOS-120 in the [threat model](../crates/bashkit/docs/threat-model.md).

## See also

- [Browser (WASM)](start-browser.md): the `Terminal` class in `@everruns/bashkit-wasm`
- [CLI](cli.md): the `bashkit` binary's own interactive REPL on your real terminal
- [Snapshotting](snapshotting.md): persist and restore a session's state
- [Virtual filesystem](filesystem.md): where `vi` reads and writes files
