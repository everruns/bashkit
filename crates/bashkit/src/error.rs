//! Error types for Bashkit
//!
//! This module provides error types for the interpreter with the following design goals:
//! - Human-readable error messages for users
//! - No leakage of sensitive information (paths, memory addresses, secrets)
//! - Clear categorization for programmatic handling

use crate::limits::LimitExceeded;
use thiserror::Error;

/// Result type alias using Bashkit's Error.
pub type Result<T> = std::result::Result<T, Error>;

/// Bashkit error types.
///
/// All error messages are designed to be safe for display to end users without
/// exposing internal details or sensitive information.
#[derive(Error, Debug)]
pub enum Error {
    /// Parse error occurred while parsing the script.
    ///
    /// When `line` and `column` are 0, the error has no source location.
    #[error("parse error{}: {message}", if *line > 0 { format!(" at line {}, column {}", line, column) } else { String::new() })]
    Parse {
        message: String,
        line: usize,
        column: usize,
    },

    /// Execution error occurred while running the script.
    #[error("execution error: {0}")]
    Execution(String),

    /// I/O error from filesystem operations.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Resource limit exceeded.
    #[error("resource limit exceeded: {0}")]
    ResourceLimit(#[from] LimitExceeded),

    /// Network error.
    #[error("network error: {0}")]
    Network(String),

    /// Regex compilation or matching error.
    #[error("regex error: {0}")]
    Regex(#[from] regex::Error),

    /// Execution was cancelled via the cancellation token.
    #[error("execution cancelled")]
    Cancelled,

    /// Internal error for unexpected failures.
    ///
    /// THREAT[TM-INT-002]: Unexpected internal failures should not crash the interpreter.
    /// This error type provides a human-readable message without exposing:
    /// - Stack traces
    /// - Memory addresses
    /// - Internal file paths
    /// - Panic messages that may contain sensitive data
    ///
    /// Use this for:
    /// - Recovered panics that need to abort execution
    /// - Logic errors that indicate a bug
    /// - Security-sensitive failures where details should not be exposed
    #[error("internal error: {0}")]
    Internal(String),
}

impl Error {
    /// Create a parse error with source location.
    pub fn parse_at(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self::Parse {
            message: message.into(),
            line,
            column,
        }
    }

    /// Create a parse error without source location.
    pub fn parse(message: impl Into<String>) -> Self {
        Self::Parse {
            message: message.into(),
            line: 0,
            column: 0,
        }
    }
}
