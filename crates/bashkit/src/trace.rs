// THREAT[TM-INF-019]: Trace events may contain secrets.
// TraceMode::Redacted scrubs common secret patterns in argv.
// TraceMode::Off (default) disables all tracing with zero overhead.

//! Structured execution trace events.
//!
//! Records structured events during script execution for debugging
//! and observability. Events are returned in `ExecResult.events`.

use std::time::Duration;

/// Controls what trace information is collected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TraceMode {
    /// No events recorded, zero overhead (default).
    #[default]
    Off,
    /// Events recorded with secret-bearing argv scrubbed.
    Redacted,
    /// Raw events with no redaction. Unsafe for shared sinks.
    Full,
}

/// Kind of trace event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceEventKind {
    /// A command is about to execute.
    CommandStart,
    /// A command has finished executing.
    CommandExit,
    /// A file was accessed (read, stat, readdir).
    FileAccess,
    /// A file was mutated (write, mkdir, remove, rename, chmod).
    FileMutation,
    /// A policy check denied an action.
    PolicyDenied,
}

/// Per-kind details for a trace event.
#[derive(Debug, Clone)]
pub enum TraceEventDetails {
    /// Details for CommandStart.
    CommandStart {
        /// The command name.
        command: String,
        /// Command arguments.
        argv: Vec<String>,
        /// Working directory at execution time.
        cwd: String,
    },
    /// Details for CommandExit.
    CommandExit {
        /// The command name.
        command: String,
        /// Exit code.
        exit_code: i32,
        /// Duration of command execution.
        duration: Duration,
    },
    /// Details for FileAccess.
    FileAccess {
        /// File path accessed.
        path: String,
        /// Action performed.
        action: String,
    },
    /// Details for FileMutation.
    FileMutation {
        /// File path mutated.
        path: String,
        /// Action performed (write, mkdir, remove, rename, chmod).
        action: String,
    },
    /// Details for PolicyDenied.
    PolicyDenied {
        /// Subject of the policy check.
        subject: String,
        /// Reason for denial.
        reason: String,
        /// Action that was denied.
        action: String,
    },
}

/// A single trace event.
#[derive(Debug, Clone)]
pub struct TraceEvent {
    /// Kind of event.
    pub kind: TraceEventKind,
    /// Monotonic sequence number within the execution.
    pub seq: u64,
    /// Per-kind details.
    pub details: TraceEventDetails,
}

/// Callback type for real-time trace event streaming.
pub type TraceCallback = Box<dyn FnMut(&TraceEvent) + Send + Sync>;

/// Collector for trace events during execution.
#[derive(Default)]
pub struct TraceCollector {
    mode: TraceMode,
    events: Vec<TraceEvent>,
    seq: u64,
    callback: Option<TraceCallback>,
}

impl TraceCollector {
    /// Create a new trace collector with the given mode.
    pub fn new(mode: TraceMode) -> Self {
        Self {
            mode,
            events: Vec::new(),
            seq: 0,
            callback: None,
        }
    }

    /// Set the real-time callback.
    pub fn set_callback(&mut self, callback: TraceCallback) {
        self.callback = Some(callback);
    }

    /// Get the current trace mode.
    pub fn mode(&self) -> TraceMode {
        self.mode
    }

    /// Record a trace event. No-op if mode is Off.
    pub fn record(&mut self, kind: TraceEventKind, details: TraceEventDetails) {
        if self.mode == TraceMode::Off {
            return;
        }

        let details = if self.mode == TraceMode::Redacted {
            redact_details(details)
        } else {
            details
        };

        let event = TraceEvent {
            kind,
            seq: self.seq,
            details,
        };
        self.seq += 1;

        if let Some(cb) = &mut self.callback {
            cb(&event);
        }
        self.events.push(event);
    }

    /// Drain collected events (moves them out).
    pub fn take_events(&mut self) -> Vec<TraceEvent> {
        std::mem::take(&mut self.events)
    }

    /// Record a command start event.
    pub fn command_start(&mut self, command: &str, argv: &[String], cwd: &str) {
        self.record(
            TraceEventKind::CommandStart,
            TraceEventDetails::CommandStart {
                command: command.to_string(),
                argv: argv.to_vec(),
                cwd: cwd.to_string(),
            },
        );
    }

    /// Record a command exit event.
    pub fn command_exit(&mut self, command: &str, exit_code: i32, duration: Duration) {
        self.record(
            TraceEventKind::CommandExit,
            TraceEventDetails::CommandExit {
                command: command.to_string(),
                exit_code,
                duration,
            },
        );
    }

    /// Record a file access event.
    pub fn file_access(&mut self, path: &str, action: &str) {
        self.record(
            TraceEventKind::FileAccess,
            TraceEventDetails::FileAccess {
                path: path.to_string(),
                action: action.to_string(),
            },
        );
    }

    /// Record a file mutation event.
    pub fn file_mutation(&mut self, path: &str, action: &str) {
        self.record(
            TraceEventKind::FileMutation,
            TraceEventDetails::FileMutation {
                path: path.to_string(),
                action: action.to_string(),
            },
        );
    }

    /// Record a policy denied event.
    pub fn policy_denied(&mut self, subject: &str, reason: &str, action: &str) {
        self.record(
            TraceEventKind::PolicyDenied,
            TraceEventDetails::PolicyDenied {
                subject: subject.to_string(),
                reason: reason.to_string(),
                action: action.to_string(),
            },
        );
    }
}

// Secret patterns to redact in Redacted mode.
const SECRET_SUFFIXES: &[&str] = &[
    "_KEY",
    "_SECRET",
    "_TOKEN",
    "_PASSWORD",
    "_PASS",
    "_CREDENTIAL",
];
const SECRET_HEADERS: &[&str] = &[
    "authorization",
    "x-api-key",
    "x-auth-token",
    "cookie",
    "proxy-authorization",
    "set-cookie",
    "x-csrf-token",
    "x-vault-token",
    "x-jenkins-crumb",
];

/// CLI flags whose *next* argument is a secret value.
// THREAT[TM-LOG-002]: Extend redaction to common CLI secret-passing flags.
const SECRET_FLAGS: &[&str] = &["--token", "--api-key", "--password", "--secret", "-p"];

/// Redact secret patterns from trace event details.
fn redact_details(details: TraceEventDetails) -> TraceEventDetails {
    match details {
        TraceEventDetails::CommandStart { command, argv, cwd } => TraceEventDetails::CommandStart {
            command,
            argv: redact_argv(&argv),
            cwd,
        },
        other => other,
    }
}

/// Redact secret values in command arguments.
// THREAT[TM-LOG-001]: Redact credentials from trace output in all flag formats
fn redact_argv(argv: &[String]) -> Vec<String> {
    let mut result = Vec::with_capacity(argv.len());
    let mut redact_next = false;

    for arg in argv {
        if redact_next {
            result.push("[REDACTED]".to_string());
            redact_next = false;
            continue;
        }

        let lower = arg.to_lowercase();

        // --header "value" or -H "value" (standalone flags — redact next arg)
        if lower == "-h" || lower == "--header" || lower == "--user" || lower == "-u" {
            result.push(arg.clone());
            redact_next = true;
            continue;
        }

        // THREAT[TM-LOG-002]: --token, --api-key, --password, --secret, -p (next arg is secret)
        if SECRET_FLAGS.iter().any(|f| lower == *f) {
            result.push(arg.clone());
            redact_next = true;
            continue;
        }

        // --token=VALUE, --api-key=VALUE, etc. (= concatenated form)
        if let Some(eq_pos) = arg.find('=') {
            let flag_part = &lower[..eq_pos];
            if SECRET_FLAGS.contains(&flag_part) {
                result.push(format!("{}=[REDACTED]", &arg[..eq_pos]));
                continue;
            }
        }

        // --header=Authorization: Bearer xxx (= concatenated form)
        if let Some(eq_pos) = arg
            .find('=')
            .filter(|_| lower.starts_with("--header=") || lower.starts_with("--user="))
        {
            let header_val = &arg[eq_pos + 1..];
            let header_lower = header_val.to_lowercase();
            if SECRET_HEADERS
                .iter()
                .any(|h| header_lower.starts_with(&format!("{h}:")))
                || lower.starts_with("--user=")
            {
                result.push(format!("{}=[REDACTED]", &arg[..eq_pos]));
            } else {
                result.push(arg.clone());
            }
            continue;
        }

        // -HAuthorization: Bearer xxx (concatenated -H form)
        if (lower.starts_with("-h") && lower.len() > 2 && !lower.starts_with("-h="))
            || (lower.starts_with("-u") && lower.len() > 2 && !lower.starts_with("-u="))
        {
            let prefix = &arg[..2]; // -H or -u
            let val = &arg[2..];
            let val_lower = val.to_lowercase();
            if lower.starts_with("-u")
                || SECRET_HEADERS
                    .iter()
                    .any(|h| val_lower.starts_with(&format!("{h}:")))
            {
                result.push(format!("{prefix}[REDACTED]"));
            } else {
                result.push(arg.clone());
            }
            continue;
        }

        // Check for "Authorization: xxx" or "Cookie: xxx" inline
        if SECRET_HEADERS
            .iter()
            .any(|h| lower.starts_with(&format!("{h}:")))
        {
            if let Some(colon_pos) = arg.find(':') {
                result.push(format!("{}: [REDACTED]", &arg[..colon_pos]));
            } else {
                result.push("[REDACTED]".to_string());
            }
            continue;
        }

        // Check for KEY=value env-style assignments with secret suffixes
        if let Some(eq_pos) = arg.find('=') {
            let key = &arg[..eq_pos].to_uppercase();
            if SECRET_SUFFIXES.iter().any(|s| key.ends_with(s)) {
                result.push(format!("{}=[REDACTED]", &arg[..eq_pos]));
                continue;
            }
        }

        // Check for URL credentials (user:pass@host)
        if arg.contains("://") && arg.contains('@') {
            result.push(redact_url_credentials(arg));
            continue;
        }

        result.push(arg.clone());
    }

    result
}

/// Redact credentials from a URL (user:pass@host → [REDACTED]@host).
fn redact_url_credentials(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let after_scheme = &url[scheme_end + 3..];
        if let Some(at_pos) = after_scheme.find('@') {
            return format!(
                "{}://[REDACTED]@{}",
                &url[..scheme_end],
                &after_scheme[at_pos + 1..]
            );
        }
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_mode_default_is_off() {
        assert_eq!(TraceMode::default(), TraceMode::Off);
    }

    #[test]
    fn test_collector_off_no_events() {
        let mut c = TraceCollector::new(TraceMode::Off);
        c.command_start("echo", &["hello".into()], "/home");
        assert!(c.take_events().is_empty());
    }

    #[test]
    fn test_collector_full_records() {
        let mut c = TraceCollector::new(TraceMode::Full);
        c.command_start("echo", &["hello".into()], "/home");
        c.command_exit("echo", 0, Duration::from_millis(1));
        let events = c.take_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, TraceEventKind::CommandStart);
        assert_eq!(events[1].kind, TraceEventKind::CommandExit);
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[1].seq, 1);
    }

    #[test]
    fn test_redact_authorization_header() {
        let argv = vec![
            "curl".into(),
            "-H".into(),
            "Authorization: Bearer secret123".into(),
            "https://api.example.com".into(),
        ];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[0], "curl");
        assert_eq!(redacted[1], "-H");
        assert_eq!(redacted[2], "[REDACTED]");
        assert_eq!(redacted[3], "https://api.example.com");
    }

    #[test]
    fn test_redact_inline_header() {
        let argv = vec!["curl".into(), "Authorization: Bearer secret".into()];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[1], "Authorization: [REDACTED]");
    }

    #[test]
    fn test_redact_env_secret() {
        let argv = vec!["env".into(), "API_KEY=supersecret".into(), "command".into()];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[1], "API_KEY=[REDACTED]");
    }

    #[test]
    fn test_redact_url_credentials() {
        let url = "https://user:password@api.example.com/path";
        let redacted = redact_url_credentials(url);
        assert_eq!(redacted, "https://[REDACTED]@api.example.com/path");
    }

    #[test]
    fn test_no_redact_normal_args() {
        let argv = vec!["ls".into(), "-la".into(), "/tmp".into()];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted, argv);
    }

    #[test]
    fn test_collector_callback() {
        use std::sync::{Arc, Mutex};
        let count = Arc::new(Mutex::new(0u32));
        let count_clone = count.clone();
        let mut c = TraceCollector::new(TraceMode::Full);
        c.set_callback(Box::new(move |_event| {
            *count_clone.lock().unwrap() += 1;
        }));
        c.command_start("echo", &["hi".into()], "/");
        c.file_access("/tmp/file", "read");
        assert_eq!(*count.lock().unwrap(), 2);
    }

    #[test]
    fn test_redacted_mode_scrubs() {
        let mut c = TraceCollector::new(TraceMode::Redacted);
        c.command_start(
            "curl",
            &["-H".into(), "Authorization: Bearer secret".into()],
            "/",
        );
        let events = c.take_events();
        if let TraceEventDetails::CommandStart { argv, .. } = &events[0].details {
            assert_eq!(argv[1], "[REDACTED]");
        } else {
            panic!("wrong event type");
        }
    }

    #[test]
    fn test_redact_user_flag() {
        let argv = vec![
            "curl".into(),
            "--user".into(),
            "admin:password123".into(),
            "https://api.example.com".into(),
        ];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[2], "[REDACTED]");
    }

    #[test]
    fn test_redact_short_user_flag() {
        let argv = vec![
            "curl".into(),
            "-u".into(),
            "admin:password123".into(),
            "https://api.example.com".into(),
        ];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[2], "[REDACTED]");
    }

    #[test]
    fn test_redact_header_equals_form() {
        let argv = vec![
            "curl".into(),
            "--header=Authorization: Bearer token".into(),
            "https://api.example.com".into(),
        ];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[1], "--header=[REDACTED]");
    }

    #[test]
    fn test_redact_concatenated_h_flag() {
        let argv = vec![
            "curl".into(),
            "-HAuthorization: Bearer secret".into(),
            "https://api.example.com".into(),
        ];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[1], "-H[REDACTED]");
    }

    #[test]
    fn test_redact_cookie_header() {
        let argv = vec!["curl".into(), "cookie: session=abc123".into()];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[1], "cookie: [REDACTED]");
    }

    #[test]
    fn test_redact_proxy_authorization() {
        let argv = vec![
            "curl".into(),
            "-H".into(),
            "Proxy-Authorization: Basic abc".into(),
        ];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[2], "[REDACTED]");
    }

    // THREAT[TM-LOG-002]: Tests for extended secret flag redaction

    #[test]
    fn test_redact_token_flag() {
        let argv = vec![
            "cli".into(),
            "--token".into(),
            "sk-secret-123".into(),
            "https://api.example.com".into(),
        ];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[1], "--token");
        assert_eq!(redacted[2], "[REDACTED]");
        assert_eq!(redacted[3], "https://api.example.com");
    }

    #[test]
    fn test_redact_api_key_flag() {
        let argv = vec!["cli".into(), "--api-key".into(), "key-abc".into()];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[2], "[REDACTED]");
    }

    #[test]
    fn test_redact_password_flag() {
        let argv = vec!["mysql".into(), "--password".into(), "s3cret".into()];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[2], "[REDACTED]");
    }

    #[test]
    fn test_redact_short_p_flag() {
        let argv = vec!["mysql".into(), "-p".into(), "s3cret".into()];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[2], "[REDACTED]");
    }

    #[test]
    fn test_redact_secret_flag() {
        let argv = vec!["vault".into(), "--secret".into(), "top-secret".into()];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[2], "[REDACTED]");
    }

    #[test]
    fn test_redact_token_equals_form() {
        let argv = vec!["cli".into(), "--token=sk-secret-123".into()];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[1], "--token=[REDACTED]");
    }

    #[test]
    fn test_redact_api_key_equals_form() {
        let argv = vec!["cli".into(), "--api-key=key-abc".into()];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[1], "--api-key=[REDACTED]");
    }

    #[test]
    fn test_redact_vault_token_header() {
        let argv = vec!["curl".into(), "X-Vault-Token: s.abcdef".into()];
        let redacted = redact_argv(&argv);
        assert_eq!(redacted[1], "X-Vault-Token: [REDACTED]");
    }
}
