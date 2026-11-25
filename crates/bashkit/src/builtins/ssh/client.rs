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
    handler: Result<Option<Box<dyn SshHandler>>, String>,
    default_handler: RusshHandler,
    active_sessions: AtomicUsize,
}

// Return the permit when the handler future completes or is dropped.
struct SessionPermit<'a>(&'a AtomicUsize);

impl Drop for SessionPermit<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl std::fmt::Debug for SshClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshClient")
            .field("config", &self.config)
            .field("has_custom_handler", &!matches!(self.handler, Ok(None)))
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
            handler: Ok(None),
            default_handler,
            active_sessions: AtomicUsize::new(0),
        }
    }

    /// Set a custom SSH handler.
    ///
    /// In strict mode, operations return an error if the handler rejects the
    /// configured host-key policy.
    pub fn set_handler(&mut self, mut handler: Box<dyn SshHandler>) {
        // THREAT[TM-SSH-006]: Do not dispatch to an unconfigured custom transport.
        self.handler = if self.config.strict_host_key_checking {
            handler
                .configure_strict_host_key_checking(&self.config.trusted_host_keys)
                .map(|()| Some(handler))
        } else {
            Ok(Some(handler))
        };
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
        let handler = self.handler()?;
        let result = {
            let _permit = self.acquire_session()?;
            handler.shell(target).await
        };

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

        let handler = self.handler()?;

        // THREAT[TM-SSH-003]: Check session limit
        let result = {
            let _permit = self.acquire_session()?;
            handler.exec(target, command).await
        };

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
        let handler = self.handler()?;
        let _permit = self.acquire_session()?;
        handler.upload(target, remote_path, content, mode).await
    }

    /// Download a file from a remote host.
    pub async fn download(
        &self,
        target: &SshTarget,
        remote_path: &str,
    ) -> std::result::Result<Vec<u8>, String> {
        self.check_allowed(&target.host, target.port)?;
        let handler = self.handler()?;
        let result = {
            let _permit = self.acquire_session()?;
            handler.download(target, remote_path).await
        };

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

    fn acquire_session(&self) -> std::result::Result<SessionPermit<'_>, String> {
        self.active_sessions
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                (current < self.config.max_sessions).then(|| current + 1)
            })
            .map_err(|current| {
                format!(
                    "ssh: too many active sessions ({}, max {})",
                    current, self.config.max_sessions
                )
            })?;
        Ok(SessionPermit(&self.active_sessions))
    }

    fn handler(&self) -> Result<&dyn SshHandler, String> {
        match &self.handler {
            Ok(Some(handler)) => Ok(handler.as_ref()),
            Ok(None) => Ok(&self.default_handler),
            Err(error) => Err(error.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TrustedHostKey;

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

    #[test]
    fn test_session_permit_lifetime() {
        let client = SshClient::new(SshConfig::new().max_sessions(1));
        let mut operation = Box::pin(async {
            let _permit = client.acquire_session().expect("first permit");
            std::future::pending::<()>().await;
        });

        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(std::future::Future::poll(operation.as_mut(), &mut context).is_pending());
        assert!(client.acquire_session().is_err());

        drop(operation);
        assert_eq!(client.active_sessions.load(Ordering::SeqCst), 0);
        let _permit = client.acquire_session().expect("reclaimed permit");
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
            fn configure_strict_host_key_checking(
                &mut self,
                trusted_host_keys: &[TrustedHostKey],
            ) -> Result<(), String> {
                assert_eq!(trusted_host_keys.len(), 1);
                assert_eq!(trusted_host_keys[0].host, "db.supabase.co");
                assert_eq!(trusted_host_keys[0].public_key, "ssh-ed25519 test-pin");
                Ok(())
            }

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

        let mut client = SshClient::new(
            SshConfig::new()
                .allow("*.supabase.co")
                .trusted_host_key("db.supabase.co", "ssh-ed25519 test-pin"),
        );
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

        let mut client =
            SshClient::new(SshConfig::new().allow_all().strict_host_key_checking(false));
        client.set_handler(Box::new(LargeOutputHandler));

        let target = test_target("host.com");
        let result = client.exec(&target, "cat bigfile").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("response too large"));
    }
}
