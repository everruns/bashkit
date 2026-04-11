//! Integration tests for `ssh supabase.sh`.
//!
//! Requires `ssh` feature. No credentials needed — supabase.sh is a public SSH service.

#[cfg(feature = "ssh")]
mod ssh_supabase {
    use bashkit::{Bash, SshConfig};

    fn bash_with_supabase() -> Bash {
        Bash::builder()
            .ssh(SshConfig::new().allow("supabase.sh"))
            .build()
    }

    /// Connects to supabase.sh via SSH. Verifies the connection succeeds.
    /// supabase.sh is a TUI service — it may not send output without an
    /// interactive terminal, so we only assert the connection didn't error.
    ///
    /// Ignored by default: requires network access to supabase.sh.
    /// CI runs this explicitly via `cargo test --features ssh -p bashkit --test ssh_supabase_tests`.
    #[tokio::test]
    #[ignore]
    async fn ssh_supabase_connects() {
        let mut bash = bash_with_supabase();
        let result = bash.exec("ssh supabase.sh").await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "ssh supabase.sh failed: {}",
            result.stderr
        );
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
