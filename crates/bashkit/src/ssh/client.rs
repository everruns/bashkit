//! SSH client with allowlist-based access control.
//!
//! Wraps an [`SshHandler`] with host allowlist enforcement.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::allowlist::SshMatch;
use super::config::SshConfig;
use super::handler::{SshHandler, SshOutput, SshTarget};
use super::russh_handler::RusshHandler;

/// SSH client with allowlist-based access control.
///
/// Enforces the host allowlist before delegating to the handler.
/// Tracks active session count for resource limiting.
pub struct SshClient {
    config: SshConfig,
    handler: Option<Box<dyn SshHandler>>,
    default_handler: RusshHandler,
    active_sessions: AtomicUsize,
}

impl std::fmt::Debug for SshClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshClient")
            .field("config", &self.config)
            .field("has_custom_handler", &self.handler.is_some())
            .field(
                "active_sessions",
                &self.active_sessions.load(Ordering::Relaxed),
            )
            .finish()
    }
}

impl SshClient {
    /// Create a new SSH client with the given configuration.
    ///
    /// Uses the default `russh`-based transport. Override with
    /// [`set_handler`](Self::set_handler) for custom transports.
    pub fn new(config: SshConfig) -> Self {
        let default_handler = RusshHandler::new(
            config.timeout,
            config.max_response_bytes,
            config.strict_host_key_checking,
            config.trusted_host_keys.clone(),
        );
        Self {
            config,
            handler: None,
            default_handler,
            active_sessions: AtomicUsize::new(0),
        }
    }

    /// Set a custom SSH handler.
    pub fn set_handler(&mut self, handler: Box<dyn SshHandler>) {
        self.handler = Some(handler);
    }

    /// Get the SSH configuration.
    pub fn config(&self) -> &SshConfig {
        &self.config
    }

    /// Open a shell session (no command) and capture output.
    ///
    /// Used for SSH services like `ssh supabase.sh` that present a TUI
    /// or greeting on connect without requiring a command.
    pub async fn shell(&self, target: &SshTarget) -> std::result::Result<SshOutput, String> {
        self.check_allowed(&target.host, target.port)?;
        self.acquire_session()?;
        let result = self.handler().shell(target).await;
        self.release_session();

        if let Ok(ref output) = result {
            let total = output.stdout.len() + output.stderr.len();
            if total > self.config.max_response_bytes {
                return Err(format!(
                    "ssh: response too large ({} bytes, max {})",
                    total, self.config.max_response_bytes
                ));
            }
        }

        result
    }

    /// Execute a command on a remote host.
    ///
    /// # Security (TM-SSH-001)
    ///
    /// The host is validated against the allowlist before connecting.
    pub async fn exec(
        &self,
        target: &SshTarget,
        command: &str,
    ) -> std::result::Result<SshOutput, String> {
        // THREAT[TM-SSH-001]: Validate host against allowlist
        self.check_allowed(&target.host, target.port)?;

        // THREAT[TM-SSH-003]: Check session limit
        self.acquire_session()?;
        let result = self.exec_inner(target, command).await;
        self.release_session();

        // THREAT[TM-SSH-004]: Enforce response size limit
        if let Ok(ref output) = result {
            let total = output.stdout.len() + output.stderr.len();
            if total > self.config.max_response_bytes {
                return Err(format!(
                    "ssh: response too large ({} bytes, max {})",
                    total, self.config.max_response_bytes
                ));
            }
        }

        result
    }

    /// Upload a file to a remote host.
    pub async fn upload(
        &self,
        target: &SshTarget,
        remote_path: &str,
        content: &[u8],
        mode: u32,
    ) -> std::result::Result<(), String> {
        self.check_allowed(&target.host, target.port)?;
        self.acquire_session()?;
        let result = self.upload_inner(target, remote_path, content, mode).await;
        self.release_session();
        result
    }

    /// Download a file from a remote host.
    pub async fn download(
        &self,
        target: &SshTarget,
        remote_path: &str,
    ) -> std::result::Result<Vec<u8>, String> {
        self.check_allowed(&target.host, target.port)?;
        self.acquire_session()?;
        let result = self.download_inner(target, remote_path).await;
        self.release_session();

        // THREAT[TM-SSH-004]: Enforce response size limit
        if let Ok(ref data) = result
            && data.len() > self.config.max_response_bytes
        {
            return Err(format!(
                "ssh: download too large ({} bytes, max {})",
                data.len(),
                self.config.max_response_bytes
            ));
        }

        result
    }

    fn check_allowed(&self, host: &str, port: u16) -> std::result::Result<(), String> {
        match self.config.allowlist.check(host, port) {
            SshMatch::Allowed => Ok(()),
            SshMatch::Blocked { reason } => Err(format!("ssh: {}", reason)),
        }
    }

    fn acquire_session(&self) -> std::result::Result<(), String> {
        let current = self.active_sessions.fetch_add(1, Ordering::SeqCst);
        if current >= self.config.max_sessions {
            self.active_sessions.fetch_sub(1, Ordering::SeqCst);
            return Err(format!(
                "ssh: too many active sessions ({}, max {})",
                current, self.config.max_sessions
            ));
        }
        Ok(())
    }

    fn release_session(&self) {
        self.active_sessions.fetch_sub(1, Ordering::SeqCst);
    }

    /// Get the handler: custom if set, otherwise default RusshHandler.
    fn handler(&self) -> &dyn SshHandler {
        match self.handler {
            Some(ref h) => h.as_ref(),
            None => &self.default_handler,
        }
    }

    async fn exec_inner(
        &self,
        target: &SshTarget,
        command: &str,
    ) -> std::result::Result<SshOutput, String> {
        self.handler().exec(target, command).await
    }

    async fn upload_inner(
        &self,
        target: &SshTarget,
        remote_path: &str,
        content: &[u8],
        mode: u32,
    ) -> std::result::Result<(), String> {
        self.handler()
            .upload(target, remote_path, content, mode)
            .await
    }

    async fn download_inner(
        &self,
        target: &SshTarget,
        remote_path: &str,
    ) -> std::result::Result<Vec<u8>, String> {
        self.handler().download(target, remote_path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SshConfig {
        SshConfig::new().allow("*.supabase.co").allow("10.0.0.1")
    }

    fn test_target(host: &str) -> SshTarget {
        SshTarget {
            host: host.to_string(),
            port: 22,
            user: "root".to_string(),
            private_key: None,
            password: None,
        }
    }

    #[tokio::test]
    async fn test_blocked_host() {
        let client = SshClient::new(test_config());
        let target = test_target("evil.com");
        let result = client.exec(&target, "ls").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not in allowlist"));
    }

    #[tokio::test]
    async fn test_blocked_port() {
        let client = SshClient::new(test_config());
        let mut target = test_target("db.supabase.co");
        target.port = 3333;
        let result = client.exec(&target, "ls").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("port"));
    }

    #[tokio::test]
    async fn test_allowed_host_default_handler_connect_fails() {
        let client = SshClient::new(test_config());
        let target = test_target("db.supabase.co");
        let result = client.exec(&target, "ls").await;
        // Allowed host, but connection fails (no real server)
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("connection failed") || err.contains("no authentication"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_session_limit() {
        let config = SshConfig::new().allow_all().max_sessions(1);
        let client = SshClient::new(config);

        // Simulate one active session
        client.active_sessions.store(1, Ordering::SeqCst);

        let target = test_target("any.host");
        let result = client.exec(&target, "ls").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too many active sessions"));
    }

    #[tokio::test]
    async fn test_with_mock_handler() {
        struct MockHandler;

        #[async_trait::async_trait]
        impl SshHandler for MockHandler {
            async fn exec(
                &self,
                target: &SshTarget,
                command: &str,
            ) -> std::result::Result<SshOutput, String> {
                Ok(SshOutput {
                    stdout: format!("{}@{}: {}\n", target.user, target.host, command),
                    stderr: String::new(),
                    exit_code: 0,
                })
            }

            async fn upload(
                &self,
                _target: &SshTarget,
                _path: &str,
                _content: &[u8],
                _mode: u32,
            ) -> std::result::Result<(), String> {
                Ok(())
            }

            async fn download(
                &self,
                _target: &SshTarget,
                _path: &str,
            ) -> std::result::Result<Vec<u8>, String> {
                Ok(b"file content".to_vec())
            }
        }

        let mut client = SshClient::new(SshConfig::new().allow("*.supabase.co"));
        client.set_handler(Box::new(MockHandler));

        let target = test_target("db.supabase.co");
        let result = client.exec(&target, "psql -c 'SELECT 1'").await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.stdout, "root@db.supabase.co: psql -c 'SELECT 1'\n");
        assert_eq!(output.exit_code, 0);
    }

    #[tokio::test]
    async fn test_response_size_limit() {
        struct LargeOutputHandler;

        #[async_trait::async_trait]
        impl SshHandler for LargeOutputHandler {
            async fn exec(
                &self,
                _target: &SshTarget,
                _command: &str,
            ) -> std::result::Result<SshOutput, String> {
                Ok(SshOutput {
                    stdout: "x".repeat(20_000_000), // 20MB
                    stderr: String::new(),
                    exit_code: 0,
                })
            }

            async fn upload(
                &self,
                _: &SshTarget,
                _: &str,
                _: &[u8],
                _: u32,
            ) -> std::result::Result<(), String> {
                Ok(())
            }

            async fn download(
                &self,
                _: &SshTarget,
                _: &str,
            ) -> std::result::Result<Vec<u8>, String> {
                Ok(Vec::new())
            }
        }

        let mut client = SshClient::new(SshConfig::new().allow_all());
        client.set_handler(Box::new(LargeOutputHandler));

        let target = test_target("host.com");
        let result = client.exec(&target, "cat bigfile").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("response too large"));
    }
}
