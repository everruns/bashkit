// Agent loop: send messages → get response → execute tool calls → repeat
// Uses Bash directly (not BashTool) for persistent VFS across tool calls
// BashTool used only for input_schema/system_prompt introspection
// Every eval Bash gets git, python3 (CPython) and sqlite3; tasks may carry a
// hidden `verify` probe run after the loop. See knowledge/operations/eval.md.

use anyhow::{Context, Result};
use bashkit::{Bash, BashBuilder, BashTool, CPython, GitConfig, Sqlite, Tool};
use serde::{Deserialize, Serialize};

use crate::dataset::EvalTask;
use crate::provider::{ContentBlock, Message, Provider, Role, ToolDefinition};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub commands: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTrace {
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCallResult>,
    pub tool_call_count: usize,
    /// Number of LLM round-trips (each provider.chat call = 1 turn)
    pub turns: usize,
    pub last_tool_response: Option<ToolCallResult>,
    pub natural_stop: bool,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    /// Wall-clock duration in milliseconds
    pub duration_ms: u64,
}

fn format_tool_output(stdout: &str, stderr: &str, exit_code: i32) -> String {
    let mut out = String::new();
    if !stdout.is_empty() {
        out.push_str(stdout);
    }
    if !stderr.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("STDERR: {}", stderr));
    }
    if exit_code != 0 {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("Exit code: {}", exit_code));
    }
    if out.is_empty() {
        out.push_str("(no output)");
    }
    out
}

/// Virtual git identity for eval sessions (TM-GIT-002: never the host's).
pub const EVAL_GIT_AUTHOR: (&str, &str) = ("Eval Agent", "eval@bashkit-eval.invalid");

/// Runtime opt-in env var for the embedded sqlite builtin (Turso is BETA).
pub const SQLITE_OPT_IN: (&str, &str) = ("BASHKIT_ALLOW_INPROCESS_SQLITE", "1");

/// Enable the embedded runtimes every eval `Bash` gets: real CPython 3.14
/// (`python`/`python3`, WASI guest, default `CPythonLimits`) and sqlite
/// (`sqlite`/`sqlite3`, runtime opt-in set).
///
/// Decision: on for every task, not only `mode=runtime`, so the model sees one
/// environment (and one system prompt) across the dataset. CPython, not Monty:
/// agents write CPython scripts (stdlib, classes, CLI). Shared with
/// `bashkit-replay` so gap telemetry replays on the same environment.
pub fn with_eval_runtimes(builder: BashBuilder) -> BashBuilder {
    builder
        .cpython()
        .sqlite()
        .env(SQLITE_OPT_IN.0, SQLITE_OPT_IN.1)
}

/// The `BashTool` used only to describe the tool to the model (description,
/// schema, system prompt). Registers the same runtimes as `with_eval_runtimes`
/// so the prompt lists python3/sqlite3 (with their hints) instead of the
/// default "python/python3 not available" warning.
pub fn eval_tool() -> BashTool {
    BashTool::builder()
        .username("eval")
        .hostname("bashkit-eval")
        .builtin("python", Box::new(CPython::default()))
        .builtin("python3", Box::new(CPython::default()))
        .builtin("sqlite", Box::new(Sqlite::default()))
        .builtin("sqlite3", Box::new(Sqlite::default()))
        .env(SQLITE_OPT_IN.0, SQLITE_OPT_IN.1)
        .build()
}

/// Build the task's starting `Bash`: mount `files`, enable sandboxed `git`,
/// python3 and sqlite3, then run the optional `setup` script (fixture repos,
/// symlinks, exec bits, generated inputs).
///
/// Decision: `setup` runs inside a subshell so its `cd`/variables never leak
/// into the agent's session, and its output is not recorded as a tool call
/// (checks see only the agent's own calls). A failing setup is an infra error,
/// not a model failure. Shared with the reference-solution test so CI proves
/// every task is solvable on the exact starting state the model sees.
pub async fn build_task_bash(task: &EvalTask) -> Result<Bash> {
    let mut builder = with_eval_runtimes(
        Bash::builder()
            .username("eval")
            .hostname("bashkit-eval")
            .git(GitConfig::new().author(EVAL_GIT_AUTHOR.0, EVAL_GIT_AUTHOR.1)),
    );

    for (path, content) in &task.files {
        builder = builder.mount_text(path, content);
    }
    let mut bash = builder.build();

    if let Some(setup) = task.setup.as_deref().filter(|s| !s.trim().is_empty()) {
        let r = bash
            .exec(&format!("(\nset -e\n{setup}\n)"))
            .await
            .with_context(|| format!("task {} setup failed to run", task.id))?;
        anyhow::ensure!(
            r.exit_code == 0,
            "task {} setup exited {}: {}",
            task.id,
            r.exit_code,
            r.stderr
        );
    }
    Ok(bash)
}

/// Where `run_verify` stores the hidden `verify` script's output.
pub const VERIFY_OUT: &str = "/.eval/verify.out";

/// Run the task's hidden `verify` script (if any) against the final VFS and
/// store its stdout+stderr at [`VERIFY_OUT`].
///
/// Decision: verify runs in a *fresh* `Bash` sharing the task's filesystem,
/// so the model's shell state (`set -e`, aliases, functions, cwd, exported
/// vars) cannot change how its work is probed; only files carry over, which
/// is exactly the runtime contract under test. Its exit code is ignored
/// (scripts echo what they need) and the output file is written through the
/// fs API, overwriting anything the model left there.
pub async fn run_verify(bash: &Bash, task: &EvalTask) {
    let Some(verify) = task.verify.as_deref().filter(|s| !s.trim().is_empty()) else {
        return;
    };
    let fs = bash.fs();
    let mut checker = with_eval_runtimes(
        Bash::builder()
            .username("eval")
            .hostname("bashkit-eval")
            .fs(fs.clone()),
    )
    .build();
    let out = match checker.exec(&format!("(\n{verify}\n) 2>&1")).await {
        Ok(r) => r.stdout.to_string(),
        Err(e) => format!("verify error: {e}\n"),
    };
    let path = std::path::Path::new(VERIFY_OUT);
    if let Some(dir) = path.parent() {
        fs.mkdir(dir, true).await.ok();
    }
    // Not an infra error: only the model's own files (e.g. a file at
    // `/.eval`, a full VFS) can block this, and then the checks fail.
    let _ = fs.write_file(path, out.as_bytes()).await;
}

/// Run the agent loop for a single task.
/// Returns (trace, bash) — bash kept for VFS inspection by scorer.
pub async fn run_agent_loop(
    provider: &dyn Provider,
    task: &EvalTask,
    max_turns: usize,
) -> Result<(AgentTrace, Bash)> {
    let mut bash = build_task_bash(task).await?;

    // Get tool definition from BashTool with matching config
    let tool = eval_tool();
    let tool_def = ToolDefinition {
        name: "bash".to_string(),
        description: tool.description().to_string(),
        input_schema: tool.input_schema(),
    };

    // Compose system message
    let default_system = tool.system_prompt();
    let system = task.system.as_deref().unwrap_or(&default_system);

    // Initialize conversation
    let mut messages = vec![Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: task.prompt.clone(),
        }],
    }];

    let mut all_tool_calls = Vec::new();
    let mut last_tool_response = None;
    let mut natural_stop = false;
    let mut total_input_tokens = 0u32;
    let mut total_output_tokens = 0u32;
    let mut turns = 0usize;
    let start = std::time::Instant::now();

    for _turn in 0..max_turns {
        let response = provider
            .chat(&messages, std::slice::from_ref(&tool_def), system)
            .await
            .context("provider chat failed")?;

        turns += 1;
        total_input_tokens += response.input_tokens;
        total_output_tokens += response.output_tokens;
        messages.push(response.message.clone());

        if response.stop {
            natural_stop = true;
            break;
        }

        // Extract tool_use blocks
        let tool_uses: Vec<_> = response
            .message
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => Some((id, name, input)),
                _ => None,
            })
            .collect();

        if tool_uses.is_empty() {
            natural_stop = true;
            break;
        }

        let mut result_blocks = Vec::new();
        for (id, _name, input) in &tool_uses {
            let commands = input["commands"]
                .as_str()
                .or_else(|| input["script"].as_str())
                .unwrap_or("");

            let (stdout, stderr, exit_code) = match bash.exec(commands).await {
                Ok(r) => (r.stdout.to_string(), r.stderr.to_string(), r.exit_code),
                Err(e) => (String::new(), e.to_string(), 1),
            };

            let tcr = ToolCallResult {
                commands: commands.to_string(),
                stdout: stdout.clone(),
                stderr: stderr.clone(),
                exit_code,
            };
            all_tool_calls.push(tcr.clone());
            last_tool_response = Some(tcr);

            let content = format_tool_output(&stdout, &stderr, exit_code);
            result_blocks.push(ContentBlock::ToolResult {
                tool_use_id: (*id).clone(),
                content,
                is_error: exit_code != 0,
            });
        }

        messages.push(Message {
            role: Role::ToolResult,
            content: result_blocks,
        });
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    // After the clock: verify probes the result, it is not the model's time.
    run_verify(&bash, task).await;

    Ok((
        AgentTrace {
            messages,
            tool_call_count: all_tool_calls.len(),
            turns,
            tool_calls: all_tool_calls,
            last_tool_response,
            natural_stop,
            total_input_tokens,
            total_output_tokens,
            duration_ms,
        },
        bash,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn task(json: &str) -> EvalTask {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn tool_prompt_advertises_runtimes() {
        let tool = eval_tool();
        let prompt = tool.system_prompt();
        assert!(prompt.contains("python/python3: CPython 3.14"), "{prompt}");
        assert!(prompt.contains("sqlite/sqlite3"), "{prompt}");
        assert!(!prompt.contains("python/python3 not available"), "{prompt}");
        assert!(tool.description().contains("python3"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn task_bash_runs_python3_and_sqlite3() {
        let t =
            task(r#"{"id":"t","category":"c","description":"d","prompt":"p","expectations":[]}"#);
        let mut bash = build_task_bash(&t).await.unwrap();
        let r = bash
            .exec("python3 -c 'import sys; print(sys.version_info[:2])'; sqlite3 :memory: 'SELECT 6*7'")
            .await
            .unwrap();
        assert_eq!(r.stdout, "(3, 14)\n42\n", "stderr: {}", r.stderr);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn verify_sees_files_not_shell_state() {
        let t = task(
            r#"{"id":"t","category":"c","description":"d","prompt":"p","expectations":[],
                "verify":"echo \"cwd=$PWD x=${X:-unset}\"; cat /work/out.txt; type f 2>&1"}"#,
        );
        let mut bash = build_task_bash(&t).await.unwrap();
        bash.exec("mkdir -p /work && cd /work && echo made > out.txt; export X=1; f() { :; }")
            .await
            .unwrap();
        run_verify(&bash, &t).await;
        let out = bash.fs().read_file(Path::new(VERIFY_OUT)).await.unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("x=unset"), "{out}");
        assert!(!out.contains("cwd=/work"), "{out}");
        assert!(out.contains("made\n"), "{out}");
        assert!(out.contains("not found"), "{out}");
    }
}
