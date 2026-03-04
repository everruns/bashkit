// Decision: expose `python` command in CLI via host command execution.
// Prefer `monty` backend when available to match product requirement.
// Fallback to `python3` to keep CLI usable where monty isn't installed.

use bashkit::{async_trait, Builtin, BuiltinContext, ExecResult};
use std::env;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

pub struct PythonBuiltin;

impl PythonBuiltin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Builtin for PythonBuiltin {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let (program, mut args) = if command_exists("monty") {
            ("monty", vec!["python".to_string()])
        } else {
            ("python3", Vec::new())
        };

        args.extend(ctx.args.iter().cloned());

        let output = match Command::new(program).args(&args).output() {
            Ok(output) => output,
            Err(err) => {
                return Ok(ExecResult::err(
                    format!("python: failed to start {program}: {err}\n"),
                    1,
                ));
            }
        };

        Ok(ExecResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(1),
            ..Default::default()
        })
    }
}

fn command_exists(cmd: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&path).any(|dir| is_executable_file(dir.join(cmd)))
}

fn is_executable_file(path: PathBuf) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && (metadata.permissions().mode() & 0o111 != 0)
}
