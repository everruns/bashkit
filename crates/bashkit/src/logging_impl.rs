//! Logging infrastructure for Bashkit
//!
//! This module provides structured logging with built-in security features
//! to prevent sensitive data leakage (TM-LOG-001).
//!
//! # Log Levels
//!
//! - **ERROR**: Unrecoverable failures, exceptions, security violations
//! - **WARN**: Recoverable issues, limit approaching, deprecated usage
//! - **INFO**: Session lifecycle, high-level execution flow
//! - **DEBUG**: Command execution, variable expansion, control flow
//! - **TRACE**: Internal parser/interpreter state, detailed data flow
//!
//! # Security
//!
//! Logging includes built-in redaction for sensitive patterns:
//! - Environment variables matching common secret patterns
//! - File paths containing sensitive directories
//! - Network credentials in URLs
//!
//! See threat model TM-LOG-* entries for security considerations.

use std::borrow::Cow;
use std::collections::HashSet;

/// Configuration for logging behavior
///
/// # Security (TM-LOG-001, TM-LOG-002)
///
/// By default, sensitive data is redacted from logs. Configure `redact_patterns`
/// to add custom patterns that should be hidden.
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Whether to redact sensitive data from logs (default: true)
    pub redact_sensitive: bool,

    /// Additional environment variable names to redact (case-insensitive)
    /// Default patterns include: PASSWORD, SECRET, TOKEN, KEY, CREDENTIAL, AUTH
    pub redact_env_vars: HashSet<String>,

    /// Whether to include script content in logs (default: false for security)
    /// WARN: Setting this to true may log sensitive data in scripts
    pub log_script_content: bool,

    /// Whether to include file contents in logs (default: false)
    pub log_file_contents: bool,

    /// Maximum length of logged values before truncation (default: 200)
    pub max_value_length: usize,
}

impl Default for LogConfig {
    fn default() -> Self {
        let mut redact_env_vars = HashSet::new();
        // Common sensitive variable patterns (TM-LOG-001)
        for pattern in &[
            "PASSWORD",
            "PASSWD",
            "SECRET",
            "TOKEN",
            "KEY",
            "CREDENTIAL",
            "AUTH",
            "API_KEY",
            "APIKEY",
            "PRIVATE",
            "BEARER",
            "JWT",
            "SESSION",
            "COOKIE",
            "ENCRYPTION",
            "SIGNING",
            "DATABASE_URL",
            "DB_URL",
            "CONNECTION_STRING",
            "AWS_SECRET",
            "AWS_ACCESS",
            "GITHUB_TOKEN",
            "NPM_TOKEN",
            "STRIPE",
            "TWILIO",
            "SENDGRID",
            // AI provider patterns
            "OPENAI",
            "ANTHROPIC",
            "CLAUDE",
            "AZURE_OPENAI",
            "GOOGLE_AI",
            "GEMINI",
            "COHERE",
            "HUGGINGFACE",
            "HUGGING_FACE",
            "REPLICATE",
            "MISTRAL",
            "PERPLEXITY",
            "GROQ",
            "TOGETHER",
            "ANYSCALE",
            "FIREWORKS",
            "DEEPMIND",
            "VERTEX_AI",
            "BEDROCK",
            "SAGEMAKER",
        ] {
            redact_env_vars.insert(pattern.to_string());
        }
        Self {
            redact_sensitive: true,
            redact_env_vars,
            log_script_content: false,
            log_file_contents: false,
            max_value_length: 200,
        }
    }
}

impl LogConfig {
    /// Create a new log configuration with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Disable sensitive data redaction (UNSAFE - use only for debugging)
    ///
    /// # Warning
    ///
    /// This may expose secrets in logs. Only use in trusted debugging environments.
    pub fn unsafe_disable_redaction(mut self) -> Self {
        self.redact_sensitive = false;
        self
    }

    /// Add custom environment variable patterns to redact
    pub fn redact_env(mut self, pattern: &str) -> Self {
        self.redact_env_vars.insert(pattern.to_uppercase());
        self
    }

    /// Enable logging of script content (UNSAFE)
    ///
    /// # Warning
    ///
    /// Scripts may contain embedded secrets, credentials, or sensitive data.
    pub fn unsafe_log_scripts(mut self) -> Self {
        self.log_script_content = true;
        self
    }

    /// Set maximum length for logged values
    pub fn max_value_length(mut self, len: usize) -> Self {
        self.max_value_length = len;
        self
    }

    /// Check if an environment variable name should be redacted
    pub fn should_redact_env(&self, name: &str) -> bool {
        if !self.redact_sensitive {
            return false;
        }
        let upper = name.to_uppercase();
        self.redact_env_vars
            .iter()
            .any(|pattern| upper.contains(pattern))
    }

    /// Redact a value if it appears sensitive
    ///
    /// # Security (TM-LOG-001)
    ///
    /// This function checks for common secret patterns and redacts them.
    pub fn redact_value<'a>(&self, value: &'a str) -> Cow<'a, str> {
        if !self.redact_sensitive {
            return self.truncate(value);
        }

        // Check for common secret patterns in the value itself
        let lower = value.to_lowercase();
        if lower.contains("password")
            || lower.contains("secret")
            || lower.contains("token")
            || lower.contains("bearer ")
            || lower.contains("basic ")
            || is_likely_secret(value)
        {
            return Cow::Borrowed("[REDACTED]");
        }

        self.truncate(value)
    }

    /// Redact URL credentials (TM-LOG-001)
    ///
    /// Removes userinfo (username:password) from URLs before logging.
    pub fn redact_url<'a>(&self, url: &'a str) -> Cow<'a, str> {
        if !self.redact_sensitive {
            return self.truncate(url);
        }

        // Check for userinfo in URL (scheme://user:pass@host)
        if let Some(scheme_end) = url.find("://") {
            let rest = &url[scheme_end + 3..];
            if let Some(at_pos) = rest.find('@') {
                // Check if there's a colon before @ (indicates password)
                if rest[..at_pos].contains(':') {
                    let scheme = &url[..scheme_end + 3];
                    let host_part = &rest[at_pos + 1..];
                    return Cow::Owned(format!("{}[REDACTED]@{}", scheme, host_part));
                }
            }
        }

        self.truncate(url)
    }

    /// Truncate value if it exceeds max length
    ///
    /// Handles UTF-8 char boundaries properly to avoid panics on multi-byte chars.
    fn truncate<'a>(&self, value: &'a str) -> Cow<'a, str> {
        if value.len() <= self.max_value_length {
            Cow::Borrowed(value)
        } else {
            // Find a valid char boundary at or before max_value_length
            let mut end = self.max_value_length;
            while end > 0 && !value.is_char_boundary(end) {
                end -= 1;
            }
            Cow::Owned(format!(
                "{}...[truncated {} bytes]",
                &value[..end],
                value.len() - end
            ))
        }
    }
}

/// Check if a value looks like a secret (high entropy, specific formats)
///
/// # Security (TM-LOG-001)
///
/// Detects common secret formats:
/// - Base64-encoded secrets (high entropy, specific length)
/// - API keys (common prefixes)
/// - JWTs (three dot-separated parts)
fn is_likely_secret(value: &str) -> bool {
    let trimmed = value.trim();

    // JWT format: three base64 parts separated by dots
    if trimmed.matches('.').count() == 2 {
        let parts: Vec<&str> = trimmed.split('.').collect();
        if parts.iter().all(|p| p.len() > 10 && is_base64_like(p)) {
            return true;
        }
    }

    // Common API key prefixes
    let prefixes = [
        "sk-", "pk-", "sk_live_", "sk_test_", "pk_live_", "pk_test_", "ghp_", "gho_", "ghu_",
        "ghs_", "ghr_", "xoxb-", "xoxp-", "xoxa-", "AKIA",
        "eyJ", // JWT header start (base64 of {"
    ];
    for prefix in prefixes {
        if trimmed.starts_with(prefix) && trimmed.len() > prefix.len() + 10 {
            return true;
        }
    }

    // High entropy detection for longer strings (likely random secrets)
    if trimmed.len() >= 32 && is_high_entropy(trimmed) {
        return true;
    }

    false
}

/// Check if string looks like base64
fn is_base64_like(s: &str) -> bool {
    s.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '_' || c == '-'
    })
}

/// Simple entropy check - high ratio of unique chars suggests random data
fn is_high_entropy(s: &str) -> bool {
    // Only check alphanumeric strings (likely tokens/keys)
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return false;
    }

    let unique: HashSet<char> = s.chars().collect();
    let ratio = unique.len() as f64 / s.len() as f64;

    // Random strings typically have high unique char ratio
    ratio > 0.5 && unique.len() > 15
}

/// Sanitize script content for logging (TM-LOG-002)
///
/// Escapes potentially dangerous characters that could be used for log injection.
pub fn sanitize_for_log(input: &str) -> String {
    input
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .chars()
        .filter(|c| !c.is_control() || *c == ' ')
        .collect()
}

/// Format script for logging with optional redaction
pub fn format_script_for_log(script: &str, config: &LogConfig) -> String {
    if !config.log_script_content {
        let lines = script.lines().count();
        let bytes = script.len();
        return format!("[script: {} lines, {} bytes]", lines, bytes);
    }

    let sanitized = sanitize_for_log(script);
    config.truncate(&sanitized).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_redaction() {
        let config = LogConfig::new();

        // Should redact common sensitive env vars
        assert!(config.should_redact_env("PASSWORD"));
        assert!(config.should_redact_env("api_key"));
        assert!(config.should_redact_env("MY_SECRET_TOKEN"));
        assert!(config.should_redact_env("DATABASE_URL"));

        // Should not redact normal vars
        assert!(!config.should_redact_env("HOME"));
        assert!(!config.should_redact_env("PATH"));
        assert!(!config.should_redact_env("USER"));
    }

    #[test]
    fn test_url_redaction() {
        let config = LogConfig::new();

        // Should redact userinfo
        assert_eq!(
            config
                .redact_url("https://user:pass@example.com/path")
                .as_ref(),
            "https://[REDACTED]@example.com/path"
        );

        // Should not modify URLs without credentials
        assert_eq!(
            config.redact_url("https://example.com/path").as_ref(),
            "https://example.com/path"
        );

        // Username without password is not redacted
        assert_eq!(
            config.redact_url("https://user@example.com/path").as_ref(),
            "https://user@example.com/path"
        );
    }

    #[test]
    fn test_value_redaction() {
        let config = LogConfig::new();

        // Should redact JWT-like tokens
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signature123";
        assert_eq!(config.redact_value(jwt).as_ref(), "[REDACTED]");

        // Should redact API keys
        assert_eq!(
            config.redact_value("sk-1234567890abcdefghij").as_ref(),
            "[REDACTED]"
        );
        assert_eq!(
            config.redact_value("ghp_1234567890abcdefghij").as_ref(),
            "[REDACTED]"
        );

        // Should not redact normal values
        assert_eq!(config.redact_value("hello world").as_ref(), "hello world");
    }

    #[test]
    fn test_truncation() {
        let config = LogConfig::new().max_value_length(20);
        let long_value = "a".repeat(50);
        let truncated = config.truncate(&long_value);
        // Should start with exactly 20 'a' chars (the max_value_length)
        assert!(
            truncated.starts_with("aaaaaaaaaaaaaaaaaaaa"),
            "Expected 20 a's at start"
        );
        assert!(truncated.contains("[truncated"));
    }

    #[test]
    fn test_script_formatting() {
        let config = LogConfig::new();
        let script = "echo hello\necho world";

        // Default: don't log content
        let formatted = format_script_for_log(script, &config);
        assert!(formatted.contains("2 lines"));
        assert!(!formatted.contains("echo"));

        // With unsafe flag: log content
        let config = config.unsafe_log_scripts();
        let formatted = format_script_for_log(script, &config);
        assert!(formatted.contains("echo"));
    }

    #[test]
    fn test_log_injection_prevention() {
        // TM-LOG-002: Prevent log injection via newlines
        let malicious = "normal\n[ERROR] fake log entry\nmore";
        let sanitized = sanitize_for_log(malicious);
        assert!(!sanitized.contains('\n'));
        assert!(sanitized.contains("\\n"));
    }

    #[test]
    fn test_disabled_redaction() {
        let config = LogConfig::new().unsafe_disable_redaction();

        // Should not redact when disabled
        assert!(!config.should_redact_env("PASSWORD"));
        assert_eq!(
            config.redact_url("https://user:pass@example.com").as_ref(),
            "https://user:pass@example.com"
        );
    }
}
