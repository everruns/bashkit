// Playground runtime: xterm.js wired to @everruns/bashkit-wasm.
//
// Decisions:
// - Loaded only by /playground through a dynamic import, so xterm.js and the
//   WebAssembly module never touch any other page's load.
// - Two modes. When the installed package exports `Terminal` (bundle built
//   with the `terminal` cargo feature), keystrokes go straight to an in-memory
//   PTY and full-screen programs (vi, less, more) work. Older packages lack it,
//   so we fall back to a small line editor over `Bash.execute`. The fallback
//   goes away once the site pins a release that ships `Terminal`.
// - The session is pull-driven: after each keystroke batch we await
//   `runUntilIdle()` and flush `takeOutput()` into xterm. While a command
//   runs, output is flushed every animation frame so `sleep` loops stream.

import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import * as bashkit from "@everruns/bashkit-wasm";

export interface Playground {
  /** "terminal" (full PTY) or "line" (fallback line editor). */
  mode: "terminal" | "line";
  /** Type a command as if the user entered it. */
  run(command: string): void;
  focus(): void;
}

const THEME = {
  background: "#0d1117",
  foreground: "#d1d5db",
  cursor: "#3fb950",
  selectionBackground: "#264f78",
  black: "#484f58",
  red: "#ff7b72",
  green: "#3fb950",
  yellow: "#d29922",
  blue: "#58a6ff",
  magenta: "#bc8cff",
  cyan: "#39c5cf",
  white: "#b1bac4",
  brightBlack: "#6e7681",
  brightRed: "#ffa198",
  brightGreen: "#56d364",
  brightYellow: "#e3b341",
  brightBlue: "#79c0ff",
  brightMagenta: "#d2a8ff",
  brightCyan: "#56d4dd",
  brightWhite: "#f0f6fc",
};

const SEED_FILES: Record<string, string> = {
  "/home/user/README.md": [
    "# Welcome to the Bashkit playground",
    "",
    "Everything here runs in your browser: a sandboxed bash",
    "interpreter compiled to WebAssembly, with a virtual filesystem.",
    "Nothing you type leaves this page.",
    "",
    "Try:",
    "  ls -la",
    "  cat data.json | jq '.tools[] | select(.stars > 2) | .name'",
    "  vi notes.txt          # :wq to save, :q! to quit",
    "  seq 1 200 | less      # q to quit",
    "",
  ].join("\n"),
  "/home/user/data.json":
    JSON.stringify(
      {
        tools: [
          { name: "grep", stars: 5 },
          { name: "awk", stars: 4 },
          { name: "sed", stars: 3 },
          { name: "jq", stars: 5 },
          { name: "ed", stars: 1 },
        ],
      },
      null,
      2,
    ) + "\n",
};

const BASH_OPTIONS = {
  username: "user",
  hostname: "bashkit",
  cwd: "/home/user",
  env: { HOME: "/home/user", PS1: "\\u@\\h:\\w\\$ " },
  files: SEED_FILES,
};

const BANNER =
  "\x1b[1;32mbashkit\x1b[0m playground\r\n" +
  "\x1b[2mSandboxed bash in WebAssembly.\x1b[0m\r\n" +
  "Type \x1b[1mcat README.md\x1b[0m to get started.\r\n\r\n";

export async function startPlayground(mount: HTMLElement): Promise<Playground> {
  await bashkit.initBashkit();

  const xterm = new XTerm({
    theme: THEME,
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace',
    fontSize: 14,
    cursorBlink: true,
    convertEol: false,
    scrollback: 2000,
  });
  const fit = new FitAddon();
  xterm.loadAddon(fit);
  mount.replaceChildren();
  xterm.open(mount);
  fit.fit();
  xterm.write(BANNER);

  const TerminalCtor = (bashkit as Record<string, unknown>).Terminal as
    | (new (options: object) => WasmTerminal)
    | undefined;
  const playground = TerminalCtor
    ? attachTerminal(xterm, new TerminalCtor({ ...BASH_OPTIONS, rows: xterm.rows, cols: xterm.cols }))
    : attachLineEditor(xterm, new bashkit.Bash(BASH_OPTIONS));

  const observer = new ResizeObserver(() => fit.fit());
  observer.observe(mount);
  xterm.focus();
  return playground;
}

// --- Full PTY mode ---------------------------------------------------------

interface WasmTerminal {
  send(input: string): number;
  runUntilIdle(): Promise<{ status: "idle" } | { status: "exited"; exitCode: number }>;
  takeOutput(): Uint8Array;
  resize(rows: number, cols: number): void;
}

function attachTerminal(xterm: XTerm, term: WasmTerminal): Playground {
  let running = false;
  let again = false;
  let exited = false;

  const flush = () => {
    const bytes = term.takeOutput();
    if (bytes.length > 0) xterm.write(bytes);
  };

  async function pump() {
    if (running) {
      again = true;
      return;
    }
    running = true;
    let frame = 0;
    const tick = () => {
      flush();
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    try {
      do {
        again = false;
        const status = await term.runUntilIdle();
        if (status.status === "exited") {
          exited = true;
          flush();
          xterm.write(
            `\r\n\x1b[2m[session exited with code ${status.exitCode}; reload the page to start again]\x1b[0m\r\n`,
          );
          break;
        }
      } while (again);
    } finally {
      cancelAnimationFrame(frame);
      flush();
      running = false;
    }
  }

  xterm.onData((data) => {
    if (exited) return;
    term.send(data);
    void pump();
  });
  xterm.onResize(({ rows, cols }) => {
    term.resize(rows, cols);
    void pump();
  });
  void pump();

  return {
    mode: "terminal",
    run(command) {
      if (exited) return;
      // Ctrl-U clears whatever is half-typed on the prompt first.
      term.send(`\x15${command}\r`);
      void pump();
    },
    focus: () => xterm.focus(),
  };
}

// --- Fallback line-editor mode ---------------------------------------------

interface WasmBash {
  execute(commands: string): Promise<{ stdout: string; stderr: string }>;
}

function attachLineEditor(xterm: XTerm, bash: WasmBash): Playground {
  const history: string[] = [];
  let historyIndex = 0;
  let line = "";
  let busy = false;

  const crlf = (s: string) => s.replace(/\r?\n/g, "\r\n");

  async function prompt() {
    let ps1 = "$ ";
    try {
      const r = await bash.execute("pwd");
      const cwd = r.stdout.trim().replace(/^\/home\/user(?=\/|$)/, "~");
      ps1 = `\x1b[1;32muser@bashkit\x1b[0m:\x1b[1;34m${cwd}\x1b[0m$ `;
    } catch {
      // keep the plain prompt
    }
    xterm.write(ps1);
  }

  function replaceLine(next: string) {
    xterm.write("\b \b".repeat(line.length));
    line = next;
    xterm.write(line);
  }

  async function submit(command: string) {
    xterm.write("\r\n");
    if (command.trim() !== "") {
      history.push(command);
      busy = true;
      try {
        const r = await bash.execute(command);
        xterm.write(crlf(r.stdout));
        if (r.stderr) xterm.write(`\x1b[31m${crlf(r.stderr)}\x1b[0m`);
      } catch (err) {
        xterm.write(`\x1b[31m${crlf(String(err))}\x1b[0m\r\n`);
      }
      busy = false;
    }
    historyIndex = history.length;
    line = "";
    await prompt();
  }

  xterm.onData((data) => {
    if (busy) return;
    // Escape sequences (arrow keys etc.) arrive as one chunk.
    if (data.startsWith("\x1b")) {
      if (data === "\x1b[A" && historyIndex > 0) {
        historyIndex -= 1;
        replaceLine(history[historyIndex]);
      } else if (data === "\x1b[B" && historyIndex < history.length) {
        historyIndex += 1;
        replaceLine(history[historyIndex] ?? "");
      }
      return;
    }
    for (const ch of data) {
      if (ch === "\r") {
        void submit(line);
        return;
      }
      if (ch === "\x7f" || ch === "\b") {
        if (line.length > 0) {
          line = line.slice(0, -1);
          xterm.write("\b \b");
        }
      } else if (ch === "\x03") {
        xterm.write("^C\r\n");
        line = "";
        void prompt();
      } else if (ch === "\x15") {
        replaceLine("");
      } else if (ch === "\x0c") {
        xterm.clear();
      } else if (ch >= " ") {
        line += ch;
        xterm.write(ch);
      }
    }
  });

  void prompt();

  return {
    mode: "line",
    run(command) {
      if (busy) return;
      replaceLine(command);
      void submit(command);
    },
    focus: () => xterm.focus(),
  };
}
