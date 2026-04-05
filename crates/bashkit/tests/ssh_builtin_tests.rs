//! Integration tests for SSH builtins (ssh, scp, sftp).
//!
//! Uses a mock SshHandler so these tests run without network access.

#[cfg(feature = "ssh")]
mod ssh_builtin_tests {
    use async_trait::async_trait;
    use bashkit::{Bash, SshConfig, SshHandler, SshOutput, SshTarget};

    struct RecordingHandler;

    #[async_trait]
    impl SshHandler for RecordingHandler {
        async fn exec(&self, target: &SshTarget, command: &str) -> Result<SshOutput, String> {
            Ok(SshOutput {
                stdout: format!(
                    "user={} host={} cmd={}\n",
                    target.user, target.host, command
                ),
                stderr: String::new(),
                exit_code: 0,
            })
        }

        async fn shell(&self, target: &SshTarget) -> Result<SshOutput, String> {
            Ok(SshOutput {
                stdout: format!("shell user={} host={}\n", target.user, target.host),
                stderr: String::new(),
                exit_code: 0,
            })
        }

        async fn upload(
            &self,
            _target: &SshTarget,
            remote_path: &str,
            content: &[u8],
            _mode: u32,
        ) -> Result<(), String> {
            if remote_path == "/fail" {
                return Err("permission denied".to_string());
            }
            assert!(!content.is_empty());
            Ok(())
        }

        async fn download(
            &self,
            _target: &SshTarget,
            remote_path: &str,
        ) -> Result<Vec<u8>, String> {
            if remote_path == "/missing" {
                return Err("no such file".to_string());
            }
            Ok(format!("content of {remote_path}\n").into_bytes())
        }
    }

    fn bash() -> Bash {
        Bash::builder()
            .ssh(
                SshConfig::new()
                    .allow("host.example.com")
                    .allow("*.allowed.co")
                    .default_user("testuser"),
            )
            .ssh_handler(Box::new(RecordingHandler))
            .build()
    }

    // ── SSH ──

    #[tokio::test]
    async fn ssh_basic_command() {
        let mut b = bash();
        let r = b.exec("ssh host.example.com ls -la").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("cmd=ls -la"));
        assert!(r.stdout.contains("user=testuser"));
    }

    #[tokio::test]
    async fn ssh_with_user() {
        let mut b = bash();
        let r = b.exec("ssh deploy@host.example.com whoami").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("user=deploy"));
    }

    #[tokio::test]
    async fn ssh_no_command_opens_shell() {
        let mut b = bash();
        let r = b.exec("ssh host.example.com").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(
            r.stdout
                .contains("shell user=testuser host=host.example.com")
        );
    }

    #[tokio::test]
    async fn ssh_heredoc() {
        let mut b = bash();
        let r = b
            .exec("ssh host.example.com <<'EOF'\necho hello\nEOF")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("cmd=echo hello"));
    }

    #[tokio::test]
    async fn ssh_blocked_host() {
        let mut b = bash();
        let r = b.exec("ssh evil.com 'id'").await.unwrap();
        assert_ne!(r.exit_code, 0);
        assert!(r.stderr.contains("not in allowlist"));
    }

    #[tokio::test]
    async fn ssh_wildcard_host() {
        let mut b = bash();
        let r = b.exec("ssh db.allowed.co uname").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("host=db.allowed.co"));
    }

    #[tokio::test]
    async fn ssh_no_host() {
        let mut b = bash();
        let r = b.exec("ssh").await.unwrap();
        assert_ne!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn ssh_port_flag() {
        let mut b = Bash::builder()
            .ssh(
                SshConfig::new()
                    .allow("host.example.com")
                    .allow_port(22)
                    .allow_port(2222)
                    .default_user("u"),
            )
            .ssh_handler(Box::new(RecordingHandler))
            .build();
        let r = b.exec("ssh host.example.com echo ok").await.unwrap();
        assert_eq!(r.exit_code, 0);
        let r = b
            .exec("ssh -p 2222 host.example.com echo ok")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        let r = b
            .exec("ssh -p 3333 host.example.com echo ok")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn ssh_port_flag_missing_arg() {
        let mut b = bash();
        let r = b.exec("ssh -p").await.unwrap();
        assert_ne!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn ssh_pipe_output() {
        let mut b = bash();
        let r = b
            .exec("ssh host.example.com echo hello | tr h H")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
    }

    // ── SCP ──

    #[tokio::test]
    async fn scp_upload() {
        let mut b = bash();
        b.exec("echo 'data' > /tmp/local.txt").await.unwrap();
        let r = b
            .exec("scp /tmp/local.txt host.example.com:/remote/path.txt")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn scp_download() {
        let mut b = bash();
        let r = b
            .exec("scp host.example.com:/etc/config.txt /tmp/downloaded.txt")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        let cat = b.exec("cat /tmp/downloaded.txt").await.unwrap();
        assert!(cat.stdout.contains("content of /etc/config.txt"));
    }

    #[tokio::test]
    async fn scp_download_missing() {
        let mut b = bash();
        let r = b
            .exec("scp host.example.com:/missing /tmp/out.txt")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn scp_no_remote() {
        let mut b = bash();
        let r = b.exec("scp file1 file2").await.unwrap();
        assert_ne!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn scp_too_few_args() {
        let mut b = bash();
        let r = b.exec("scp file1").await.unwrap();
        assert_ne!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn scp_blocked_host() {
        let mut b = bash();
        b.exec("echo x > /tmp/f.txt").await.unwrap();
        let r = b.exec("scp /tmp/f.txt evil.com:/tmp/f.txt").await.unwrap();
        assert_ne!(r.exit_code, 0);
    }

    // ── SFTP ──

    #[tokio::test]
    async fn sftp_put() {
        let mut b = bash();
        b.exec("echo 'data' > /tmp/upload.txt").await.unwrap();
        let r = b
            .exec("sftp host.example.com <<'EOF'\nput /tmp/upload.txt /remote/upload.txt\nEOF")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn sftp_get() {
        let mut b = bash();
        let r = b
            .exec("sftp host.example.com <<'EOF'\nget /remote/data.txt /tmp/fetched.txt\nEOF")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        let cat = b.exec("cat /tmp/fetched.txt").await.unwrap();
        assert!(cat.stdout.contains("content of /remote/data.txt"));
    }

    #[tokio::test]
    async fn sftp_ls() {
        let mut b = bash();
        let r = b
            .exec("sftp host.example.com <<'EOF'\nls /var\nEOF")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        // Path is shell-escaped (TM-SSH-008)
        assert!(
            r.stdout.contains("cmd=ls -la"),
            "expected ls command in output, got: {}",
            r.stdout
        );
    }

    #[tokio::test]
    async fn sftp_unsupported_command() {
        let mut b = bash();
        let r = b
            .exec("sftp host.example.com <<'EOF'\nrm /tmp/x\nEOF")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn sftp_no_stdin() {
        let mut b = bash();
        let r = b.exec("sftp host.example.com").await.unwrap();
        assert_ne!(r.exit_code, 0);
    }

    // ── Not configured ──

    #[tokio::test]
    async fn ssh_not_configured() {
        let mut b = Bash::new();
        let r = b.exec("ssh host.example.com ls").await.unwrap();
        assert_ne!(r.exit_code, 0);
        assert!(r.stderr.contains("not configured"));
    }

    #[tokio::test]
    async fn scp_not_configured() {
        let mut b = Bash::new();
        let r = b.exec("scp file host.example.com:/path").await.unwrap();
        assert_ne!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn sftp_not_configured() {
        let mut b = Bash::new();
        let r = b.exec("sftp host.example.com").await.unwrap();
        assert_ne!(r.exit_code, 0);
    }
}
