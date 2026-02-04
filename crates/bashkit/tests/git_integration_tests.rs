//! Git Integration Tests
//!
//! Tests for git builtin functionality (Phase 1: local operations).

#![cfg(feature = "git")]

use bashkit::{Bash, GitConfig};

/// Helper to create a bash instance with git configured
fn create_git_bash() -> Bash {
    Bash::builder()
        .git(GitConfig::new().author("Test User", "test@example.com"))
        .build()
}

mod init {
    use super::*;

    #[tokio::test]
    async fn test_git_init_creates_repository() {
        let mut bash = create_git_bash();

        let result = bash.exec("git init /repo").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Initialized empty Git repository"));
    }

    #[tokio::test]
    async fn test_git_init_in_current_directory() {
        let mut bash = create_git_bash();

        // Create and cd to directory first
        let result = bash
            .exec("mkdir -p /myrepo && cd /myrepo && git init")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Initialized empty Git repository"));
    }

    #[tokio::test]
    async fn test_git_init_reinitialize() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo").await.unwrap();
        let result = bash.exec("git init /repo").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result
            .stdout
            .contains("Reinitialized existing Git repository"));
    }
}

mod config {
    use super::*;

    #[tokio::test]
    async fn test_git_config_get_user_name() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo").await.unwrap();
        let result = bash.exec("cd /repo && git config user.name").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "Test User");
    }

    #[tokio::test]
    async fn test_git_config_get_user_email() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo").await.unwrap();
        let result = bash
            .exec("cd /repo && git config user.email")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "test@example.com");
    }

    #[tokio::test]
    async fn test_git_config_set() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo").await.unwrap();
        bash.exec("cd /repo && git config user.name 'New Name'")
            .await
            .unwrap();
        let result = bash.exec("cd /repo && git config user.name").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "New Name");
    }

    #[tokio::test]
    async fn test_git_config_not_in_repo() {
        let mut bash = create_git_bash();

        let result = bash.exec("cd /tmp && git config user.name").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("not a git repository"));
    }
}

mod add_commit {
    use super::*;

    #[tokio::test]
    async fn test_git_add_single_file() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo && cd /repo && echo 'hello' > test.txt")
            .await
            .unwrap();
        let result = bash.exec("cd /repo && git add test.txt").await.unwrap();
        assert_eq!(result.exit_code, 0);

        let status = bash.exec("cd /repo && git status").await.unwrap();
        assert!(status.stdout.contains("test.txt"));
        assert!(status.stdout.contains("Changes to be committed"));
    }

    #[tokio::test]
    async fn test_git_add_all() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo && cd /repo && echo 'a' > a.txt && echo 'b' > b.txt")
            .await
            .unwrap();
        let result = bash.exec("cd /repo && git add .").await.unwrap();
        assert_eq!(result.exit_code, 0);

        let status = bash.exec("cd /repo && git status").await.unwrap();
        assert!(status.stdout.contains("a.txt"));
        assert!(status.stdout.contains("b.txt"));
    }

    #[tokio::test]
    async fn test_git_commit_with_message() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo && cd /repo && echo 'hello' > test.txt && git add test.txt")
            .await
            .unwrap();
        let result = bash
            .exec("cd /repo && git commit -m 'Initial commit'")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("[master"));
        assert!(result.stdout.contains("Initial commit"));
    }

    #[tokio::test]
    async fn test_git_commit_nothing_to_commit() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo").await.unwrap();
        let result = bash
            .exec("cd /repo && git commit -m 'Empty'")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("nothing to commit"));
    }

    #[tokio::test]
    async fn test_git_commit_requires_message() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo && cd /repo && echo 'x' > x.txt && git add x.txt")
            .await
            .unwrap();
        let result = bash.exec("cd /repo && git commit").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("requires a value"));
    }
}

mod status {
    use super::*;

    #[tokio::test]
    async fn test_git_status_clean() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo").await.unwrap();
        let result = bash.exec("cd /repo && git status").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("On branch master"));
        assert!(result.stdout.contains("nothing to commit"));
    }

    #[tokio::test]
    async fn test_git_status_untracked() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo && cd /repo && echo 'x' > new.txt")
            .await
            .unwrap();
        let result = bash.exec("cd /repo && git status").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Untracked files"));
        assert!(result.stdout.contains("new.txt"));
    }

    #[tokio::test]
    async fn test_git_status_staged() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo && cd /repo && echo 'x' > new.txt && git add new.txt")
            .await
            .unwrap();
        let result = bash.exec("cd /repo && git status").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Changes to be committed"));
        assert!(result.stdout.contains("new.txt"));
    }

    #[tokio::test]
    async fn test_git_status_not_in_repo() {
        let mut bash = create_git_bash();

        let result = bash.exec("cd /tmp && git status").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("not a git repository"));
    }
}

mod log {
    use super::*;

    #[tokio::test]
    async fn test_git_log_after_commit() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo && cd /repo && echo 'x' > x.txt && git add x.txt && git commit -m 'First'").await.unwrap();
        let result = bash.exec("cd /repo && git log").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("commit"));
        assert!(result.stdout.contains("Author: Test User"));
        assert!(result.stdout.contains("First"));
    }

    #[tokio::test]
    async fn test_git_log_multiple_commits() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo && cd /repo").await.unwrap();
        bash.exec("cd /repo && echo 'a' > a.txt && git add a.txt && git commit -m 'First'")
            .await
            .unwrap();
        bash.exec("cd /repo && echo 'b' > b.txt && git add b.txt && git commit -m 'Second'")
            .await
            .unwrap();

        let result = bash.exec("cd /repo && git log").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("First"));
        assert!(result.stdout.contains("Second"));
    }

    #[tokio::test]
    async fn test_git_log_limit() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo && cd /repo").await.unwrap();
        bash.exec("cd /repo && echo 'a' > a.txt && git add a.txt && git commit -m 'First'")
            .await
            .unwrap();
        bash.exec("cd /repo && echo 'b' > b.txt && git add b.txt && git commit -m 'Second'")
            .await
            .unwrap();
        bash.exec("cd /repo && echo 'c' > c.txt && git add c.txt && git commit -m 'Third'")
            .await
            .unwrap();

        let result = bash.exec("cd /repo && git log -n 1").await.unwrap();
        assert_eq!(result.exit_code, 0);
        // Should only show the most recent commit
        assert!(result.stdout.contains("Third"));
        // First commit should not appear with -n 1
        let first_count = result.stdout.matches("First").count();
        assert_eq!(first_count, 0);
    }

    #[tokio::test]
    async fn test_git_log_no_commits() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo").await.unwrap();
        let result = bash.exec("cd /repo && git log").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("does not have any commits"));
    }
}

mod workflow {
    use super::*;

    #[tokio::test]
    async fn test_full_workflow() {
        let mut bash = create_git_bash();

        // Initialize repository
        let result = bash.exec("git init /project").await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Create files
        bash.exec("cd /project && echo '# My Project' > README.md")
            .await
            .unwrap();
        bash.exec("cd /project && echo 'fn main() {}' > main.rs")
            .await
            .unwrap();

        // Check status shows untracked
        let status = bash.exec("cd /project && git status").await.unwrap();
        assert!(status.stdout.contains("Untracked files"));

        // Add files
        let result = bash.exec("cd /project && git add .").await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Check status shows staged
        let status = bash.exec("cd /project && git status").await.unwrap();
        assert!(status.stdout.contains("Changes to be committed"));

        // Commit
        let result = bash
            .exec("cd /project && git commit -m 'Initial project setup'")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);

        // Status should be clean
        let status = bash.exec("cd /project && git status").await.unwrap();
        assert!(status.stdout.contains("nothing to commit"));

        // Log shows commit
        let log = bash.exec("cd /project && git log").await.unwrap();
        assert!(log.stdout.contains("Initial project setup"));
    }
}

mod error_handling {
    use super::*;

    #[tokio::test]
    async fn test_git_unknown_command() {
        let mut bash = create_git_bash();

        let result = bash.exec("git unknown").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("is not a git command"));
    }

    #[tokio::test]
    async fn test_git_no_subcommand() {
        let mut bash = create_git_bash();

        let result = bash.exec("git").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("usage: git"));
    }

    #[tokio::test]
    async fn test_git_add_nothing() {
        let mut bash = create_git_bash();

        bash.exec("git init /repo").await.unwrap();
        let result = bash.exec("cd /repo && git add").await.unwrap();
        // git add with no args should warn but not fail
        assert!(result.stderr.contains("Nothing specified"));
    }
}

mod not_configured {
    use bashkit::Bash;

    #[tokio::test]
    async fn test_git_not_configured() {
        // Create bash without git configuration
        let mut bash = Bash::new();

        let result = bash.exec("git init /repo").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("not configured"));
    }
}
