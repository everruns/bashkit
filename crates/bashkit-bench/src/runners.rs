// Runner implementations for different shell interpreters
// Each runner implements the same interface for fair comparison
// Using enum dispatch instead of dyn traits for async compatibility

use anyhow::Result;
use bashkit::Bash;
use std::process::Stdio;
use tokio::process::Command;

/// Enum-based runner for different shell interpreters
#[derive(Clone)]
pub enum Runner {
    Bashkit,
    Bash(String),     // path to bash
    JustBash(String), // path or "npx:just-bash"
}

impl Runner {
    pub fn name(&self) -> &str {
        match self {
            Runner::Bashkit => "bashkit",
            Runner::Bash(_) => "bash",
            Runner::JustBash(_) => "just-bash",
        }
    }

    pub async fn run(&self, script: &str) -> Result<(String, String, i32)> {
        match self {
            Runner::Bashkit => run_bashkit(script).await,
            Runner::Bash(path) => run_bash(path, script).await,
            Runner::JustBash(path) => run_just_bash(path, script).await,
        }
    }
}

/// Bashkit runner factory
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

/// Native bash runner factory
pub struct BashRunner;

impl BashRunner {
    pub async fn create() -> Result<Runner> {
        let path = which_bash().await?;
        Ok(Runner::Bash(path))
    }
}

async fn which_bash() -> Result<String> {
    // Try common locations
    for path in &["/bin/bash", "/usr/bin/bash", "/usr/local/bin/bash"] {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }

    // Try which
    let output = Command::new("which").arg("bash").output().await?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(path);
        }
    }

    anyhow::bail!("bash not found")
}

async fn run_bash(path: &str, script: &str) -> Result<(String, String, i32)> {
    let child = Command::new(path)
        .arg("-c")
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

/// Just-bash runner factory
pub struct JustBashRunner;

impl JustBashRunner {
    pub async fn create() -> Result<Runner> {
        let path = which_just_bash().await?;
        Ok(Runner::JustBash(path))
    }
}

async fn which_just_bash() -> Result<String> {
    // Try common locations
    for path in &[
        "./just-bash",
        "/usr/local/bin/just-bash",
        "/usr/bin/just-bash",
    ] {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }

    // Try npx
    let output = Command::new("npx")
        .args(["--yes", "just-bash", "--version"])
        .output()
        .await;

    if let Ok(out) = output {
        if out.status.success() {
            return Ok("npx:just-bash".to_string());
        }
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

async fn run_just_bash(path: &str, script: &str) -> Result<(String, String, i32)> {
    let (cmd, args): (&str, Vec<&str>) = if path == "npx:just-bash" {
        ("npx", vec!["--yes", "just-bash", "-c"])
    } else {
        (path, vec!["-c"])
    };

    let child = Command::new(cmd)
        .args(&args)
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
