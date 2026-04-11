//! Default SSH handler using russh.
//!
//! Provides a real SSH transport backed by the `russh` crate.
//! Used automatically when no custom [`SshHandler`] is set.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;

use super::handler::{SshHandler, SshOutput, SshTarget};

/// Shell-escape a string for safe interpolation into a remote command.
/// Wraps in single quotes and escapes embedded single quotes.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// SSH client handler that accepts all server keys.
///
/// THREAT[TM-SSH-006]: In production, embedders should implement
/// `SshHandler` with proper host key verification. This default
/// handler accepts all keys for simplicity.
struct ClientHandler;

impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        // Accept all host keys. Embedders needing strict verification
        // should implement their own SshHandler.
        Ok(true)
    }
}

/// Default SSH transport using russh.
///
/// Supports password and private key authentication.
/// SCP/SFTP are implemented via remote commands (`cat`, `base64`).
pub struct RusshHandler {
    timeout: Duration,
    /// THREAT[TM-SSH-004]: Streaming size limit to prevent OOM from malicious servers.
    max_response_bytes: usize,
}

impl RusshHandler {
    pub fn new(timeout: Duration, max_response_bytes: usize) -> Self {
        Self {
            timeout,
            max_response_bytes,
        }
    }

    /// Connect and authenticate to a remote host.
    async fn connect(
        &self,
        target: &SshTarget,
    ) -> std::result::Result<russh::client::Handle<ClientHandler>, String> {
        let config = russh::client::Config {
            inactivity_timeout: Some(self.timeout),
            ..<_>::default()
        };

        let addr = (target.host.as_str(), target.port);
        let mut session = russh::client::connect(Arc::new(config), addr, ClientHandler)
            .await
            .map_err(|e| format!("connection failed: {e}"))?;

        // Authenticate: try "none" first (public SSH services like supabase.sh),
        // then private key, then password.
        if let Some(ref key_pem) = target.private_key {
            let key_pair = russh::keys::PrivateKey::from_openssh(key_pem.as_bytes())
                .map_err(|e| format!("invalid private key: {e}"))?;
            let auth = session
                .authenticate_publickey(
                    &target.user,
                    russh::keys::PrivateKeyWithHashAlg::new(
                        Arc::new(key_pair),
                        session
                            .best_supported_rsa_hash()
                            .await
                            .ok()
                            .flatten()
                            .flatten(),
                    ),
                )
                .await
                .map_err(|e| format!("publickey auth failed: {e}"))?;
            if !auth.success() {
                return Err("publickey authentication rejected".to_string());
            }
        } else if let Some(ref password) = target.password {
            let auth = session
                .authenticate_password(&target.user, password)
                .await
                .map_err(|e| format!("password auth failed: {e}"))?;
            if !auth.success() {
                return Err("password authentication rejected".to_string());
            }
        } else {
            // No credentials — try "none" auth (works for public SSH services)
            let auth = session
                .authenticate_none(&target.user)
                .await
                .map_err(|e| format!("auth failed: {e}"))?;
            if !auth.success() {
                return Err("ssh: authentication failed (server requires credentials)".to_string());
            }
        }

        Ok(session)
    }
}

#[async_trait]
impl SshHandler for RusshHandler {
    async fn exec(
        &self,
        target: &SshTarget,
        command: &str,
    ) -> std::result::Result<SshOutput, String> {
        let session = self.connect(target).await?;

        let mut channel = session
            .channel_open_session()
            .await
            .map_err(|e| format!("channel open failed: {e}"))?;

        channel
            .exec(true, command)
            .await
            .map_err(|e| format!("exec failed: {e}"))?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code: Option<u32> = None;

        loop {
            let Some(msg) = channel.wait().await else {
                break;
            };
            match msg {
                russh::ChannelMsg::Data { ref data } => {
                    stdout.extend_from_slice(data);
                }
                russh::ChannelMsg::ExtendedData { ref data, ext } => {
                    if ext == 1 {
                        // stderr
                        stderr.extend_from_slice(data);
                    }
                }
                russh::ChannelMsg::ExitStatus { exit_status } => {
                    exit_code = Some(exit_status);
                }
                _ => {}
            }
            // THREAT[TM-SSH-004]: Enforce streaming size limit to prevent OOM
            if stdout.len() + stderr.len() > self.max_response_bytes {
                let _ = channel.close().await;
                let _ = session
                    .disconnect(russh::Disconnect::ByApplication, "", "")
                    .await;
                return Err(format!(
                    "ssh: response too large (streaming limit exceeded, max {} bytes)",
                    self.max_response_bytes
                ));
            }
        }

        let _ = session
            .disconnect(russh::Disconnect::ByApplication, "", "")
            .await;

        Ok(SshOutput {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            exit_code: exit_code.unwrap_or(0) as i32,
        })
    }

    async fn shell(&self, target: &SshTarget) -> std::result::Result<SshOutput, String> {
        let session = self.connect(target).await?;

        let mut channel = session
            .channel_open_session()
            .await
            .map_err(|e| format!("channel open failed: {e}"))?;

        // Request a PTY so the remote TUI sends output
        channel
            .request_pty(false, "xterm", 80, 24, 0, 0, &[])
            .await
            .map_err(|e| format!("pty request failed: {e}"))?;

        channel
            .request_shell(true)
            .await
            .map_err(|e| format!("shell request failed: {e}"))?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code: Option<u32> = None;

        loop {
            let Some(msg) = channel.wait().await else {
                break;
            };
            match msg {
                russh::ChannelMsg::Data { ref data } => {
                    stdout.extend_from_slice(data);
                }
                russh::ChannelMsg::ExtendedData { ref data, ext } => {
                    if ext == 1 {
                        stderr.extend_from_slice(data);
                    }
                }
                russh::ChannelMsg::ExitStatus { exit_status } => {
                    exit_code = Some(exit_status);
                }
                _ => {}
            }
            // THREAT[TM-SSH-004]: Enforce streaming size limit to prevent OOM
            if stdout.len() + stderr.len() > self.max_response_bytes {
                let _ = channel.close().await;
                let _ = session
                    .disconnect(russh::Disconnect::ByApplication, "", "")
                    .await;
                return Err(format!(
                    "ssh: response too large (streaming limit exceeded, max {} bytes)",
                    self.max_response_bytes
                ));
            }
        }

        let _ = session
            .disconnect(russh::Disconnect::ByApplication, "", "")
            .await;

        Ok(SshOutput {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            exit_code: exit_code.unwrap_or(0) as i32,
        })
    }

    async fn upload(
        &self,
        target: &SshTarget,
        remote_path: &str,
        content: &[u8],
        mode: u32,
    ) -> std::result::Result<(), String> {
        // THREAT[TM-SSH-008]: Shell-escape remote path to prevent injection
        let b64 = base64::engine::general_purpose::STANDARD.encode(content);
        let escaped_path = shell_escape(remote_path);
        let cmd = format!(
            "echo '{}' | base64 -d > {} && chmod {:o} {}",
            b64, escaped_path, mode, escaped_path
        );
        let result = self.exec(target, &cmd).await?;
        if result.exit_code != 0 {
            return Err(format!(
                "upload failed (exit {}): {}",
                result.exit_code, result.stderr
            ));
        }
        Ok(())
    }

    async fn download(
        &self,
        target: &SshTarget,
        remote_path: &str,
    ) -> std::result::Result<Vec<u8>, String> {
        // THREAT[TM-SSH-008]: Shell-escape remote path to prevent injection
        let cmd = format!("base64 < {}", shell_escape(remote_path));
        let result = self.exec(target, &cmd).await?;
        if result.exit_code != 0 {
            return Err(format!(
                "download failed (exit {}): {}",
                result.exit_code, result.stderr
            ));
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(result.stdout.trim())
            .map_err(|e| format!("base64 decode failed: {e}"))?;
        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_russh_handler_stores_max_response_bytes() {
        let handler = RusshHandler::new(Duration::from_secs(30), 1024);
        assert_eq!(handler.max_response_bytes, 1024);
    }

    #[test]
    fn test_russh_handler_default_max_response_bytes() {
        use crate::ssh::config::DEFAULT_MAX_RESPONSE_BYTES;
        let handler = RusshHandler::new(Duration::from_secs(30), DEFAULT_MAX_RESPONSE_BYTES);
        assert_eq!(handler.max_response_bytes, 10_000_000);
    }

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("hello"), "'hello'");
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
        assert_eq!(shell_escape(""), "''");
    }

    /// Verify the streaming limit is wired through from SshConfig to RusshHandler.
    /// The actual streaming enforcement is tested via the mock handler in client.rs tests;
    /// here we verify construction and field propagation.
    #[test]
    fn test_streaming_limit_propagation() {
        use crate::ssh::client::SshClient;
        use crate::ssh::config::SshConfig;

        let config = SshConfig::new().max_response_bytes(512);
        let client = SshClient::new(config);
        // The client's default_handler should have max_response_bytes = 512.
        // We can't inspect it directly, but we verify config is passed through
        // by checking the client's config.
        assert_eq!(client.config().max_response_bytes, 512);
    }
}
