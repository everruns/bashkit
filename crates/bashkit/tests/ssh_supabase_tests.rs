//! Integration tests for `ssh supabase.sh`.
//!
//! Requires `ssh` feature. No credentials needed — supabase.sh is a public SSH service.
//! CI decision: the live public endpoint can reset connections, so the live
//! test uses a small bounded retry while still remaining a required gate.

#[cfg(feature = "ssh")]
mod ssh_supabase {
    use bashkit::{Bash, SshConfig};

    fn bash_with_supabase() -> Bash {
        Bash::builder()
            .ssh(
                SshConfig::new()
                    .allow("supabase.sh")
                    .strict_host_key_checking(false),
            )
            .build()
    }

    /// Connects to supabase.sh via SSH. Verifies the connection succeeds.
    /// supabase.sh is a TUI service — it may not send output without an
    /// interactive terminal, so we only assert the connection didn't error.
    #[tokio::test]
    async fn ssh_supabase_connects() {
        let mut last_stderr = String::new();

        for attempt in 1..=3 {
            let mut bash = bash_with_supabase();
            let result = bash.exec("ssh supabase.sh").await.unwrap();
            if result.exit_code == 0 {
                return;
            }

            last_stderr = result.stderr;
            if attempt < 3 {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        }

        panic!("ssh supabase.sh failed after 3 attempts: {last_stderr}");
    }

    #[tokio::test]
    async fn ssh_blocked_host_rejected() {
        let mut bash = bash_with_supabase();
        let result = bash.exec("ssh evil.com 'id'").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(
            result.stderr.contains("not in allowlist"),
            "expected allowlist error, got: {}",
            result.stderr
        );
    }
}
