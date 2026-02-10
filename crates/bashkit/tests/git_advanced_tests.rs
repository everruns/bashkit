//! Git Advanced Operations Tests (Phase 3)
//!
//! Tests for git branch, checkout, diff, and reset commands.

#![cfg(feature = "git")]

use bashkit::{Bash, GitConfig};

/// Helper to create a bash instance with git configured
fn create_git_bash() -> Bash {
    Bash::builder()
        .git(
            GitConfig::new()
                .author("Test User", "test@example.com")
                .allow_all_remotes(),
        )
        .build()
}

/// Helper to set up a repo with an initial commit
async fn setup_repo_with_commit(bash: &mut Bash) {
    bash.exec("git init /repo").await.unwrap();
    bash.exec("echo 'hello' > /repo/README.md").await.unwrap();
    bash.exec("cd /repo && git add README.md").await.unwrap();
    bash.exec("cd /repo && git commit -m 'Initial commit'")
        .await
        .unwrap();
}

mod branch_operations {
    use super::*;

    #[tokio::test]
    async fn test_branch_list_default() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        let result = bash.exec("cd /repo && git branch").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("* master"));
    }

    #[tokio::test]
    async fn test_branch_create() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        let result = bash.exec("cd /repo && git branch feature").await.unwrap();
        assert_eq!(result.exit_code, 0);

        let result = bash.exec("cd /repo && git branch").await.unwrap();
        assert!(result.stdout.contains("* master"));
        assert!(result.stdout.contains("feature"));
    }

    #[tokio::test]
    async fn test_branch_create_duplicate() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        bash.exec("cd /repo && git branch feature").await.unwrap();
        let result = bash.exec("cd /repo && git branch feature").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("already exists"));
    }

    #[tokio::test]
    async fn test_branch_delete() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        bash.exec("cd /repo && git branch feature").await.unwrap();
        let result = bash
            .exec("cd /repo && git branch -d feature")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Deleted branch feature"));

        let result = bash.exec("cd /repo && git branch").await.unwrap();
        assert!(!result.stdout.contains("feature"));
    }

    #[tokio::test]
    async fn test_branch_delete_current() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        let result = bash.exec("cd /repo && git branch -d master").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("cannot delete branch"));
    }

    #[tokio::test]
    async fn test_branch_delete_nonexistent() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        let result = bash
            .exec("cd /repo && git branch -d nonexistent")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("not found"));
    }

    #[tokio::test]
    async fn test_branch_no_commits() {
        let mut bash = create_git_bash();
        bash.exec("git init /repo").await.unwrap();

        // Creating branch without commits should fail
        let result = bash.exec("cd /repo && git branch feature").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("not a valid object name"));
    }
}

mod checkout_operations {
    use super::*;

    #[tokio::test]
    async fn test_checkout_branch() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        bash.exec("cd /repo && git branch feature").await.unwrap();
        let result = bash.exec("cd /repo && git checkout feature").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Switched to branch 'feature'"));

        // Verify current branch
        let result = bash.exec("cd /repo && git branch").await.unwrap();
        assert!(result.stdout.contains("* feature"));
    }

    #[tokio::test]
    async fn test_checkout_create_branch() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        let result = bash
            .exec("cd /repo && git checkout -b newbranch")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Switched to branch 'newbranch'"));

        let result = bash.exec("cd /repo && git branch").await.unwrap();
        assert!(result.stdout.contains("* newbranch"));
    }

    #[tokio::test]
    async fn test_checkout_nonexistent() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        let result = bash
            .exec("cd /repo && git checkout nonexistent")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("did not match"));
    }

    #[tokio::test]
    async fn test_checkout_no_args() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        let result = bash.exec("cd /repo && git checkout").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("must specify"));
    }

    #[tokio::test]
    async fn test_checkout_commit_hash() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        // Get the commit hash
        let log_result = bash.exec("cd /repo && git log -1").await.unwrap();
        // Extract hash from "commit <hash>" line
        let hash = log_result
            .stdout
            .lines()
            .find(|l| l.starts_with("commit"))
            .map(|l| l.strip_prefix("commit ").unwrap_or(l).trim())
            .unwrap_or("abcd1234");

        // Checkout by hash (detached HEAD)
        let result = bash
            .exec(&format!("cd /repo && git checkout {}", hash))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("detached HEAD"));
    }
}

mod diff_operations {
    use super::*;

    #[tokio::test]
    async fn test_diff_basic() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        let result = bash.exec("cd /repo && git diff").await.unwrap();
        assert_eq!(result.exit_code, 0);
        // Simplified diff in virtual mode
        assert!(result.stdout.contains("Diff output"));
    }

    #[tokio::test]
    async fn test_diff_not_a_repo() {
        let mut bash = create_git_bash();

        let result = bash.exec("cd / && git diff").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("not a git repository"));
    }
}

mod reset_operations {
    use super::*;

    #[tokio::test]
    async fn test_reset_soft() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        // Stage a file
        bash.exec("echo 'new' > /repo/new.txt").await.unwrap();
        bash.exec("cd /repo && git add new.txt").await.unwrap();

        // Reset soft
        let result = bash.exec("cd /repo && git reset --soft").await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_reset_mixed() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        // Stage a file
        bash.exec("echo 'new' > /repo/new.txt").await.unwrap();
        bash.exec("cd /repo && git add new.txt").await.unwrap();

        // Reset mixed (default)
        let result = bash.exec("cd /repo && git reset --mixed").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Unstaged changes"));
    }

    #[tokio::test]
    async fn test_reset_hard() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        // Stage a file
        bash.exec("echo 'new' > /repo/new.txt").await.unwrap();
        bash.exec("cd /repo && git add new.txt").await.unwrap();

        // Reset hard
        let result = bash.exec("cd /repo && git reset --hard").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("HEAD is now at"));
    }

    #[tokio::test]
    async fn test_reset_invalid_mode() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        let result = bash.exec("cd /repo && git reset --invalid").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("unknown switch"));
    }

    #[tokio::test]
    async fn test_reset_clears_staging() {
        let mut bash = create_git_bash();
        setup_repo_with_commit(&mut bash).await;

        // Stage a file
        bash.exec("echo 'new' > /repo/new.txt").await.unwrap();
        bash.exec("cd /repo && git add new.txt").await.unwrap();

        // Verify staged
        let result = bash.exec("cd /repo && git status").await.unwrap();
        assert!(result.stdout.contains("new.txt"));
        assert!(result.stdout.contains("Changes to be committed"));

        // Reset
        bash.exec("cd /repo && git reset --mixed").await.unwrap();

        // Verify unstaged (should be untracked now)
        let result = bash.exec("cd /repo && git status").await.unwrap();
        assert!(result.stdout.contains("Untracked files"));
    }
}

mod help_messages {
    use super::*;

    #[tokio::test]
    async fn test_git_help_shows_all_commands() {
        let mut bash = create_git_bash();

        let result = bash.exec("git").await.unwrap();
        // All Phase 3 commands should be in help
        assert!(result.stderr.contains("branch"));
        assert!(result.stderr.contains("checkout"));
        assert!(result.stderr.contains("diff"));
        assert!(result.stderr.contains("reset"));
    }
}
