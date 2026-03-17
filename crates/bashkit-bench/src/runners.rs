// Runner implementations for different shell interpreters
// Each runner implements the same interface for fair comparison
// Using enum dispatch instead of dyn traits for async compatibility
//
// Runner types:
// - bashkit: in-process Rust (no fork)
// - bashkit-cli: out-of-process via bashkit binary (subprocess per run)
// - bashkit-js: in-process via Node.js + @everruns/bashkit (persistent child)
// - bashkit-py: in-process via Python + bashkit package (persistent child)
// - bash: out-of-process via /bin/bash (subprocess per run)
// - just-bash: out-of-process via just-bash CLI (subprocess per run)
// - just-bash-inproc: in-process via Node.js + just-bash library (persistent child)

use anyhow::{Context, Result};
use bashkit::Bash;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

/// Enum-based runner for different shell interpreters
pub enum Runner {
    Bashkit,
    BashkitCli(String),
    BashkitJs(PersistentChild),
    BashkitPy(PersistentChild),
    NativeBash(String),
    Gosh(String),
    JustBash(String),
    JustBashInproc(PersistentChild),
}

impl Runner {
    pub fn name(&self) -> &str {
        match self {
            Runner::Bashkit => "bashkit",
            Runner::BashkitCli(_) => "bashkit-cli",
            Runner::BashkitJs(_) => "bashkit-js",
            Runner::BashkitPy(_) => "bashkit-py",
            Runner::NativeBash(_) => "bash",
            Runner::Gosh(_) => "gosh",
            Runner::JustBash(_) => "just-bash",
            Runner::JustBashInproc(_) => "just-bash-inproc",
        }
    }

    pub async fn run(&mut self, script: &str) -> Result<(String, String, i32)> {
        match self {
            Runner::Bashkit => run_bashkit(script).await,
            Runner::BashkitCli(path) => run_subprocess(path, &["-c"], script).await,
            Runner::BashkitJs(child) => child.run(script).await,
            Runner::BashkitPy(child) => child.run(script).await,
            Runner::NativeBash(path) => run_subprocess(path, &["-c"], script).await,
            Runner::Gosh(path) => run_subprocess(path, &["-c"], script).await,
            Runner::JustBash(path) => run_just_bash_subprocess(path, script).await,
            Runner::JustBashInproc(child) => child.run(script).await,
        }
    }
}

// === In-process bashkit (Rust) ===

pub struct BashkitRunner;

impl BashkitRunner {
    pub async fn create() -> Result<Runner> {
        Ok(Runner::Bashkit)
    }
}

async fn run_bashkit(script: &str) -> Result<(String, String, i32)> {
    let mut bash = Bash::builder().build();
    let result = bash.exec(script).await?;
    Ok((result.stdout, result.stderr, result.exit_code))
}

// === Out-of-process bashkit CLI ===

pub struct BashkitCliRunner;

impl BashkitCliRunner {
    pub async fn create() -> Result<Runner> {
        let path = which_bashkit_cli().await?;
        Ok(Runner::BashkitCli(path))
    }
}

async fn which_bashkit_cli() -> Result<String> {
    // Try release build in workspace
    let workspace = workspace_root();
    let release_path = workspace.join("target/release/bashkit");
    if release_path.exists() {
        return Ok(release_path.to_string_lossy().to_string());
    }

    // Try debug build
    let debug_path = workspace.join("target/debug/bashkit");
    if debug_path.exists() {
        return Ok(debug_path.to_string_lossy().to_string());
    }

    // Try PATH
    let output = Command::new("which").arg("bashkit").output().await?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(path);
        }
    }

    anyhow::bail!("bashkit CLI not found (run `cargo build -p bashkit-cli --release`)")
}

// === In-process bashkit via Node.js ===

pub struct BashkitJsRunner;

impl BashkitJsRunner {
    pub async fn create() -> Result<Runner> {
        let script_path = scripts_dir().join("bashkit-js-runner.cjs");
        if !script_path.exists() {
            anyhow::bail!(
                "bashkit-js runner script not found at {}",
                script_path.display()
            );
        }
        let child = PersistentChild::spawn("node", &[script_path.to_str().unwrap()]).await?;
        Ok(Runner::BashkitJs(child))
    }
}

// === In-process bashkit via Python ===

pub struct BashkitPyRunner;

impl BashkitPyRunner {
    pub async fn create() -> Result<Runner> {
        let script_path = scripts_dir().join("bashkit-py-runner.py");
        if !script_path.exists() {
            anyhow::bail!(
                "bashkit-py runner script not found at {}",
                script_path.display()
            );
        }
        // Run from /tmp to avoid Python importing local bashkit source dir
        let child =
            PersistentChild::spawn_with_cwd("python3", &[script_path.to_str().unwrap()], "/tmp")
                .await?;
        Ok(Runner::BashkitPy(child))
    }
}

// === Native bash (out-of-process) ===

pub struct BashRunner;

impl BashRunner {
    pub async fn create() -> Result<Runner> {
        let path = which_bash().await?;
        Ok(Runner::NativeBash(path))
    }
}

async fn which_bash() -> Result<String> {
    for path in &["/bin/bash", "/usr/bin/bash", "/usr/local/bin/bash"] {
        if Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }

    let output = Command::new("which").arg("bash").output().await?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(path);
        }
    }

    anyhow::bail!("bash not found")
}

// === Gosh - Go shell interpreter (out-of-process) ===

pub struct GoshRunner;

impl GoshRunner {
    pub async fn create() -> Result<Runner> {
        let path = which_gosh().await?;
        Ok(Runner::Gosh(path))
    }
}

async fn which_gosh() -> Result<String> {
    // Try GOPATH/bin
    if let Ok(gopath) = std::env::var("GOPATH") {
        let p = PathBuf::from(&gopath).join("bin/gosh");
        if p.exists() {
            return Ok(p.to_string_lossy().to_string());
        }
    }

    // Try ~/go/bin (default GOPATH)
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(&home).join("go/bin/gosh");
        if p.exists() {
            return Ok(p.to_string_lossy().to_string());
        }
    }

    // Try PATH
    let output = Command::new("which").arg("gosh").output().await?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(path);
        }
    }

    anyhow::bail!("gosh not found (install via: go install mvdan.cc/sh/v3/cmd/gosh@latest)")
}

// === Just-bash (out-of-process) ===

pub struct JustBashRunner;

impl JustBashRunner {
    pub async fn create() -> Result<Runner> {
        let path = which_just_bash().await?;
        Ok(Runner::JustBash(path))
    }
}

async fn which_just_bash() -> Result<String> {
    for path in &[
        "./just-bash",
        "/usr/local/bin/just-bash",
        "/usr/bin/just-bash",
    ] {
        if Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }

    // Try npx
    let output = Command::new("npx")
        .args(["--yes", "just-bash", "--version"])
        .output()
        .await;

    if let Ok(out) = output
        && out.status.success()
    {
        return Ok("npx:just-bash".to_string());
    }

    // Try which
    let output = Command::new("which").arg("just-bash").output().await?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(path);
        }
    }

    anyhow::bail!("just-bash not found (install via: npm install -g just-bash)")
}

async fn run_just_bash_subprocess(path: &str, script: &str) -> Result<(String, String, i32)> {
    let (cmd, args): (&str, Vec<&str>) = if path == "npx:just-bash" {
        ("npx", vec!["--yes", "just-bash", "--allow-write", "-c"])
    } else {
        (path, vec!["--allow-write", "-c"])
    };

    run_subprocess(cmd, &args, script).await
}

// === In-process just-bash via Node.js ===

pub struct JustBashInprocRunner;

impl JustBashInprocRunner {
    pub async fn create() -> Result<Runner> {
        let script_path = scripts_dir().join("justbash-inproc-runner.cjs");
        if !script_path.exists() {
            anyhow::bail!(
                "just-bash inproc runner script not found at {}",
                script_path.display()
            );
        }
        let child = PersistentChild::spawn("node", &[script_path.to_str().unwrap()]).await?;
        Ok(Runner::JustBashInproc(child))
    }
}

// === Shared utilities ===

/// Run a command as a subprocess (one process per invocation)
async fn run_subprocess(cmd: &str, args: &[&str], script: &str) -> Result<(String, String, i32)> {
    let child = Command::new(cmd)
        .args(args)
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let output = child.wait_with_output().await?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    Ok((stdout, stderr, exit_code))
}

/// A persistent child process that communicates via JSON lines over stdin/stdout.
/// Used for in-process runners that need a warm interpreter (Node.js, Python).
///
/// Protocol:
/// - On startup, child sends: {"ready": true}
/// - For each request, parent sends: {"script": "..."}
/// - Child responds with: {"stdout": "...", "stderr": "...", "exitCode": 0}
pub struct PersistentChild {
    child: Child,
    stdin: tokio::process::ChildStdin,
    reader: BufReader<tokio::process::ChildStdout>,
}

impl PersistentChild {
    async fn spawn(cmd: &str, args: &[&str]) -> Result<Self> {
        Self::spawn_with_cwd(cmd, args, ".").await
    }

    async fn spawn_with_cwd(cmd: &str, args: &[&str], cwd: &str) -> Result<Self> {
        let mut child = Command::new(cmd)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to spawn {cmd}"))?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        let mut pc = Self {
            child,
            stdin,
            reader,
        };

        // Wait for ready signal
        let mut line = String::new();
        pc.reader.read_line(&mut line).await?;
        let ready: serde_json::Value = serde_json::from_str(line.trim())?;
        if !ready
            .get("ready")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            anyhow::bail!("Child process did not send ready signal");
        }

        Ok(pc)
    }

    async fn run(&mut self, script: &str) -> Result<(String, String, i32)> {
        let request = serde_json::json!({ "script": script });
        let mut msg = serde_json::to_string(&request)?;
        msg.push('\n');
        self.stdin.write_all(msg.as_bytes()).await?;
        self.stdin.flush().await?;

        let mut line = String::new();
        self.reader.read_line(&mut line).await?;

        if line.is_empty() {
            anyhow::bail!("Child process closed stdout unexpectedly");
        }

        let resp: serde_json::Value =
            serde_json::from_str(line.trim()).context("Failed to parse child response")?;

        let stdout = resp
            .get("stdout")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let stderr = resp
            .get("stderr")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let exit_code = resp.get("exitCode").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;

        Ok((stdout, stderr, exit_code))
    }
}

impl Drop for PersistentChild {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn workspace_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent() // crates/
        .and_then(|p| p.parent()) // workspace root
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn scripts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts")
}
