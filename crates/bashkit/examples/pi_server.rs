//! JSON-line server bridging pi coding agent to bashkit virtual bash + VFS.
//!
//! Reads JSON requests from stdin, executes via bashkit, writes JSON responses to stdout.
//! All operations (bash, read, write, edit) use bashkit's in-memory virtual filesystem.
//!
//! Build: cargo build --example pi_server --release
//! See:   examples/bashkit-pi/README.md

use bashkit::Bash;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Debug, Deserialize)]
struct Request {
    id: String,
    #[serde(flatten)]
    op: Operation,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op")]
enum Operation {
    #[serde(rename = "bash")]
    Bash { command: String },
    #[serde(rename = "read")]
    Read { path: String },
    #[serde(rename = "write")]
    Write { path: String, content: String },
    #[serde(rename = "mkdir")]
    Mkdir { path: String },
    #[serde(rename = "exists")]
    Exists { path: String },
}

#[derive(Debug, Serialize)]
struct Response {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stderr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exists: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Response {
    fn ok(id: String) -> Self {
        Self {
            id,
            stdout: None,
            stderr: None,
            exit_code: None,
            content: None,
            exists: None,
            error: None,
        }
    }

    fn err(id: String, msg: String) -> Self {
        Self {
            error: Some(msg),
            ..Self::ok(id)
        }
    }
}

#[tokio::main]
async fn main() {
    let mut bash = Bash::builder().build();
    let fs = bash.fs();

    // Signal ready
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({"ready": true})).unwrap()
    );

    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::err(String::new(), format!("parse error: {e}"));
                println!("{}", serde_json::to_string(&resp).unwrap());
                continue;
            }
        };

        let resp = match req.op {
            Operation::Bash { command } => match bash.exec(&command).await {
                Ok(result) => Response {
                    stdout: Some(result.stdout),
                    stderr: Some(result.stderr),
                    exit_code: Some(result.exit_code),
                    ..Response::ok(req.id)
                },
                Err(e) => Response::err(req.id, format!("{e}")),
            },
            Operation::Read { path } => match fs.read_file(Path::new(&path)).await {
                Ok(bytes) => Response {
                    content: Some(String::from_utf8_lossy(&bytes).into_owned()),
                    ..Response::ok(req.id)
                },
                Err(e) => Response::err(req.id, format!("{e}")),
            },
            Operation::Write { path, content } => {
                // Ensure parent directory exists
                if let Some(parent) = Path::new(&path).parent() {
                    let _ = fs.mkdir(parent, true).await;
                }
                match fs.write_file(Path::new(&path), content.as_bytes()).await {
                    Ok(()) => Response::ok(req.id),
                    Err(e) => Response::err(req.id, format!("{e}")),
                }
            }
            Operation::Mkdir { path } => match fs.mkdir(Path::new(&path), true).await {
                Ok(()) => Response::ok(req.id),
                Err(e) => Response::err(req.id, format!("{e}")),
            },
            Operation::Exists { path } => match fs.exists(Path::new(&path)).await {
                Ok(exists) => Response {
                    exists: Some(exists),
                    ..Response::ok(req.id)
                },
                Err(e) => Response::err(req.id, format!("{e}")),
            },
        };

        println!("{}", serde_json::to_string(&resp).unwrap());
    }
}
