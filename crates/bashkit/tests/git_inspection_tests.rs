//! Tests for git inspection/scripting commands
//!
//! Covers: show, ls-files, rev-parse, restore, merge-base, grep

#![cfg(feature = "git")]

use bashkit::{Bash, GitConfig};

fn create_git_bash() -> Bash {
    Bash::builder()
        .git(GitConfig::new().author("Test User", "test@example.com"))
        .build()
}

/// Helper: init repo, add file, commit
async fn setup_repo(bash: &mut Bash) {
    bash.exec(
        r#"
git init /repo
cd /repo
echo "hello world" > README.md
echo "fn main() {}" > main.rs
git add README.md main.rs
git commit -m "Initial commit"
"#,
    )
    .await
    .unwrap();
}

mod show {
    use super::*;

    #[tokio::test]
    async fn show_head_commit() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash.exec("cd /repo && git show").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Initial commit"));
        assert!(result.stdout.contains("Author:"));
    }

    #[tokio::test]
    async fn show_file_at_rev() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash
            .exec("cd /repo && git show HEAD:README.md")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello world"));
    }

    #[tokio::test]
    async fn show_nonexistent_file() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash
            .exec("cd /repo && git show HEAD:nofile.txt")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("does not exist"));
    }

    #[tokio::test]
    async fn show_no_commits() {
        let mut bash = create_git_bash();
        bash.exec("git init /repo && cd /repo").await.unwrap();
        let result = bash.exec("cd /repo && git show").await.unwrap();
        assert_ne!(result.exit_code, 0);
    }
}

mod ls_files {
    use super::*;

    #[tokio::test]
    async fn lists_tracked_files() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash.exec("cd /repo && git ls-files").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("README.md"));
        assert!(result.stdout.contains("main.rs"));
    }

    #[tokio::test]
    async fn empty_repo_no_files() {
        let mut bash = create_git_bash();
        bash.exec("git init /repo").await.unwrap();
        let result = bash.exec("cd /repo && git ls-files").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.trim().is_empty());
    }

    #[tokio::test]
    async fn includes_staged_files() {
        let mut bash = create_git_bash();
        bash.exec(
            r#"
git init /repo
cd /repo
echo "new" > new.txt
git add new.txt
"#,
        )
        .await
        .unwrap();
        let result = bash.exec("cd /repo && git ls-files").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("new.txt"));
    }
}

mod rev_parse {
    use super::*;

    #[tokio::test]
    async fn show_toplevel() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash
            .exec("cd /repo && git rev-parse --show-toplevel")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "/repo");
    }

    #[tokio::test]
    async fn git_dir() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash
            .exec("cd /repo && git rev-parse --git-dir")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "/repo/.git");
    }

    #[tokio::test]
    async fn is_inside_work_tree() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash
            .exec("cd /repo && git rev-parse --is-inside-work-tree")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "true");
    }

    #[tokio::test]
    async fn abbrev_ref_head() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash
            .exec("cd /repo && git rev-parse --abbrev-ref HEAD")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "master");
    }

    #[tokio::test]
    async fn head_resolves_to_hash() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash.exec("cd /repo && git rev-parse HEAD").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!result.stdout.trim().is_empty());
        // Hash should be hex
        assert!(result.stdout.trim().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn not_a_repo() {
        let mut bash = create_git_bash();
        let result = bash
            .exec("cd /tmp && git rev-parse --show-toplevel")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("not a git repository"));
    }
}

mod restore {
    use super::*;

    #[tokio::test]
    async fn restore_staged_unstages_file() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        bash.exec(
            r#"
cd /repo
echo "modified" > new.txt
git add new.txt
"#,
        )
        .await
        .unwrap();
        let result = bash
            .exec("cd /repo && git restore --staged new.txt && git status")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        // new.txt should no longer be staged
        assert!(!result.stdout.contains("new file:   new.txt"));
    }

    #[tokio::test]
    async fn restore_no_args() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash.exec("cd /repo && git restore").await.unwrap();
        assert_ne!(result.exit_code, 0);
    }
}

mod merge_base {
    use super::*;

    #[tokio::test]
    async fn merge_base_returns_hash() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash
            .exec("cd /repo && git merge-base HEAD master")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!result.stdout.trim().is_empty());
    }

    #[tokio::test]
    async fn merge_base_needs_two_args() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash.exec("cd /repo && git merge-base HEAD").await.unwrap();
        assert_ne!(result.exit_code, 0);
    }
}

mod grep {
    use super::*;

    #[tokio::test]
    async fn grep_finds_content() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash.exec("cd /repo && git grep hello").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("README.md"));
        assert!(result.stdout.contains("hello world"));
    }

    #[tokio::test]
    async fn grep_no_match_exits_1() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash
            .exec("cd /repo && git grep nonexistent_pattern")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn grep_specific_file() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash.exec("cd /repo && git grep fn main.rs").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("main.rs"));
        assert!(result.stdout.contains("fn main()"));
    }

    #[tokio::test]
    async fn grep_no_args() {
        let mut bash = create_git_bash();
        setup_repo(&mut bash).await;
        let result = bash.exec("cd /repo && git grep").await.unwrap();
        assert_ne!(result.exit_code, 0);
    }
}
