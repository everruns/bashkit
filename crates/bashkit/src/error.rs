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

    /// THREAT[TM-INF-016]: Create an I/O error with sanitized message.
    /// Strips host-internal paths from the error message to prevent information
    /// leakage to the sandbox guest.
    pub fn io_sanitized(err: std::io::Error) -> Self {
        Self::Io(std::io::Error::new(
            err.kind(),
            sanitize_error_message(&err.to_string()),
        ))
    }

    /// THREAT[TM-INF-016]: Create a network error with sanitized message.
    /// Strips resolved IPs, TLS details, and DNS info from reqwest errors.
    pub fn network_sanitized(context: &str, err: &dyn std::fmt::Display) -> Self {
        Self::Network(format!(
            "{}: {}",
            context,
            sanitize_error_message(&err.to_string())
        ))
    }
}

/// THREAT[TM-INF-016]: Sanitize error messages to prevent information leakage.
/// Strips:
/// - Host filesystem paths (anything starting with /)
/// - Resolved IP addresses (IPv4 and IPv6)
/// - TLS/SSL negotiation details
fn sanitize_error_message(msg: &str) -> String {
    use std::sync::LazyLock;

    static PATH_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r#"(/(?:home|usr|var|etc|opt|root|proc|sys|run|snap|nix|mnt|media)[/][^\s:"']+)"#,
        )
        .expect("path regex")
    });
    static IPV4_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(:\d+)?\b").expect("ipv4 regex")
    });
    static IPV6_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"\[?[0-9a-fA-F:]{3,39}\]?(:\d+)?").expect("ipv6 regex")
    });
    static TLS_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?i)(ssl|tls)\s*(handshake|negotiation|error|alert)[^.;]*[.;]?")
            .expect("tls regex")
    });

    let mut result = msg.to_string();

    // Strip absolute host paths (preserve VFS paths like /tmp, /dev/null)
    result = PATH_RE.replace_all(&result, "<path>").to_string();

    // Strip IPv4 addresses
    result = IPV4_RE.replace_all(&result, "<address>").to_string();

    // Strip IPv6 addresses (only if :: present to avoid false positives)
    if result.contains("::") {
        result = IPV6_RE.replace_all(&result, "<address>").to_string();
    }

    // Strip TLS/SSL handshake details
    result = TLS_RE.replace_all(&result, "<tls-error>").to_string();

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_host_paths() {
        let msg = "No such file: /home/user/.config/bashkit/settings.json";
        let sanitized = sanitize_error_message(msg);
        assert!(!sanitized.contains("/home/user"));
        assert!(sanitized.contains("<path>"));
    }

    #[test]
    fn sanitize_strips_ipv4() {
        let msg = "connection refused: 192.168.1.100:8080";
        let sanitized = sanitize_error_message(msg);
        assert!(!sanitized.contains("192.168"));
        assert!(sanitized.contains("<address>"));
    }

    #[test]
    fn sanitize_strips_tls_details() {
        let msg = "SSL handshake failed with cipher TLS_AES_256_GCM;";
        let sanitized = sanitize_error_message(msg);
        assert!(!sanitized.contains("cipher"));
        assert!(sanitized.contains("<tls-error>"));
    }

    #[test]
    fn sanitize_preserves_safe_paths() {
        let msg = "file not found: /tmp/script.sh";
        let sanitized = sanitize_error_message(msg);
        assert!(sanitized.contains("/tmp/script.sh"));
    }

    #[test]
    fn sanitize_preserves_generic_messages() {
        let msg = "operation timed out";
        assert_eq!(sanitize_error_message(msg), msg);
    }
}
