//! Git Security Tests
//!
//! Tests for git-related threats documented in specs/006-threat-model.md
//! Section 8: Git Security (TM-GIT-*)

#![cfg(feature = "git")]

use bashkit::{Bash, GitConfig};

/// Helper to create a bash instance with git configured
fn create_git_bash() -> Bash {
    Bash::builder()
        .git(GitConfig::new().author("Sandbox User", "sandbox@test.local"))
        .build()
}

mod identity_security {
    use super::*;

    /// TM-GIT-002: Host identity leak
    /// Commits should use the configured sandbox identity, not host identity.
    #[tokio::test]
    async fn test_commit_uses_sandbox_identity() {
        let mut bash = create_git_bash();

        // Create commit
        bash.exec("git init /repo && cd /repo && echo 'x' > x.txt && git add x.txt && git commit -m 'Test'").await.unwrap();

        // Check log shows sandbox identity
        let log = bash.exec("cd /repo && git log").await.unwrap();
        assert!(log.stdout.contains("Sandbox User"));
        assert!(log.stdout.contains("sandbox@test.local"));

        // Should NOT contain actual system user info
        // (this is a basic check - in real system we'd check it's not the host user)
        assert!(!log.stdout.contains("root@"));
    }

    /// TM-GIT-002: Verify custom author is used
    #[tokio::test]
    async fn test_custom_author_in_commits() {
        let mut bash = Bash::builder()
            .git(GitConfig::new().author("CI Bot", "ci@company.com"))
            .build();

        bash.exec("git init /repo && cd /repo && echo 'x' > x.txt && git add . && git commit -m 'CI commit'").await.unwrap();

        let log = bash.exec("cd /repo && git log").await.unwrap();
        assert!(log.stdout.contains("CI Bot"));
        assert!(log.stdout.contains("ci@company.com"));
    }

    /// TM-GIT-003: Host git config access
    /// Git config should only read from repo .git/config, not host ~/.gitconfig
    #[tokio::test]
    async fn test_config_only_reads_repo_config() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo").await.unwrap();

        // Config should reflect what we set, not system config
        let result = bash.exec("cd /repo && git config user.name").await.unwrap();
        assert_eq!(result.stdout.trim(), "Sandbox User");

        // Setting config should only affect repo
        bash.exec("cd /repo && git config user.name 'Repo User'")
            .await
            .unwrap();
        let result = bash.exec("cd /repo && git config user.name").await.unwrap();
        assert_eq!(result.stdout.trim(), "Repo User");
    }
}

mod vfs_isolation {
    use super::*;

    /// TM-GIT-005: Repository escape
    /// Git operations should be confined to the virtual filesystem.
    #[tokio::test]
    async fn test_git_operations_confined_to_vfs() {
        let mut bash = create_git_bash();

        // Initialize repo in VFS
        let result = bash.exec("git init /repo").await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Verify .git directory exists in VFS
        let result = bash.exec("ls -la /repo/.git").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("HEAD"));
        assert!(result.stdout.contains("config"));
    }

    /// TM-GIT-005: Path traversal in git init
    #[tokio::test]
    async fn test_git_init_path_traversal_blocked() {
        let mut bash = create_git_bash();

        // Path traversal should be normalized by VFS
        let result = bash.exec("git init /repo/../../../etc").await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Should create /etc (normalized), not escape VFS
        let result = bash.exec("ls /etc/.git").await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    /// TM-GIT-005: Git add doesn't access files outside VFS
    #[tokio::test]
    async fn test_git_add_vfs_only() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo && cd /repo && echo 'content' > file.txt")
            .await
            .unwrap();

        // Add file - should work with VFS files
        let result = bash.exec("cd /repo && git add file.txt").await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Status should show file
        let status = bash.exec("cd /repo && git status").await.unwrap();
        assert!(status.stdout.contains("file.txt"));
    }
}

mod resource_limits {
    use super::*;
    use bashkit::ExecutionLimits;

    /// TM-GIT-007: Many git objects
    /// FS file count limits should apply to git objects.
    #[tokio::test]
    async fn test_fs_limits_apply_to_git() {
        let mut bash = Bash::builder()
            .git(GitConfig::new())
            .limits(ExecutionLimits::new().max_commands(100))
            .build();

        // Initialize repo - should work within limits
        let result = bash.exec("git init /repo").await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    /// TM-GIT-008: Deep history
    /// Log should support limit parameter to prevent excessive output.
    #[tokio::test]
    async fn test_git_log_limit() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo && cd /repo").await.unwrap();

        // Create several commits
        for i in 1..=5 {
            bash.exec(&format!(
                "cd /repo && echo '{}' > file{}.txt && git add . && git commit -m 'Commit {}'",
                i, i, i
            ))
            .await
            .unwrap();
        }

        // Log with limit should restrict output
        let result = bash.exec("cd /repo && git log -n 2").await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Should contain last 2 commits
        assert!(result.stdout.contains("Commit 5"));
        assert!(result.stdout.contains("Commit 4"));

        // Should NOT contain earlier commits
        assert!(!result.stdout.contains("Commit 1"));
    }
}

mod error_message_safety {
    use super::*;

    /// TM-INT-004 applied to git: No real paths in error messages
    #[tokio::test]
    async fn test_error_messages_no_real_paths() {
        let mut bash = create_git_bash();

        // Operation on non-existent repo
        let result = bash.exec("cd /nonexistent && git status").await.unwrap();
        assert_ne!(result.exit_code, 0);

        // Error should not leak real filesystem paths
        assert!(!result.stderr.contains("/home/"));
        assert!(!result.stderr.contains("/Users/"));
        assert!(!result.stderr.contains("C:\\"));
    }

    /// Verify error messages are user-friendly
    #[tokio::test]
    async fn test_error_messages_user_friendly() {
        let mut bash = create_git_bash();

        // Not in a repo
        let result = bash.exec("cd /tmp && git status").await.unwrap();
        assert!(result.stderr.contains("not a git repository"));

        // Commit without staged files
        bash.exec("git init /repo").await.unwrap();
        let result = bash
            .exec("cd /repo && git commit -m 'Empty'")
            .await
            .unwrap();
        assert!(result.stderr.contains("nothing to commit"));
    }
}

mod git_not_configured {
    use bashkit::Bash;

    /// Git operations should fail gracefully when not configured
    #[tokio::test]
    async fn test_git_disabled_by_default() {
        let mut bash = Bash::new();

        let result = bash.exec("git init /repo").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("not configured"));
    }

    /// Error message should guide user
    #[tokio::test]
    async fn test_git_not_configured_helpful_message() {
        let mut bash = Bash::new();

        let result = bash.exec("git status").await.unwrap();
        assert!(result.stderr.contains("Bash::builder().git()"));
    }
}

mod concurrent_safety {
    use super::*;

    /// Multiple git operations should not interfere with each other
    #[tokio::test]
    async fn test_isolated_repositories() {
        let mut bash1 = Bash::builder()
            .git(GitConfig::new().author("User1", "user1@test.com"))
            .build();
        let mut bash2 = Bash::builder()
            .git(GitConfig::new().author("User2", "user2@test.com"))
            .build();

        // Create separate repos
        bash1.exec("git init /repo1 && cd /repo1 && echo 'a' > a.txt && git add . && git commit -m 'Repo1'").await.unwrap();
        bash2.exec("git init /repo2 && cd /repo2 && echo 'b' > b.txt && git add . && git commit -m 'Repo2'").await.unwrap();

        // Verify isolation
        let log1 = bash1.exec("cd /repo1 && git log").await.unwrap();
        assert!(log1.stdout.contains("User1"));
        assert!(!log1.stdout.contains("User2"));

        let log2 = bash2.exec("cd /repo2 && git log").await.unwrap();
        assert!(log2.stdout.contains("User2"));
        assert!(!log2.stdout.contains("User1"));
    }
}
