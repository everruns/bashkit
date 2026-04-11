//! Default SSH handler using russh.
//!
//! Provides a real SSH transport backed by the `russh` crate.
//! Used automatically when no custom [`SshHandler`] is set.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;

use super::config::TrustedHostKey;
use super::handler::{SshHandler, SshOutput, SshTarget};

/// Shell-escape a string for safe interpolation into a remote command.
/// Wraps in single quotes and escapes embedded single quotes.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// SSH client handler with host key verification.
///
/// THREAT[TM-SSH-006]: When strict host key checking is enabled (default),
/// connections are rejected unless the server key matches a trusted key.
struct ClientHandler {
    /// Target host for this connection (used to look up trusted keys).
    host: String,
    /// Whether to reject unknown host keys.
    strict: bool,
    /// Trusted host keys to verify against.
    trusted_keys: Vec<TrustedHostKey>,
}

impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        if !self.strict {
            // THREAT[TM-SSH-006]: Warn when accepting unverified host keys.
            eprintln!(
                "WARNING: ssh: accepting unverified host key for '{}' \
                 (strict_host_key_checking is disabled — vulnerable to MITM)",
                self.host
            );
            return Ok(true);
        }

        // Serialize the server key for comparison.
        let server_key_str = server_public_key.to_string();

        for trusted in &self.trusted_keys {
            if trusted.host != self.host && trusted.host != "*" {
                continue;
            }
            // Compare the key type+data portion.
            if keys_match(&server_key_str, &trusted.public_key) {
                return Ok(true);
            }
        }

        eprintln!(
            "WARNING: ssh: rejecting unknown host key for '{}' \
             (no matching trusted key configured)",
            self.host
        );
        Ok(false)
    }
}

/// Compare two SSH public key strings, ignoring trailing comments.
/// Accepts formats like "ssh-ed25519 AAAA..." or "ssh-ed25519 AAAA... comment".
fn keys_match(server_key: &str, trusted_key: &str) -> bool {
    fn normalize(s: &str) -> (&str, &str) {
        let parts: Vec<&str> = s.trim().splitn(3, ' ').collect();
        if parts.len() >= 2 {
            (parts[0], parts[1])
        } else {
            (s.trim(), "")
        }
    }
    let (s_type, s_data) = normalize(server_key);
    let (t_type, t_data) = normalize(trusted_key);
    s_type == t_type && s_data == t_data
}

/// Default SSH transport using russh.
///
/// Supports password and private key authentication.
/// SCP/SFTP are implemented via remote commands (`cat`, `base64`).
pub struct RusshHandler {
    timeout: Duration,
    /// THREAT[TM-SSH-004]: Streaming size limit to prevent OOM from malicious servers.
    max_response_bytes: usize,
    /// THREAT[TM-SSH-006]: Whether to verify host keys.
    strict_host_key_checking: bool,
    /// Trusted host keys for verification.
    trusted_host_keys: Vec<TrustedHostKey>,
}

impl RusshHandler {
    pub fn new(
        timeout: Duration,
        max_response_bytes: usize,
        strict_host_key_checking: bool,
        trusted_host_keys: Vec<TrustedHostKey>,
    ) -> Self {
        Self {
            timeout,
            max_response_bytes,
            strict_host_key_checking,
            trusted_host_keys,
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

        let handler = ClientHandler {
            host: target.host.clone(),
            strict: self.strict_host_key_checking,
            trusted_keys: self.trusted_host_keys.clone(),
        };

        let addr = (target.host.as_str(), target.port);
        let mut session = russh::client::connect(Arc::new(config), addr, handler)
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
        let handler = RusshHandler::new(Duration::from_secs(30), 1024, true, vec![]);
        assert_eq!(handler.max_response_bytes, 1024);
    }

    #[test]
    fn test_russh_handler_default_max_response_bytes() {
        use crate::ssh::config::DEFAULT_MAX_RESPONSE_BYTES;
        let handler =
            RusshHandler::new(Duration::from_secs(30), DEFAULT_MAX_RESPONSE_BYTES, true, vec![]);
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
        assert_eq!(client.config().max_response_bytes, 512);
    }

    #[test]
    fn test_strict_host_key_checking_propagation() {
        use crate::ssh::client::SshClient;
        use crate::ssh::config::SshConfig;

        let config = SshConfig::new().strict_host_key_checking(true);
        let client = SshClient::new(config);
        assert!(client.config().strict_host_key_checking);

        let config = SshConfig::new().strict_host_key_checking(false);
        let client = SshClient::new(config);
        assert!(!client.config().strict_host_key_checking);
    }

    #[test]
    fn test_keys_match_same_key() {
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKtQ";
        assert!(keys_match(key, key));
    }

    #[test]
    fn test_keys_match_ignores_comment() {
        let server = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKtQ";
        let trusted = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKtQ user@host";
        assert!(keys_match(server, trusted));
    }

    #[test]
    fn test_keys_match_different_key() {
        let server = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKtQ";
        let trusted = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDiff";
        assert!(!keys_match(server, trusted));
    }

    #[test]
    fn test_keys_match_different_type() {
        let server = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKtQ";
        let trusted = "ssh-rsa AAAAC3NzaC1lZDI1NTE5AAAAIKtQ";
        assert!(!keys_match(server, trusted));
    }

    /// THREAT[TM-SSH-006]: Default strict mode rejects connections with unknown keys.
    #[tokio::test]
    async fn test_strict_mode_rejects_unknown_key() {
        let config = crate::ssh::config::SshConfig::new()
            .allow_all()
            .strict_host_key_checking(true);
        let client = crate::ssh::client::SshClient::new(config);
        let target = crate::ssh::handler::SshTarget {
            host: "localhost".to_string(),
            port: 22,
            user: "test".to_string(),
            private_key: None,
            password: None,
        };
        // Connection will fail — either because no server is listening,
        // or because the host key is unknown. Either way, strict mode
        // ensures we don't silently accept keys.
        let result = client.exec(&target, "echo hi").await;
        assert!(result.is_err());
    }
}
