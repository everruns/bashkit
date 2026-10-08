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
    ResourceLimit(LimitExceeded),

    /// Network error.
    #[error("network error: {0}")]
    Network(String),

    /// Regex compilation or matching error.
    #[error("regex error: {0}")]
    Regex(#[from] regex::Error),

    /// Execution was cancelled via the cancellation token.
    #[error("execution cancelled")]
    Cancelled,

    /// A bash-level command failure that should not abort the interpreter.
    ///
    /// Used for errors (e.g. missing input-redirect target) that bash handles
    /// by failing the individual command with exit 1 and continuing execution,
    /// rather than aborting the whole script.  Callers that process redirections
    /// before executing a command must catch this variant and convert it into a
    /// non-fatal `Ok(ExecResult)` with the enclosed stderr message.
    #[error("{0}")]
    CommandFailure(String),

    /// A snapshot declares a minimum reader version this build cannot satisfy.
    ///
    /// Distinct from [`Error::Internal`] so callers can tell "your bashkit is
    /// too old for this snapshot" apart from corruption, and prompt an upgrade
    /// instead of discarding stored state. See
    /// `knowledge/foundations/snapshot-history.md` for the version policy.
    #[error("snapshot requires reader version {required}, this build supports {supported}")]
    SnapshotTooNew {
        /// Minimum reader version the snapshot declares.
        required: u16,
        /// Highest reader version this build implements.
        supported: u16,
    },

    /// The environment restoring a snapshot differs from the one that made it.
    ///
    /// Carries a rendered [`CapabilityDelta`](crate::CapabilityDelta) summary.
    #[error("snapshot capability mismatch: {0}")]
    SnapshotCapabilityMismatch(String),

    /// Bash abandons the current command line (its `DISCARD` jump), e.g. an
    /// arithmetic error inside `$((...))`. Converted to
    /// `ControlFlow::Abort` at the command boundary;
    /// the message is the diagnostic written to stderr.
    #[error("{0}")]
    LineAbort(String),

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

impl From<LimitExceeded> for Error {
    fn from(limit: LimitExceeded) -> Self {
        match limit {
            LimitExceeded::ExecutionBudget(crate::limits::ExecutionBudgetExceeded::Cancelled) => {
                Self::Cancelled
            }
            LimitExceeded::ExecutionBudget(crate::limits::ExecutionBudgetExceeded::Deadline {
                limit,
            }) => Self::ResourceLimit(LimitExceeded::Timeout(limit)),
            limit => Self::ResourceLimit(limit),
        }
    }
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

    /// bash's report of this syntax error in `source`, as read by `who`
    /// (`bash: -c`, a script path): `who: line N: message`, followed by the
    /// offending source line for a `near unexpected token` error. `None` for
    /// errors other than [`Error::Parse`].
    ///
    /// ```
    /// let err = bashkit::parser::Parser::new("if then fi").parse().unwrap_err();
    /// assert_eq!(
    ///     err.syntax_report("bash: -c", "if then fi").unwrap(),
    ///     "bash: -c: line 1: syntax error near unexpected token `then'\n\
    ///      bash: -c: line 1: `if then fi'\n"
    /// );
    /// ```
    pub fn syntax_report(&self, who: &str, source: &str) -> Option<String> {
        match self {
            Self::Parse { message, line, .. } => {
                Some(syntax_report(who, source, message, *line, 0))
            }
            _ => None,
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

/// VFS messages that only restate their `io::ErrorKind`.
///
/// These carry nothing the errno does not, so diagnostics swap them for the
/// `strerror` text real tools print. Every other message —
/// `filesystem is read-only`, a custom [`crate::FileSystem`] backend's own
/// wording — tells the caller *why* in a way the bare errno cannot, and is
/// kept verbatim.
const ERRNO_RESTATING_MESSAGES: &[&str] =
    &["file not found", "not found", "parent directory not found"];

/// Render a filesystem error the way a real shell tool renders it.
///
/// Never format a [`Error`] with `Display` into a diagnostic a script can
/// see: `io error: ` and `internal error: ` are Rust enum shapes no shell
/// ever prints, and the fuzz/proptest leak detector in [`crate::testing`]
/// bans them (TM-INF-022). `od /nope` must report
/// `od: /nope: No such file or directory`, not
/// `od: /nope: io error: file not found`.
pub(crate) fn io_error_reason(e: &Error) -> String {
    let io = match e {
        Error::Io(io) => io,
        other => return other.to_string(),
    };
    let message = io.to_string();
    // The in-memory backend reports this with no errno, so the kind match
    // below cannot reach it; bash prints the capitalized strerror.
    if message == "is a directory" {
        return "Is a directory".to_string();
    }
    // Errors straight from the OS stringify as `<strerror> (os error N)`;
    // real tools print only the strerror half.
    let restates_errno =
        io.raw_os_error().is_some() || ERRNO_RESTATING_MESSAGES.contains(&message.as_str());
    if !restates_errno {
        return message;
    }
    match io.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory",
        std::io::ErrorKind::PermissionDenied => "Permission denied",
        std::io::ErrorKind::IsADirectory => "Is a directory",
        std::io::ErrorKind::NotADirectory => "Not a directory",
        std::io::ErrorKind::AlreadyExists => "File exists",
        std::io::ErrorKind::Unsupported => "Operation not supported",
        _ => return message,
    }
    .to_string()
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

/// See [`Error::syntax_report`]. `shift` moves the reported line number
/// (eval'd text counts from the eval's own line). A message line after the
/// first that reads `line N: text` is a further diagnostic bash prints for
/// the same error, on its own line number.
pub(crate) fn syntax_report(
    who: &str,
    source: &str,
    message: &str,
    line: usize,
    shift: usize,
) -> String {
    let message = if message.starts_with("syntax error") || message.starts_with("unexpected EOF") {
        message.to_string()
    } else {
        format!("syntax error: {message}")
    };
    if line == 0 {
        return format!("{who}: {message}\n");
    }
    let at = line + shift;
    let mut lines = message.lines();
    let first = lines.next().unwrap_or_default();
    let mut out = format!("{who}: line {at}: {first}\n");
    if first.contains("near unexpected token")
        && let Some(text) = source.lines().nth(line - 1)
    {
        out.push_str(&format!("{who}: line {at}: `{text}'\n"));
    }
    for more in lines {
        let further = more.strip_prefix("line ").and_then(|rest| {
            let (n, text) = rest.split_once(": ")?;
            Some((n.parse::<usize>().ok()? + shift, text))
        });
        match further {
            Some((n, text)) => out.push_str(&format!("{who}: line {n}: {text}\n")),
            None => out.push_str(&format!("{who}: line {at}: {more}\n")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    /// A VFS message that only restates its errno is swapped for the
    /// `strerror` text real tools print — `od`, `cat` and redirection all
    /// depend on this to avoid printing the `io error: ` enum shape.
    #[test]
    fn io_error_reason_maps_errno_restating_messages() {
        for message in ["file not found", "not found", "parent directory not found"] {
            let err = Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, message));
            assert_eq!(io_error_reason(&err), "No such file or directory");
        }
    }

    /// A backend reason richer than its errno survives: `read-only` is the
    /// only actionable detail and collapsing it to `Permission denied`
    /// would drop it.
    #[test]
    fn io_error_reason_keeps_specific_backend_reason() {
        let err = Error::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "filesystem is read-only",
        ));
        assert_eq!(io_error_reason(&err), "filesystem is read-only");
    }

    /// An OS error stringifies as `<strerror> (os error N)`; only the
    /// strerror half is shell-shaped.
    #[test]
    fn io_error_reason_drops_the_os_error_number() {
        let err = Error::Io(std::io::Error::from_raw_os_error(2));
        assert_eq!(io_error_reason(&err), "No such file or directory");
    }

    /// Non-I/O variants are passed through: they have no errno to map, and
    /// their own `Display` is what callers already report.
    #[test]
    fn io_error_reason_passes_through_non_io_variants() {
        let err = Error::Execution("boom".into());
        assert_eq!(io_error_reason(&err), "execution error: boom");
    }

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
