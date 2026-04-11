//! SSH configuration for Bashkit.
//!
//! # Security Mitigations
//!
//! - **TM-SSH-001**: Unauthorized host access → host allowlist (default-deny)
//! - **TM-SSH-002**: Credential leakage → keys from VFS only
//! - **TM-SSH-003**: Session exhaustion → max concurrent sessions
//! - **TM-SSH-005**: Connection hang → configurable timeouts

use std::time::Duration;

use super::allowlist::SshAllowlist;

/// Default SSH connection timeout.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Default maximum response size (10 MB).
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 10_000_000;

/// Default maximum concurrent sessions.
pub const DEFAULT_MAX_SESSIONS: usize = 5;

/// Default SSH port.
pub const DEFAULT_PORT: u16 = 22;

/// SSH configuration for Bashkit.
///
/// Controls SSH behavior including host allowlist, authentication,
/// timeouts, and resource limits.
///
/// # Example
///
/// ```rust
/// use bashkit::SshConfig;
/// use std::time::Duration;
///
/// let config = SshConfig::new()
///     .allow("*.supabase.co")
///     .allow("bastion.example.com")
///     .allow_port(2222)
///     .default_user("deploy")
///     .timeout(Duration::from_secs(60));
/// ```
///
/// # Security
///
/// - Host allowlist is default-deny (empty blocks everything)
/// - Keys are read from VFS only, never from host filesystem
/// - All connections have timeouts to prevent hangs
#[derive(Clone)]
pub struct SshConfig {
    /// Host allowlist
    pub(crate) allowlist: SshAllowlist,
    /// Default username for connections
    pub(crate) default_user: Option<String>,
    /// Default password for connections
    pub(crate) default_password: Option<String>,
    /// Default private key (PEM/OpenSSH format) for connections
    pub(crate) default_private_key: Option<String>,
    /// Connection timeout
    pub(crate) timeout: Duration,
    /// Maximum response body size in bytes
    pub(crate) max_response_bytes: usize,
    /// Maximum concurrent SSH sessions
    pub(crate) max_sessions: usize,
    /// Default port
    pub(crate) default_port: u16,
}

// THREAT[TM-INF-016]: Redact credentials in Debug output to prevent
// passwords and private keys from leaking into logs, error messages, or LLM context.
impl std::fmt::Debug for SshConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshConfig")
            .field("allowlist", &self.allowlist)
            .field("default_user", &self.default_user)
            .field(
                "default_password",
                &self.default_password.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "default_private_key",
                &self.default_private_key.as_ref().map(|_| "[REDACTED]"),
            )
            .field("timeout", &self.timeout)
            .field("max_response_bytes", &self.max_response_bytes)
            .field("max_sessions", &self.max_sessions)
            .field("default_port", &self.default_port)
            .finish()
    }
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            allowlist: SshAllowlist::new(),
            default_user: None,
            default_password: None,
            default_private_key: None,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            max_sessions: DEFAULT_MAX_SESSIONS,
            default_port: DEFAULT_PORT,
        }
    }
}

impl SshConfig {
    /// Create a new SSH configuration with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a host pattern to the allowlist.
    ///
    /// Patterns can be exact hosts (`db.supabase.co`) or
    /// wildcard subdomains (`*.supabase.co`).
    ///
    /// # Security (TM-SSH-001)
    ///
    /// Only hosts matching the allowlist can be connected to.
    pub fn allow(mut self, pattern: impl Into<String>) -> Self {
        self.allowlist = self.allowlist.allow(pattern);
        self
    }

    /// Add multiple host patterns.
    pub fn allow_many(mut self, patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.allowlist = self.allowlist.allow_many(patterns);
        self
    }

    /// Add an allowed port. Default: only port 22.
    ///
    /// # Security (TM-SSH-007)
    pub fn allow_port(mut self, port: u16) -> Self {
        self.allowlist = self.allowlist.allow_port(port);
        self
    }

    /// Allow all hosts (dangerous — testing only).
    pub fn allow_all(mut self) -> Self {
        self.allowlist = SshAllowlist::allow_all();
        self
    }

    /// Set the default username for SSH connections.
    ///
    /// Used when no `user@` prefix is specified in the ssh command.
    pub fn default_user(mut self, user: impl Into<String>) -> Self {
        self.default_user = Some(user.into());
        self
    }

    /// Set the default password for SSH connections.
    ///
    /// Used when no private key is provided. Typically set from
    /// environment variables or secret stores, not hardcoded.
    pub fn default_password(mut self, password: impl Into<String>) -> Self {
        self.default_password = Some(password.into());
        self
    }

    /// Set the default private key (PEM or OpenSSH format).
    ///
    /// Used when no `-i` flag is specified in the ssh command.
    /// Pass the key contents, not a file path.
    pub fn default_private_key(mut self, key: impl Into<String>) -> Self {
        self.default_private_key = Some(key.into());
        self
    }

    /// Set the connection timeout.
    ///
    /// # Security (TM-SSH-005)
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the maximum response size in bytes.
    ///
    /// # Security (TM-SSH-004)
    pub fn max_response_bytes(mut self, max: usize) -> Self {
        self.max_response_bytes = max;
        self
    }

    /// Set the maximum concurrent SSH sessions.
    ///
    /// # Security (TM-SSH-003)
    pub fn max_sessions(mut self, max: usize) -> Self {
        self.max_sessions = max;
        self
    }

    /// Set the default SSH port.
    pub fn default_port(mut self, port: u16) -> Self {
        self.default_port = port;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SshConfig::new();
        assert!(!config.allowlist.is_enabled());
        assert!(config.default_user.is_none());
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.max_response_bytes, 10_000_000);
        assert_eq!(config.max_sessions, 5);
        assert_eq!(config.default_port, 22);
    }

    #[test]
    fn test_builder_chain() {
        let config = SshConfig::new()
            .allow("*.supabase.co")
            .allow("bastion.example.com")
            .allow_port(2222)
            .default_user("deploy")
            .timeout(Duration::from_secs(60))
            .max_response_bytes(5_000_000)
            .max_sessions(3)
            .default_port(2222);

        assert!(config.allowlist.is_enabled());
        assert_eq!(config.default_user.as_deref(), Some("deploy"));
        assert_eq!(config.timeout, Duration::from_secs(60));
        assert_eq!(config.max_response_bytes, 5_000_000);
        assert_eq!(config.max_sessions, 3);
        assert_eq!(config.default_port, 2222);
    }

    #[test]
    fn test_debug_redacts_credentials() {
        let config = SshConfig::new()
            .default_password("super_secret_password")
            .default_private_key("-----BEGIN OPENSSH PRIVATE KEY-----");
        let debug = format!("{:?}", config);
        assert!(
            !debug.contains("super_secret_password"),
            "password leaked in Debug: {debug}"
        );
        assert!(
            !debug.contains("BEGIN OPENSSH PRIVATE KEY"),
            "private key leaked in Debug: {debug}"
        );
        assert!(debug.contains("[REDACTED]"), "REDACTED missing: {debug}");
    }

    #[test]
    fn test_allowlist_integration() {
        let config = SshConfig::new().allow("*.supabase.co").allow_port(22);

        assert!(config.allowlist.is_allowed("db.supabase.co", 22));
        assert!(!config.allowlist.is_allowed("evil.com", 22));
    }
}
