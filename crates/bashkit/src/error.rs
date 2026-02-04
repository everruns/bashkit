//! Error types for BashKit
//!
//! This module provides error types for the interpreter with the following design goals:
//! - Human-readable error messages for users
//! - No leakage of sensitive information (paths, memory addresses, secrets)
//! - Clear categorization for programmatic handling

use crate::limits::LimitExceeded;
use thiserror::Error;

/// Result type alias using BashKit's Error.
pub type Result<T> = std::result::Result<T, Error>;

/// BashKit error types.
///
/// All error messages are designed to be safe for display to end users without
/// exposing internal details or sensitive information.
#[derive(Error, Debug)]
pub enum Error {
    /// Parse error occurred while parsing the script (without location info).
    #[error("parse error: {0}")]
    Parse(String),

    /// Parse error with source location information.
    #[error("parse error at line {line}, column {column}: {message}")]
    ParseAt {
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

    /// Command not found.
    #[error("command not found: {0}")]
    CommandNotFound(String),

    /// Resource limit exceeded.
    #[error("resource limit exceeded: {0}")]
    ResourceLimit(#[from] LimitExceeded),

    /// Network error.
    #[error("network error: {0}")]
    Network(String),

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
        Self::ParseAt {
            message: message.into(),
            line,
            column,
        }
    }
}
