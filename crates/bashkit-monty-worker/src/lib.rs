// IPC protocol for bashkit <-> monty-worker subprocess communication.
// JSON lines over stdin/stdout. Worker stays synchronous (no tokio).
//
// Flow:
//   Parent -> Worker: Init { code, filename, limits }
//   Worker -> Parent: OsCall { function, args, kwargs } | Complete | Error
//   Parent -> Worker: OsResponse { result }
//   ... repeat until Complete or Error or worker crash ...
//
// If the worker segfaults, the parent sees broken pipe / child exit with signal.

use monty::{ExcType, MontyObject, OsFunction};
use serde::{Deserialize, Serialize};

/// Parent -> Worker messages (JSON lines on worker's stdin).
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WorkerRequest {
    /// Start executing Python code with given limits.
    #[serde(rename = "init")]
    Init {
        code: String,
        filename: String,
        limits: WireLimits,
    },
    /// Response to a previous OsCall from the worker.
    #[serde(rename = "os_response")]
    OsResponse { result: WireExternalResult },
}

/// Worker -> Parent messages (JSON lines on worker's stdout).
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WorkerResponse {
    /// Execution paused: needs a VFS operation from the parent.
    #[serde(rename = "os_call")]
    OsCall {
        function: OsFunction,
        args: Vec<MontyObject>,
        kwargs: Vec<(MontyObject, MontyObject)>,
    },
    /// Execution completed successfully.
    #[serde(rename = "complete")]
    Complete { result: MontyObject, output: String },
    /// Execution failed with a Python exception.
    #[serde(rename = "error")]
    Error { exception: String, output: String },
}

/// Resource limits sent from parent to worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireLimits {
    pub max_allocations: usize,
    pub max_duration_secs: f64,
    pub max_memory: usize,
    pub max_recursion: usize,
}

/// Wire-safe version of monty's ExternalResult (which doesn't derive Serialize).
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum WireExternalResult {
    #[serde(rename = "ok")]
    Return { value: MontyObject },
    #[serde(rename = "error")]
    Error {
        exc_type: ExcType,
        message: Option<String>,
    },
}

/// Read one JSON line from a reader. Returns None on EOF.
pub fn read_message<T: serde::de::DeserializeOwned>(
    reader: &mut impl std::io::BufRead,
) -> Result<Option<T>, String> {
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => Ok(None), // EOF
        Ok(_) => serde_json::from_str(&line).map(Some).map_err(|e| {
            format!(
                "protocol error: {e}: {:?}",
                if line.len() > 200 {
                    &line[..200]
                } else {
                    &line
                }
            )
        }),
        Err(e) => Err(format!("read error: {e}")),
    }
}

/// Write one JSON line to a writer.
pub fn write_message<T: serde::Serialize>(
    writer: &mut impl std::io::Write,
    msg: &T,
) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, msg).map_err(|e| format!("serialize error: {e}"))?;
    writer
        .write_all(b"\n")
        .map_err(|e| format!("write error: {e}"))?;
    writer.flush().map_err(|e| format!("flush error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_worker_request_init() {
        let req = WorkerRequest::Init {
            code: "print('hi')".into(),
            filename: "<string>".into(),
            limits: WireLimits {
                max_allocations: 1_000_000,
                max_duration_secs: 30.0,
                max_memory: 64 * 1024 * 1024,
                max_recursion: 200,
            },
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: WorkerRequest = serde_json::from_str(&json).unwrap();
        match back {
            WorkerRequest::Init { code, filename, .. } => {
                assert_eq!(code, "print('hi')");
                assert_eq!(filename, "<string>");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn roundtrip_worker_response_os_call() {
        let resp = WorkerResponse::OsCall {
            function: OsFunction::ReadText,
            args: vec![MontyObject::Path("/tmp/f.txt".into())],
            kwargs: vec![],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: WorkerResponse = serde_json::from_str(&json).unwrap();
        match back {
            WorkerResponse::OsCall { function, args, .. } => {
                assert_eq!(function, OsFunction::ReadText);
                assert_eq!(args.len(), 1);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn roundtrip_wire_external_result() {
        let ok = WireExternalResult::Return {
            value: MontyObject::String("content".into()),
        };
        let json = serde_json::to_string(&ok).unwrap();
        let back: WireExternalResult = serde_json::from_str(&json).unwrap();
        match back {
            WireExternalResult::Return { value } => {
                assert_eq!(value, MontyObject::String("content".into()));
            }
            _ => panic!("wrong variant"),
        }

        let err = WireExternalResult::Error {
            exc_type: ExcType::FileNotFoundError,
            message: Some("not found".into()),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: WireExternalResult = serde_json::from_str(&json).unwrap();
        match back {
            WireExternalResult::Error { exc_type, message } => {
                assert_eq!(exc_type, ExcType::FileNotFoundError);
                assert_eq!(message.as_deref(), Some("not found"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn read_write_message_roundtrip() {
        let msg = WorkerResponse::Complete {
            result: MontyObject::Int(42),
            output: "42\n".into(),
        };
        let mut buf = Vec::new();
        write_message(&mut buf, &msg).unwrap();

        let mut cursor = std::io::Cursor::new(buf);
        let back: Option<WorkerResponse> = read_message(&mut cursor).unwrap();
        match back.unwrap() {
            WorkerResponse::Complete { result, output } => {
                assert_eq!(result, MontyObject::Int(42));
                assert_eq!(output, "42\n");
            }
            _ => panic!("wrong variant"),
        }
    }
}
