//! Git Remote Security Tests
//!
//! Tests for git remote-related threats documented in specs/006-threat-model.md
//! Section 8: Git Security (TM-GIT-010 through TM-GIT-013)

#![cfg(feature = "git")]

use bashkit::{Bash, GitConfig};

/// Helper to create a bash instance with git configured and allowlist
fn create_git_bash_with_allowlist(allowed: &[&str]) -> Bash {
    let mut config = GitConfig::new().author("Test User", "test@example.com");
    for pattern in allowed {
        config = config.allow_remote(*pattern);
    }
    Bash::builder().git(config).build()
}

/// Helper to create a bash instance with all remotes allowed
fn create_git_bash_allow_all() -> Bash {
    Bash::builder()
        .git(
            GitConfig::new()
                .author("Test User", "test@example.com")
                .allow_all_remotes(),
        )
        .build()
}

/// Helper to create a bash instance with no remote access
fn create_git_bash_no_remote() -> Bash {
    Bash::builder()
        .git(GitConfig::new().author("Test User", "test@example.com"))
        .build()
}

mod url_allowlist {
    use super::*;

    /// TM-GIT-010, TM-GIT-011: Empty allowlist blocks all remote URLs
    #[tokio::test]
    async fn test_empty_allowlist_blocks_all() {
        let mut bash = create_git_bash_no_remote();

        // Initialize repo and add remote
        bash.exec("git init /repo").await.unwrap();

        // Adding remote with HTTPS should fail (no URLs allowed)
        let result = bash
            .exec("cd /repo && git remote add origin https://github.com/org/repo.git")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("no remote URLs are allowed"));
    }

    /// TM-GIT-010, TM-GIT-011: Only allowed URL patterns succeed
    #[tokio::test]
    async fn test_allowlist_pattern_matching() {
        let mut bash = create_git_bash_with_allowlist(&["https://github.com/allowed-org/"]);

        bash.exec("git init /repo").await.unwrap();

        // Allowed org should succeed
        let result = bash
            .exec("cd /repo && git remote add origin https://github.com/allowed-org/repo.git")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);

        // Different org should fail
        let result = bash
            .exec("cd /repo && git remote add other https://github.com/other-org/repo.git")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("not in allowlist"));
    }

    /// TM-GIT-010, TM-GIT-011: Multiple allowed patterns
    #[tokio::test]
    async fn test_multiple_allowlist_patterns() {
        let mut bash = create_git_bash_with_allowlist(&[
            "https://github.com/org1/",
            "https://github.com/org2/",
            "https://gitlab.com/",
        ]);

        bash.exec("git init /repo").await.unwrap();

        // All allowed patterns should work
        let r1 = bash
            .exec("cd /repo && git remote add gh1 https://github.com/org1/repo.git")
            .await
            .unwrap();
        assert_eq!(r1.exit_code, 0);

        let r2 = bash
            .exec("cd /repo && git remote add gh2 https://github.com/org2/repo.git")
            .await
            .unwrap();
        assert_eq!(r2.exit_code, 0);

        let r3 = bash
            .exec("cd /repo && git remote add gl https://gitlab.com/any/repo.git")
            .await
            .unwrap();
        assert_eq!(r3.exit_code, 0);

        // Non-allowed should fail
        let r4 = bash
            .exec("cd /repo && git remote add bitbucket https://bitbucket.org/any/repo.git")
            .await
            .unwrap();
        assert_ne!(r4.exit_code, 0);
    }
}

mod protocol_enforcement {
    use super::*;

    /// TM-GIT-012: SSH URLs should be blocked
    #[tokio::test]
    async fn test_ssh_urls_blocked() {
        let mut bash = create_git_bash_allow_all();

        bash.exec("git init /repo").await.unwrap();

        // SSH URL should fail
        let result = bash
            .exec("cd /repo && git remote add origin git@github.com:org/repo.git")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("only HTTPS URLs are allowed"));
    }

    /// TM-GIT-013: git:// protocol should be blocked
    #[tokio::test]
    async fn test_git_protocol_blocked() {
        let mut bash = create_git_bash_allow_all();

        bash.exec("git init /repo").await.unwrap();

        // git:// protocol should fail
        let result = bash
            .exec("cd /repo && git remote add origin git://github.com/org/repo.git")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("only HTTPS URLs are allowed"));
    }

    /// TM-GIT-012, TM-GIT-013: Only HTTPS allowed
    #[tokio::test]
    async fn test_https_allowed() {
        let mut bash = create_git_bash_allow_all();

        bash.exec("git init /repo").await.unwrap();

        // HTTPS should work
        let result = bash
            .exec("cd /repo && git remote add origin https://github.com/org/repo.git")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }
}

mod remote_management {
    use super::*;

    /// git remote add/remove functionality
    #[tokio::test]
    async fn test_remote_add_remove() {
        let mut bash = create_git_bash_allow_all();

        bash.exec("git init /repo").await.unwrap();

        // Add remote
        let result = bash
            .exec("cd /repo && git remote add origin https://github.com/org/repo.git")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);

        // List remotes
        let result = bash.exec("cd /repo && git remote").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("origin"));

        // List with URLs
        let result = bash.exec("cd /repo && git remote -v").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("https://github.com/org/repo.git"));

        // Remove remote
        let result = bash
            .exec("cd /repo && git remote remove origin")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);

        // Verify removed
        let result = bash.exec("cd /repo && git remote").await.unwrap();
        assert!(!result.stdout.contains("origin"));
    }

    /// git remote add duplicate should fail
    #[tokio::test]
    async fn test_remote_add_duplicate() {
        let mut bash = create_git_bash_allow_all();

        bash.exec("git init /repo").await.unwrap();

        // Add remote
        bash.exec("cd /repo && git remote add origin https://github.com/org/repo.git")
            .await
            .unwrap();

        // Add same remote again should fail
        let result = bash
            .exec("cd /repo && git remote add origin https://github.com/other/repo.git")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("already exists"));
    }

    /// git remote remove non-existent should fail
    #[tokio::test]
    async fn test_remote_remove_nonexistent() {
        let mut bash = create_git_bash_allow_all();

        bash.exec("git init /repo").await.unwrap();

        // Remove non-existent remote
        let result = bash
            .exec("cd /repo && git remote remove nonexistent")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("no such remote"));
    }
}

mod network_operations {
    use super::*;

    /// git clone with valid URL returns virtual mode message
    #[tokio::test]
    async fn test_clone_sandbox_message() {
        let mut bash = create_git_bash_allow_all();

        let result = bash
            .exec("git clone https://github.com/org/repo.git")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("network operations not supported"));
        assert!(result.stderr.contains("passed allowlist validation"));
    }

    /// git clone with invalid URL returns allowlist error
    #[tokio::test]
    async fn test_clone_invalid_url() {
        let mut bash = create_git_bash_with_allowlist(&["https://github.com/allowed/"]);

        let result = bash
            .exec("git clone https://github.com/blocked/repo.git")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("not in allowlist"));
    }

    /// git fetch/push/pull validate remote URL
    #[tokio::test]
    async fn test_fetch_push_pull_validate_url() {
        let mut bash = create_git_bash_allow_all();

        bash.exec("git init /repo").await.unwrap();
        bash.exec("cd /repo && git remote add origin https://github.com/org/repo.git")
            .await
            .unwrap();

        // Fetch should validate and return virtual mode message
        let result = bash.exec("cd /repo && git fetch origin").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("network operations not supported"));
        assert!(result.stderr.contains("passed allowlist validation"));

        // Push should validate and return virtual mode message
        let result = bash.exec("cd /repo && git push origin").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("network operations not supported"));

        // Pull should validate and return virtual mode message
        let result = bash.exec("cd /repo && git pull origin").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("network operations not supported"));
    }

    /// git fetch/push/pull with non-existent remote
    #[tokio::test]
    async fn test_network_ops_nonexistent_remote() {
        let mut bash = create_git_bash_allow_all();

        bash.exec("git init /repo").await.unwrap();

        // Fetch non-existent remote
        let result = bash
            .exec("cd /repo && git fetch nonexistent")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("not found"));

        // Push non-existent remote
        let result = bash.exec("cd /repo && git push nonexistent").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("not found"));
    }
}
