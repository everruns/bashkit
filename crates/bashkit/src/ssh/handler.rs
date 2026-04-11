//! SSH handler trait for pluggable transport implementations.
//!
//! Embedders can implement [`SshHandler`] to intercept, proxy, log,
//! or mock SSH operations. The allowlist check happens _before_ the
//! handler is called, so the security boundary stays in bashkit.
//!
//! # Default
//!
//! When no custom handler is set, `SshClient` uses `russh` directly.

use async_trait::async_trait;

/// Connection target for an SSH operation.
///
/// Fully resolved by the builtin before passing to the handler.
/// The handler does NOT need to validate the host — that's already done.
#[derive(Clone)]
pub struct SshTarget {
    /// Remote hostname or IP.
    pub host: String,
    /// Remote port.
    pub port: u16,
    /// Username for authentication.
    pub user: String,
    /// Optional private key (PEM contents from VFS, not a file path).
    pub private_key: Option<String>,
    /// Optional password.
    pub password: Option<String>,
}

// THREAT[TM-INF-016]: Redact credentials in Debug output.
impl std::fmt::Debug for SshTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshTarget")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field(
                "private_key",
                &self.private_key.as_ref().map(|_| "[REDACTED]"),
            )
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

/// Output from a remote command execution.
#[derive(Debug, Clone, Default)]
pub struct SshOutput {
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// Remote exit code.
    pub exit_code: i32,
}

/// Trait for custom SSH transport implementations.
///
/// Embedders can implement this to:
/// - Mock SSH for testing
/// - Proxy through a bastion host
/// - Log/audit all SSH operations
/// - Rate-limit connections
///
/// The allowlist check happens _before_ the handler is called.
///
/// # Example
///
/// ```rust,ignore
/// use bashkit::ssh::{SshHandler, SshTarget, SshOutput};
/// use async_trait::async_trait;
///
/// struct MockSsh;
///
/// #[async_trait]
/// impl SshHandler for MockSsh {
///     async fn exec(
///         &self,
///         target: &SshTarget,
///         command: &str,
///     ) -> Result<SshOutput, String> {
///         Ok(SshOutput {
///             stdout: format!("mock: ran '{}' on {}\n", command, target.host),
///             stderr: String::new(),
///             exit_code: 0,
///         })
///     }
///
///     async fn upload(
///         &self, _target: &SshTarget, _remote_path: &str,
///         _content: &[u8], _mode: u32,
///     ) -> Result<(), String> {
///         Ok(())
///     }
///
///     async fn download(
///         &self, _target: &SshTarget, _remote_path: &str,
///     ) -> Result<Vec<u8>, String> {
///         Ok(Vec::new())
///     }
/// }
/// ```
#[async_trait]
pub trait SshHandler: Send + Sync {
    /// Execute a command on a remote host and return its output.
    ///
    /// Called after the host has been validated against the allowlist.
    async fn exec(
        &self,
        target: &SshTarget,
        command: &str,
    ) -> std::result::Result<SshOutput, String>;

    /// Open a shell session (no command) and capture output.
    ///
    /// Used for SSH services that present a TUI or greeting on connect
    /// (e.g. `ssh supabase.sh`). The session closes when the remote
    /// side sends EOF or the timeout expires.
    async fn shell(&self, target: &SshTarget) -> std::result::Result<SshOutput, String> {
        // Default: delegate to exec with empty shell invocation
        self.exec(target, "").await
    }

    /// Upload file content to a remote path (scp put / sftp put).
    ///
    /// Called after the host has been validated against the allowlist.
    async fn upload(
        &self,
        target: &SshTarget,
        remote_path: &str,
        content: &[u8],
        mode: u32,
    ) -> std::result::Result<(), String>;

    /// Download a file from a remote path (scp get / sftp get).
    ///
    /// Called after the host has been validated against the allowlist.
    async fn download(
        &self,
        target: &SshTarget,
        remote_path: &str,
    ) -> std::result::Result<Vec<u8>, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_redacts_credentials() {
        let target = SshTarget {
            host: "example.com".to_string(),
            port: 22,
            user: "admin".to_string(),
            private_key: Some("-----BEGIN OPENSSH PRIVATE KEY-----".to_string()),
            password: Some("super_secret".to_string()),
        };
        let debug = format!("{:?}", target);
        assert!(!debug.contains("super_secret"), "password leaked: {debug}");
        assert!(
            !debug.contains("BEGIN OPENSSH PRIVATE KEY"),
            "key leaked: {debug}"
        );
        assert!(debug.contains("[REDACTED]"), "REDACTED missing: {debug}");
        assert!(
            debug.contains("example.com"),
            "host should be visible: {debug}"
        );
    }
}
