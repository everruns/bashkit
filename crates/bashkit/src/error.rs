//! Error types for BashKit

use thiserror::Error;

/// Result type alias using BashKit's Error.
pub type Result<T> = std::result::Result<T, Error>;

/// BashKit error types.
#[derive(Error, Debug)]
pub enum Error {
    /// Parse error occurred while parsing the script.
    #[error("parse error: {0}")]
    Parse(String),

    /// Execution error occurred while running the script.
    #[error("execution error: {0}")]
    Execution(String),

    /// I/O error from filesystem operations.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Command not found.
    #[error("command not found: {0}")]
    CommandNotFound(String),

    /// Resource limit exceeded.
    #[error("resource limit exceeded: {0}")]
    ResourceLimit(String),
}
