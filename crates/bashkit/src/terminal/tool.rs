//! LLM tool wrapper around a [`Terminal`] session.
//!
//! Decisions:
//! - Not an implementation of the crate's `Tool` trait. That contract is one
//!   isolated shell per call (`{"commands": ...}` in, stdout/stderr out). A
//!   terminal is a session: state, open programs and the screen carry over
//!   between calls. Forcing it into `ToolRequest` would hide that, so this
//!   type has its own schema and keeps the same metadata names
//!   (`name`, `description`, `system_prompt`, `input_schema`,
//!   `output_schema`, `tool_definition`).
//! - Keys use Vim notation inside one string (`ihello<Esc>:wq<Enter>`). LLMs
//!   already know it, it keeps ordering between text and special keys, and it
//!   avoids raw control characters in JSON.
//! - Each call waits for the session to go idle, bounded by `wait_ms`
//!   (default 5 s, max 60 s). A command still running is reported, not
//!   killed; the next call (even with no input) keeps waiting, and `<C-c>`
//!   interrupts it.
//! - `wait_for` (regex) returns as soon as matching text appears in command
//!   output printed during the call. It matches command stdout/stderr only,
//!   never the echoed keystrokes or prompts, so `echo ready<Enter>` with
//!   `wait_for: "ready"` waits for the output line, not the typed command.
//!   Polls in 50 ms slices; the session keeps running between slices.
//! - `screen: "changes"` returns only rows that differ from the previous
//!   call (any mode updates the baseline), to keep long sessions cheap.

use std::time::Duration;

use serde_json::{Value, json};

use super::{CommandRecord, Terminal, TerminalActivity, TerminalSize, TerminalStatus};
use crate::BashBuilder;

/// Tool name used in schemas and definitions.
pub const TERMINAL_TOOL_NAME: &str = "terminal";
const DEFAULT_WAIT_MS: u64 = 5_000;
const MAX_WAIT_MS: u64 = 60_000;
/// Longest `input` string accepted per call.
// THREAT[TM-DOS-119]: tool input feeds the bounded terminal queue; reject
// oversized calls up front instead of silently truncating.
const MAX_INPUT_BYTES: usize = 64 * 1024;
/// Longest `wait_for` pattern, and the compiled-regex size cap.
// THREAT[TM-DOS-119]: the pattern is model-supplied; bound compile cost.
const MAX_PATTERN_BYTES: usize = 1024;
const MAX_REGEX_SIZE: usize = 1 << 20;
const WAIT_SLICE: Duration = Duration::from_millis(50);

const DESCRIPTION: &str = "Interactive bash terminal in a sandbox. Type keys, get the screen back. \
Use it for full-screen programs such as vi and less, or a shell session that persists between calls.";

/// Error from [`TerminalTool::call`]. The message is safe to show to the LLM.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TerminalToolError(String);

/// A persistent terminal session exposed as an LLM tool.
///
/// One instance is one session: keep it for the whole conversation.
///
/// ```rust
/// use bashkit::Bash;
/// use bashkit::terminal::TerminalTool;
/// use serde_json::json;
///
/// # #[tokio::main]
/// # async fn main() {
/// let mut tool = TerminalTool::new(Bash::builder());
/// let out = tool
///     .call(json!({"input": "echo hi<Enter>"}))
///     .await
///     .unwrap();
/// assert_eq!(out["activity"], "prompt");
/// assert_eq!(out["commands"][0]["output"], "hi\n");
/// assert!(out["screen"].as_str().unwrap().contains("$ echo hi\nhi"));
/// # }
/// ```
pub struct TerminalTool {
    terminal: Terminal,
    /// Screen rows at the previous report, for `screen: "changes"`.
    last_rows: Option<Vec<String>>,
}

/// How much of the screen a call returns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScreenMode {
    Full,
    Changes,
    None,
}

impl TerminalTool {
    /// Start a session on an 80x24 terminal.
    pub fn new(builder: BashBuilder) -> Self {
        Self::with_size(builder, TerminalSize::default())
    }

    /// Start a session on a terminal of the given size.
    pub fn with_size(builder: BashBuilder, size: TerminalSize) -> Self {
        Self {
            terminal: Terminal::with_size(builder, size),
            last_rows: None,
        }
    }

    /// The underlying terminal, for example to read files with
    /// [`Terminal::fs`] or render [`Terminal::take_output`].
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    /// Mutable access to the underlying terminal, to drive it directly
    /// between tool calls.
    pub fn terminal_mut(&mut self) -> &mut Terminal {
        &mut self.terminal
    }

    /// Tool name: `terminal`.
    pub fn name(&self) -> &str {
        TERMINAL_TOOL_NAME
    }

    /// One-sentence description for tool listings.
    pub fn description(&self) -> &str {
        DESCRIPTION
    }

    /// Terse usage guide for a system prompt.
    pub fn system_prompt(&self) -> String {
        "terminal: interactive sandboxed bash session that persists between calls. \
Send keys in `input` using Vim notation: text is typed as-is, <Enter> runs a line, \
<Esc>, <Tab>, <BS>, <Up>/<Down>/<Left>/<Right>, <PageUp>/<PageDown>, <C-c> interrupts, \
<C-d> ends input, <lt> types a literal '<'. Each call returns the screen, the activity \
(prompt, continuation, running, input, exited) and every command that finished with its exact \
output and exit code. vi and less work; quit vi with <Esc>:wq<Enter>, less with q. \
If activity is input, a command is asking a question (input_prompt holds it); type the answer \
and <Enter>. If activity is running, call again (input may be empty) to keep waiting, or send <C-c>. \
Set wait_for to a regex to return as soon as command output matches (for example a server's \
'listening' line); matched tells whether it did. Set screen to \"changes\" to get only the rows \
that changed since the last call, or \"none\" to skip the screen."
            .to_string()
    }

    /// JSON Schema for [`call`](Self::call) arguments.
    pub fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "input": {
                    "type": "string",
                    "description": "Keys to type, Vim notation: `ls -la<Enter>`, `ihello<Esc>:wq<Enter>`, `<C-c>`. May be empty to just wait and look."
                },
                "wait_ms": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": MAX_WAIT_MS,
                    "description": "How long to wait for the session to need input (default 5000)."
                },
                "wait_for": {
                    "type": "string",
                    "description": "Regex (multi-line: ^ and $ match at line ends). Return as soon as command output printed during this call matches it, even if the command is still running. Typed keys and prompts are not matched."
                },
                "screen": {
                    "type": "string",
                    "enum": ["full", "changes", "none"],
                    "description": "full (default): whole screen. changes: only rows that differ from the previous call, as screen_changes. none: no screen."
                }
            }
        })
    }

    /// JSON Schema for the value [`call`](Self::call) returns.
    pub fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["activity", "commands", "waiting_for_input"],
            "properties": {
                "screen": {"type": "string", "description": "Visible screen as plain text (screen = full)"},
                "screen_changes": {
                    "type": "array",
                    "description": "Rows that changed since the previous call (screen = changes)",
                    "items": {
                        "type": "object",
                        "required": ["row", "text"],
                        "properties": {
                            "row": {"type": "integer", "description": "0-based screen row"},
                            "text": {"type": "string"}
                        }
                    }
                },
                "cursor": {
                    "type": "object",
                    "description": "Cursor position, 0-based (screen = changes)",
                    "properties": {"row": {"type": "integer"}, "col": {"type": "integer"}}
                },
                "matched": {"type": "boolean", "description": "Whether wait_for matched (only when wait_for was given)"},
                "activity": {
                    "type": "string",
                    "enum": ["prompt", "continuation", "running", "input", "exited"],
                    "description": "What the session is doing. input: a running command (read, select, a y/n question) waits for a typed line"
                },
                "input_prompt": {"type": "string", "description": "The question on the cursor line while activity = input, such as 'Continue? [y/N] '"},
                "running_command": {"type": "string", "description": "Command line still running (activity = running)"},
                "exit_code": {"type": "integer", "description": "Shell exit code (activity = exited)"},
                "waiting_for_input": {"type": "boolean", "description": "False when wait_ms ran out while a command was still working"},
                "full_screen": {"type": "boolean", "description": "A full-screen program such as vi or less is open"},
                "commands": {
                    "type": "array",
                    "description": "Commands that finished during this call",
                    "items": {
                        "type": "object",
                        "required": ["command", "output", "exit_code"],
                        "properties": {
                            "command": {"type": "string"},
                            "output": {"type": "string"},
                            "exit_code": {"type": "integer"},
                            "output_truncated": {"type": "boolean"}
                        }
                    }
                }
            }
        })
    }

    /// OpenAI-compatible function definition.
    pub fn tool_definition(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.name(),
                "description": self.description(),
                "parameters": self.input_schema(),
            }
        })
    }

    /// Send the keys in `args.input`, run until the session needs input (or
    /// `wait_ms` passes), and report the screen and finished commands.
    pub async fn call(&mut self, args: Value) -> Result<Value, TerminalToolError> {
        let Some(args) = args.as_object() else {
            return Err(TerminalToolError("arguments must be a JSON object".into()));
        };
        let input = match args.get("input") {
            None | Some(Value::Null) => "",
            Some(Value::String(s)) => s.as_str(),
            Some(_) => return Err(TerminalToolError("`input` must be a string".into())),
        };
        if input.len() > MAX_INPUT_BYTES {
            return Err(TerminalToolError(format!(
                "`input` is longer than {MAX_INPUT_BYTES} bytes; send it in smaller parts"
            )));
        }
        let wait_ms = match args.get("wait_ms") {
            None | Some(Value::Null) => DEFAULT_WAIT_MS,
            Some(v) => v.as_u64().ok_or_else(|| {
                TerminalToolError("`wait_ms` must be a non-negative integer".into())
            })?,
        }
        .min(MAX_WAIT_MS);
        let wait_for = match args.get("wait_for") {
            None | Some(Value::Null) => None,
            Some(Value::String(p)) => Some(compile_pattern(p)?),
            Some(_) => return Err(TerminalToolError("`wait_for` must be a string".into())),
        };
        let mode = match args.get("screen") {
            None | Some(Value::Null) => ScreenMode::Full,
            Some(Value::String(m)) if m == "full" => ScreenMode::Full,
            Some(Value::String(m)) if m == "changes" => ScreenMode::Changes,
            Some(Value::String(m)) if m == "none" => ScreenMode::None,
            Some(_) => {
                return Err(TerminalToolError(
                    "`screen` must be \"full\", \"changes\" or \"none\"".into(),
                ));
            }
        };

        let bytes = parse_keys(input);
        let mark = self.terminal.tty().output_mark();
        if self.terminal.send(&bytes) < bytes.len() {
            return Err(TerminalToolError(
                "terminal input buffer is full; wait for the running command first".into(),
            ));
        }
        let wait = Duration::from_millis(wait_ms);
        let (waiting, matched) = match wait_for {
            None => (self.wait_idle(wait).await, None),
            Some(re) => {
                let (waiting, matched) = self.wait_match(wait, mark, &re).await;
                (waiting, Some(matched))
            }
        };
        let mut out = self.report(waiting, mode);
        if let Some(matched) = matched {
            out["matched"] = json!(matched);
        }
        Ok(out)
    }

    async fn wait_idle(&mut self, wait: Duration) -> bool {
        matches!(
            crate::time_compat::timeout(wait, self.terminal.run_until_idle()).await,
            Ok(TerminalStatus::Idle | TerminalStatus::Exited(_))
        )
    }

    /// Run until `re` matches output written since `mark`, the session needs
    /// input, or `wait` passes. Returns (waiting_for_input, matched).
    async fn wait_match(&mut self, wait: Duration, mark: u64, re: &regex::Regex) -> (bool, bool) {
        let deadline = crate::time_compat::Instant::now() + wait;
        loop {
            if self.output_matches(mark, re) {
                return (self.terminal.tty().is_idle(), true);
            }
            let left = deadline.saturating_duration_since(crate::time_compat::Instant::now());
            if left.is_zero() {
                return (false, false);
            }
            if self.wait_idle(left.min(WAIT_SLICE)).await {
                return (true, self.output_matches(mark, re));
            }
        }
    }

    fn output_matches(&self, mark: u64, re: &regex::Regex) -> bool {
        let raw = self.terminal.tty().output_since(mark);
        re.is_match(&strip_ansi(&String::from_utf8_lossy(&raw)))
    }

    fn report(&mut self, waiting: bool, mode: ScreenMode) -> Value {
        let mut out = json!({
            "waiting_for_input": waiting,
            "full_screen": self.terminal.is_alternate_screen(),
            "commands": self
                .terminal
                .take_transcript()
                .into_iter()
                .map(record_json)
                .collect::<Vec<_>>(),
        });
        let (activity, extra) = match self.terminal.activity() {
            TerminalActivity::Starting | TerminalActivity::Prompt => ("prompt", None),
            TerminalActivity::ContinuationPrompt => ("continuation", None),
            TerminalActivity::Running { command } => {
                if let Some(prompt) = self.terminal.input_prompt() {
                    out["input_prompt"] = json!(prompt);
                    ("input", Some(("running_command", json!(command))))
                } else {
                    ("running", Some(("running_command", json!(command))))
                }
            }
            TerminalActivity::Exited(code) => ("exited", Some(("exit_code", json!(code)))),
        };
        out["activity"] = json!(activity);
        if let Some((key, value)) = extra {
            out[key] = value;
        }
        let rows = self.terminal.screen_rows();
        match mode {
            ScreenMode::Full => out["screen"] = json!(self.terminal.screen_text()),
            ScreenMode::Changes => {
                let prev = self.last_rows.as_deref().unwrap_or(&[]);
                let resized = prev.len() != rows.len();
                let changes: Vec<Value> = rows
                    .iter()
                    .enumerate()
                    .filter(|(i, row)| resized || prev.get(*i) != Some(*row))
                    .map(|(i, row)| json!({"row": i, "text": row}))
                    .collect();
                let (row, col) = self.terminal.cursor();
                out["screen_changes"] = json!(changes);
                out["cursor"] = json!({"row": row, "col": col});
            }
            ScreenMode::None => {}
        }
        self.last_rows = Some(rows);
        out
    }
}

fn compile_pattern(pattern: &str) -> Result<regex::Regex, TerminalToolError> {
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(TerminalToolError(format!(
            "`wait_for` is longer than {MAX_PATTERN_BYTES} bytes"
        )));
    }
    regex::RegexBuilder::new(pattern)
        .size_limit(MAX_REGEX_SIZE)
        .multi_line(true)
        .build()
        .map_err(|e| TerminalToolError(format!("`wait_for` is not a valid regex: {e}")))
}

/// Drop terminal control sequences (CSI, OSC, two-byte ESC) and `\r` so
/// `wait_for` matches the text a reader sees.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {}
            '\x1b' => match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            c => out.push(c),
        }
    }
    out
}

fn record_json(r: CommandRecord) -> Value {
    let mut v = json!({
        "command": r.command,
        "output": r.output,
        "exit_code": r.exit_code,
    });
    if r.output_truncated {
        v["output_truncated"] = json!(true);
    }
    v
}

/// Translate Vim key notation into terminal input bytes. Unknown `<...>`
/// tokens are typed literally.
pub(crate) fn parse_keys(input: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('<') {
        out.extend_from_slice(&rest.as_bytes()[..start]);
        let after = &rest[start + 1..];
        let token = after
            .find('>')
            .map(|end| &after[..end])
            .filter(|t| !t.is_empty() && t.len() <= 10 && !t.contains('<'));
        match token.and_then(key_bytes) {
            Some(bytes) => {
                out.extend_from_slice(&bytes);
                rest = &after[token.map_or(0, str::len) + 1..];
            }
            None => {
                out.push(b'<');
                rest = after;
            }
        }
    }
    out.extend_from_slice(rest.as_bytes());
    out
}

fn key_bytes(token: &str) -> Option<Vec<u8>> {
    let lower = token.to_ascii_lowercase();
    let fixed: &[u8] = match lower.as_str() {
        "enter" | "cr" | "return" => b"\r",
        "esc" | "escape" => b"\x1b",
        "tab" => b"\t",
        "bs" | "backspace" => b"\x7f",
        "del" | "delete" => b"\x1b[3~",
        "space" => b" ",
        "lt" => b"<",
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        "home" => b"\x1b[H",
        "end" => b"\x1b[F",
        "pageup" => b"\x1b[5~",
        "pagedown" => b"\x1b[6~",
        _ => {
            let ctrl = lower.strip_prefix("c-")?;
            let [c] = ctrl.as_bytes() else {
                return None;
            };
            return match c {
                b'a'..=b'z' => Some(vec![c - b'a' + 1]),
                b'[' => Some(vec![0x1b]),
                _ => None,
            };
        }
    };
    Some(fixed.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bash;

    #[test]
    fn vim_notation_maps_to_bytes() {
        assert_eq!(parse_keys("ls<Enter>"), b"ls\r");
        assert_eq!(parse_keys("ihi<Esc>:wq<CR>"), b"ihi\x1b:wq\r");
        assert_eq!(parse_keys("<C-c><c-D>"), b"\x03\x04");
        assert_eq!(parse_keys("<Up><PageDown>"), b"\x1b[A\x1b[6~");
        assert_eq!(parse_keys("a <lt> b"), b"a < b");
    }

    #[test]
    fn unknown_or_unclosed_tokens_are_literal() {
        assert_eq!(parse_keys("echo <foo>"), b"echo <foo>");
        assert_eq!(parse_keys("a<b"), b"a<b");
        assert_eq!(parse_keys("x << EOF<Enter>"), b"x << EOF\r");
        assert_eq!(parse_keys("<C-1>"), b"<C-1>");
        assert_eq!(parse_keys("<>"), b"<>");
    }

    #[tokio::test]
    async fn edits_a_file_in_vi_over_several_calls() {
        let mut tool = TerminalTool::new(Bash::builder());
        let out = tool
            .call(json!({"input": "vi /tmp/n.txt<Enter>"}))
            .await
            .unwrap();
        assert_eq!(out["activity"], "running");
        assert_eq!(out["running_command"], "vi /tmp/n.txt");
        assert_eq!(out["full_screen"], true);
        assert_eq!(out["waiting_for_input"], true);

        let out = tool
            .call(json!({"input": "ihello<Esc>:wq<Enter>"}))
            .await
            .unwrap();
        assert_eq!(out["activity"], "prompt");
        assert_eq!(out["full_screen"], false);
        assert_eq!(out["commands"][0]["command"], "vi /tmp/n.txt");
        assert_eq!(out["commands"][0]["exit_code"], 0);
        let saved = tool
            .terminal()
            .fs()
            .read_file("/tmp/n.txt".as_ref())
            .await
            .unwrap();
        assert_eq!(saved, b"hello\n");
    }

    #[tokio::test]
    async fn slow_command_reports_running_then_finishes() {
        let mut tool = TerminalTool::new(Bash::builder());
        let out = tool
            .call(json!({"input": "sleep 0.2; echo done<Enter>", "wait_ms": 20}))
            .await
            .unwrap();
        assert_eq!(out["waiting_for_input"], false);
        assert_eq!(out["activity"], "running");
        let out = tool.call(json!({"wait_ms": 2000})).await.unwrap();
        assert_eq!(out["waiting_for_input"], true);
        assert_eq!(out["commands"][0]["output"], "done\n");
    }

    #[tokio::test]
    async fn exit_is_reported() {
        let mut tool = TerminalTool::new(Bash::builder());
        let out = tool.call(json!({"input": "exit 5<Enter>"})).await.unwrap();
        assert_eq!(out["activity"], "exited");
        assert_eq!(out["exit_code"], 5);
    }

    #[tokio::test]
    async fn bad_arguments_are_user_facing_errors() {
        let mut tool = TerminalTool::new(Bash::builder());
        assert!(tool.call(json!("ls")).await.is_err());
        assert!(tool.call(json!({"input": 3})).await.is_err());
        assert!(
            tool.call(json!({"input": "", "wait_ms": -1}))
                .await
                .is_err()
        );
        let big = "a".repeat(MAX_INPUT_BYTES + 1);
        let err = tool.call(json!({"input": big})).await.unwrap_err();
        assert!(err.to_string().contains("smaller parts"));
    }

    #[tokio::test]
    async fn wait_for_returns_on_match_while_running() {
        let mut tool = TerminalTool::new(Bash::builder());
        let started = std::time::Instant::now(); // std-time-ok: native-only test
        let out = tool
            .call(json!({
                "input": "for i in 1 2 3; do echo tick$i; sleep 1; done<Enter>",
                "wait_for": "tick[2]",
                "wait_ms": 10000
            }))
            .await
            .unwrap();
        assert_eq!(out["matched"], true);
        assert_eq!(out["activity"], "running");
        assert_eq!(out["waiting_for_input"], false);
        assert!(started.elapsed() < std::time::Duration::from_millis(2500));
        assert!(out["screen"].as_str().unwrap().contains("tick2"));
        let out = tool.call(json!({"input": "<C-c>"})).await.unwrap();
        assert_eq!(out["activity"], "prompt");
    }

    #[tokio::test]
    async fn wait_for_ignores_typed_echo() {
        let mut tool = TerminalTool::new(Bash::builder());
        let out = tool
            .call(json!({"input": "echo ready<Enter>", "wait_for": "^ready$"}))
            .await
            .unwrap();
        assert_eq!(out["matched"], true);
        let out = tool
            .call(json!({"input": "true ready<Enter>", "wait_for": "ready"}))
            .await
            .unwrap();
        assert_eq!(out["matched"], false);
        assert_eq!(out["waiting_for_input"], true);
    }

    #[tokio::test]
    async fn wait_for_times_out_without_match() {
        let mut tool = TerminalTool::new(Bash::builder());
        let out = tool
            .call(json!({"input": "sleep 5<Enter>", "wait_for": "never", "wait_ms": 100}))
            .await
            .unwrap();
        assert_eq!(out["matched"], false);
        assert_eq!(out["activity"], "running");
        assert!(out.get("matched").is_some());
        let out = tool.call(json!({"input": "<C-c>"})).await.unwrap();
        assert!(out.get("matched").is_none());
    }

    #[test]
    fn wait_for_rejects_bad_patterns() {
        assert!(
            compile_pattern("(")
                .unwrap_err()
                .to_string()
                .contains("not a valid regex")
        );
        assert!(compile_pattern(&"a".repeat(MAX_PATTERN_BYTES + 1)).is_err());
        assert!(compile_pattern("a{1000}{1000}").is_err());
    }

    #[test]
    fn strip_ansi_keeps_visible_text() {
        assert_eq!(strip_ansi("\x1b[1;31mred\x1b[0m ok\r\n"), "red ok\n");
        assert_eq!(strip_ansi("\x1b]0;title\x07x\x1b]2;t\x1b\\y\x1b7z"), "xyz");
    }

    #[tokio::test]
    async fn screen_changes_returns_only_changed_rows() {
        let mut tool = TerminalTool::new(Bash::builder());
        let out = tool
            .call(json!({"input": "echo one<Enter>", "screen": "changes"}))
            .await
            .unwrap();
        // First call: every row is new.
        assert_eq!(out["screen_changes"].as_array().unwrap().len(), 24);
        assert!(out.get("screen").is_none());
        let out = tool
            .call(json!({"input": "echo two<Enter>", "screen": "changes"}))
            .await
            .unwrap();
        let changes = out["screen_changes"].as_array().unwrap();
        let texts: Vec<_> = changes
            .iter()
            .map(|c| c["text"].as_str().unwrap())
            .collect();
        assert_eq!(texts, ["$ echo two", "two", "$"]);
        assert_eq!(changes[0]["row"], 2);
        assert_eq!(out["cursor"], json!({"row": 4, "col": 2}));
        let out = tool.call(json!({"screen": "none"})).await.unwrap();
        assert!(out.get("screen").is_none() && out.get("screen_changes").is_none());
        let out = tool.call(json!({"screen": "changes"})).await.unwrap();
        assert_eq!(out["screen_changes"], json!([]));
        assert!(tool.call(json!({"screen": "diff"})).await.is_err());
        assert!(tool.call(json!({"wait_for": 1})).await.is_err());
    }

    #[tokio::test]
    async fn question_is_reported_as_input() {
        let mut tool = TerminalTool::new(Bash::builder());
        let out = tool
            .call(json!({"input": "read -p 'Overwrite? [y/N] ' a; echo a=$a<Enter>"}))
            .await
            .unwrap();
        assert_eq!(out["activity"], "input");
        assert_eq!(out["input_prompt"], "Overwrite? [y/N] ");
        assert_eq!(out["waiting_for_input"], true);
        let out = tool.call(json!({"input": "y<Enter>"})).await.unwrap();
        assert_eq!(out["activity"], "prompt");
        assert!(out.get("input_prompt").is_none());
        assert_eq!(out["commands"][0]["output"], "a=y\n");
        // vi is running, not asking a line question.
        let out = tool.call(json!({"input": "vi<Enter>"})).await.unwrap();
        assert_eq!(out["activity"], "running");
    }

    #[test]
    fn definition_matches_schema() {
        let tool = TerminalTool::new(Bash::builder());
        let def = tool.tool_definition();
        assert_eq!(def["function"]["name"], "terminal");
        assert_eq!(def["function"]["parameters"], tool.input_schema());
        assert!(tool.system_prompt().starts_with("terminal:"));
    }
}
