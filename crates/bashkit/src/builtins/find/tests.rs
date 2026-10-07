//! Tests for the find builtin.

use super::Find;
use crate::builtins::Ls;
use crate::builtins::limits::FIND_MAX_OUTPUT_BYTES;
use crate::builtins::{Builtin, Context};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::fs::{FileSystem, InMemoryFs};

async fn create_test_ctx() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
    let fs = Arc::new(InMemoryFs::new());
    let cwd = PathBuf::from("/home/user");
    let variables = HashMap::new();

    fs.mkdir(&cwd, true).await.unwrap();

    (fs, cwd, variables)
}

#[tokio::test]
async fn test_find_current_dir() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.write_file(&cwd.join("file.txt"), b"content")
        .await
        .unwrap();

    let args: Vec<String> = vec![];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("."));
    assert!(result.stdout.contains("file.txt"));
}

#[tokio::test]
async fn test_find_name_pattern() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.write_file(&cwd.join("file.txt"), b"content")
        .await
        .unwrap();
    fs.write_file(&cwd.join("other.md"), b"content")
        .await
        .unwrap();

    let args = vec!["-name".to_string(), "*.txt".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("file.txt"));
    assert!(!result.stdout.contains("other.md"));
}

#[tokio::test]
async fn test_find_type_file() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.write_file(&cwd.join("file.txt"), b"content")
        .await
        .unwrap();
    fs.mkdir(&cwd.join("subdir"), false).await.unwrap();

    let args = vec!["-type".to_string(), "f".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("file.txt"));
    assert!(!result.stdout.contains("subdir"));
}

#[tokio::test]
async fn test_find_type_directory() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.write_file(&cwd.join("file.txt"), b"content")
        .await
        .unwrap();
    fs.mkdir(&cwd.join("subdir"), false).await.unwrap();

    let args = vec!["-type".to_string(), "d".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(!result.stdout.contains("file.txt"));
    // Should contain the directory
    let lines: Vec<&str> = result.stdout.lines().collect();
    assert!(lines.iter().any(|l| l.contains("subdir") || *l == "."));
}

#[tokio::test]
async fn test_find_maxdepth() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.mkdir(&cwd.join("a"), false).await.unwrap();
    fs.mkdir(&cwd.join("a/b"), false).await.unwrap();
    fs.write_file(&cwd.join("a/b/deep.txt"), b"deep")
        .await
        .unwrap();

    let args = vec!["-maxdepth".to_string(), "1".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("./a"));
    assert!(!result.stdout.contains("deep.txt"));
}

#[tokio::test]
async fn test_find_nonexistent() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    let args = vec!["nonexistent".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("No such file or directory"));
}

#[tokio::test]
async fn test_find_missing_name_arg() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    let args = vec!["-name".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("missing argument"));
}

#[tokio::test]
async fn test_find_unknown_type() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    let args = vec!["-type".to_string(), "x".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("Unknown argument to -type"));
}

#[tokio::test]
async fn test_find_deep_recursion() {
    // Test that find without maxdepth descends into all subdirectory levels
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    // Create a deep directory structure: a/b/c/d/deep.txt
    fs.mkdir(&cwd.join("a"), false).await.unwrap();
    fs.mkdir(&cwd.join("a/b"), false).await.unwrap();
    fs.mkdir(&cwd.join("a/b/c"), false).await.unwrap();
    fs.mkdir(&cwd.join("a/b/c/d"), false).await.unwrap();
    fs.write_file(&cwd.join("a/b/c/d/deep.txt"), b"deep content")
        .await
        .unwrap();

    // Also add files at each level
    fs.write_file(&cwd.join("a/file1.txt"), b"level1")
        .await
        .unwrap();
    fs.write_file(&cwd.join("a/b/file2.txt"), b"level2")
        .await
        .unwrap();
    fs.write_file(&cwd.join("a/b/c/file3.txt"), b"level3")
        .await
        .unwrap();

    let args: Vec<String> = vec![];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);

    // Should find the root
    assert!(result.stdout.contains("."), "Should contain current dir");

    // Should find all directories at all levels
    assert!(result.stdout.contains("./a"), "Should contain ./a");
    assert!(result.stdout.contains("./a/b"), "Should contain ./a/b");
    assert!(result.stdout.contains("./a/b/c"), "Should contain ./a/b/c");
    assert!(
        result.stdout.contains("./a/b/c/d"),
        "Should contain ./a/b/c/d"
    );

    // Should find all files at all levels
    assert!(
        result.stdout.contains("file1.txt"),
        "Should contain file1.txt at level 1"
    );
    assert!(
        result.stdout.contains("file2.txt"),
        "Should contain file2.txt at level 2"
    );
    assert!(
        result.stdout.contains("file3.txt"),
        "Should contain file3.txt at level 3"
    );
    assert!(
        result.stdout.contains("deep.txt"),
        "Should contain deep.txt at level 4"
    );
}

#[tokio::test]
async fn test_find_very_deep_nesting() {
    // Test 10 levels of nesting to ensure no recursion limits
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    // Create 10 levels deep
    let mut path = cwd.clone();
    for i in 0..10 {
        path = path.join(format!("level{}", i));
        fs.mkdir(&path, false).await.unwrap();
        fs.write_file(
            &path.join(format!("file{}.txt", i)),
            format!("content{}", i).as_bytes(),
        )
        .await
        .unwrap();
    }

    let args: Vec<String> = vec![];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);

    // Verify all 10 levels are found
    for i in 0..10 {
        assert!(
            result.stdout.contains(&format!("level{}", i)),
            "Should find level{} directory",
            i
        );
        assert!(
            result.stdout.contains(&format!("file{}.txt", i)),
            "Should find file{}.txt",
            i
        );
    }
}

#[tokio::test]
async fn test_find_and_ls_consistency() {
    // Ensure find and ls -R find the same nested structure
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    // Create test structure
    fs.mkdir(&cwd.join("top"), false).await.unwrap();
    fs.mkdir(&cwd.join("top/middle"), false).await.unwrap();
    fs.mkdir(&cwd.join("top/middle/bottom"), false)
        .await
        .unwrap();
    fs.write_file(&cwd.join("top/a.txt"), b"a").await.unwrap();
    fs.write_file(&cwd.join("top/middle/b.txt"), b"b")
        .await
        .unwrap();
    fs.write_file(&cwd.join("top/middle/bottom/c.txt"), b"c")
        .await
        .unwrap();

    // Run find
    let args_find: Vec<String> = vec![];
    let ctx_find = Context {
        args: &args_find,
        env: &env,
        variables: &mut variables.clone(),
        cwd: &mut cwd.clone(),
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let find_result = Find::default().execute(ctx_find).await.unwrap();

    // Run ls -R
    let args_ls = vec!["-R".to_string()];
    let ctx_ls = Context {
        args: &args_ls,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let ls_result = Ls.execute(ctx_ls).await.unwrap();

    // Both should find all the nested content
    assert!(find_result.stdout.contains("top"));
    assert!(find_result.stdout.contains("middle"));
    assert!(find_result.stdout.contains("bottom"));
    assert!(find_result.stdout.contains("a.txt"));
    assert!(find_result.stdout.contains("b.txt"));
    assert!(find_result.stdout.contains("c.txt"));

    assert!(ls_result.stdout.contains("top"));
    assert!(ls_result.stdout.contains("middle"));
    assert!(ls_result.stdout.contains("bottom"));
    assert!(ls_result.stdout.contains("a.txt"));
    assert!(ls_result.stdout.contains("b.txt"));
    assert!(ls_result.stdout.contains("c.txt"));
}

#[tokio::test]
async fn test_find_with_empty_subdirs() {
    // Ensure empty subdirectories are still traversed
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    // Create structure with some empty dirs in the path
    fs.mkdir(&cwd.join("a"), false).await.unwrap();
    fs.mkdir(&cwd.join("a/empty1"), false).await.unwrap();
    fs.mkdir(&cwd.join("a/empty2"), false).await.unwrap();
    fs.mkdir(&cwd.join("a/empty1/deep"), false).await.unwrap();
    fs.write_file(&cwd.join("a/empty1/deep/file.txt"), b"found")
        .await
        .unwrap();

    let args: Vec<String> = vec![];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);

    // Should find the file through the empty directories
    assert!(result.stdout.contains("file.txt"));
    assert!(result.stdout.contains("empty1"));
    assert!(result.stdout.contains("empty2"));
    assert!(result.stdout.contains("deep"));
}

#[tokio::test]
async fn test_find_from_specific_path() {
    // Test finding from a specific starting path (not cwd)
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    // Create nested structure
    fs.mkdir(&cwd.join("start"), false).await.unwrap();
    fs.mkdir(&cwd.join("start/sub1"), false).await.unwrap();
    fs.mkdir(&cwd.join("start/sub1/sub2"), false).await.unwrap();
    fs.write_file(&cwd.join("start/sub1/sub2/target.txt"), b"target")
        .await
        .unwrap();

    // Find from a specific starting path
    let args = vec!["start".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);

    assert!(
        result.stdout.contains("start"),
        "Should contain starting path"
    );
    assert!(result.stdout.contains("sub1"), "Should descend into sub1");
    assert!(result.stdout.contains("sub2"), "Should descend into sub2");
    assert!(
        result.stdout.contains("target.txt"),
        "Should find target.txt"
    );
}

#[tokio::test]
async fn test_find_mindepth() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.mkdir(&cwd.join("a"), false).await.unwrap();
    fs.mkdir(&cwd.join("a/b"), false).await.unwrap();
    fs.write_file(&cwd.join("a/file1.txt"), b"f1")
        .await
        .unwrap();
    fs.write_file(&cwd.join("a/b/file2.txt"), b"f2")
        .await
        .unwrap();

    // mindepth 1 should exclude the starting directory "."
    let args = vec!["-mindepth".to_string(), "1".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    // Should NOT contain "." as the starting point (depth 0)
    let lines: Vec<&str> = result.stdout.lines().collect();
    assert!(!lines.contains(&"."), "mindepth 1 should exclude '.'");
    // Should contain everything at depth >= 1
    assert!(result.stdout.contains("./a"));
    assert!(result.stdout.contains("file1.txt"));
    assert!(result.stdout.contains("file2.txt"));
}

#[tokio::test]
async fn test_find_mindepth_with_type() {
    // Reproduces the reported issue: find . -mindepth 1 -type f | wc -l
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.mkdir(&cwd.join("a"), false).await.unwrap();
    fs.mkdir(&cwd.join("a/b"), false).await.unwrap();
    fs.write_file(&cwd.join("a/file1.txt"), b"f1")
        .await
        .unwrap();
    fs.write_file(&cwd.join("a/b/file2.txt"), b"f2")
        .await
        .unwrap();

    // mindepth 1 + type f
    let args = vec![
        "-mindepth".to_string(),
        "1".to_string(),
        "-type".to_string(),
        "f".to_string(),
    ];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    let lines: Vec<&str> = result.stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 2, "Should find 2 files: {:?}", lines); // debug-ok: assert-failure message

    // mindepth 1 + type d
    let args2 = vec![
        "-mindepth".to_string(),
        "1".to_string(),
        "-type".to_string(),
        "d".to_string(),
    ];
    let ctx2 = Context {
        args: &args2,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result2 = Find::default().execute(ctx2).await.unwrap();
    assert_eq!(result2.exit_code, 0);
    let lines2: Vec<&str> = result2.stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines2.len(), 2, "Should find 2 dirs: {:?}", lines2); // debug-ok: assert-failure message
}

#[tokio::test]
async fn test_find_mindepth_2() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.mkdir(&cwd.join("a"), false).await.unwrap();
    fs.mkdir(&cwd.join("a/b"), false).await.unwrap();
    fs.write_file(&cwd.join("top.txt"), b"top").await.unwrap();
    fs.write_file(&cwd.join("a/mid.txt"), b"mid").await.unwrap();
    fs.write_file(&cwd.join("a/b/deep.txt"), b"deep")
        .await
        .unwrap();

    // mindepth 2 should exclude depth 0 and depth 1
    let args = vec!["-mindepth".to_string(), "2".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    let lines: Vec<&str> = result.stdout.lines().collect();
    // depth 0: "." - excluded
    assert!(!lines.contains(&"."));
    // depth 1: "./a", "./top.txt" - excluded
    assert!(!lines.contains(&"./a"));
    assert!(!lines.contains(&"./top.txt"));
    // depth 2: "./a/b", "./a/mid.txt" - included
    assert!(lines.contains(&"./a/b"));
    assert!(lines.contains(&"./a/mid.txt"));
    // depth 3: "./a/b/deep.txt" - included
    assert!(lines.contains(&"./a/b/deep.txt"));
}

#[tokio::test]
async fn test_find_mindepth_missing_arg() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    let args = vec!["-mindepth".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("missing argument"));
}

#[tokio::test]
async fn test_find_mindepth_invalid_value() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    let args = vec!["-mindepth".to_string(), "abc".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(
        result
            .stderr
            .contains("Expected a positive decimal integer argument to -mindepth")
    );
}

#[tokio::test]
async fn test_find_with_overlay_fs() {
    use crate::fs::OverlayFs;

    // Create base filesystem with nested structure
    let base = Arc::new(InMemoryFs::new());
    base.mkdir(Path::new("/home/user"), true).await.unwrap();
    base.mkdir(Path::new("/home/user/base"), false)
        .await
        .unwrap();
    base.mkdir(Path::new("/home/user/base/sub1"), false)
        .await
        .unwrap();
    base.mkdir(Path::new("/home/user/base/sub1/sub2"), false)
        .await
        .unwrap();
    base.write_file(Path::new("/home/user/base/file1.txt"), b"base1")
        .await
        .unwrap();
    base.write_file(Path::new("/home/user/base/sub1/file2.txt"), b"base2")
        .await
        .unwrap();
    base.write_file(Path::new("/home/user/base/sub1/sub2/file3.txt"), b"base3")
        .await
        .unwrap();

    // Create overlay
    let overlay: Arc<dyn FileSystem> = Arc::new(OverlayFs::new(base));

    // Add a file in the overlay layer (use recursive to ensure parent exists in upper)
    overlay
        .mkdir(Path::new("/home/user/base/overlay_dir"), true)
        .await
        .unwrap();
    overlay
        .write_file(
            Path::new("/home/user/base/overlay_dir/overlay_file.txt"),
            b"overlay",
        )
        .await
        .unwrap();

    let mut cwd = PathBuf::from("/home/user");
    let mut variables = HashMap::new();
    let env = HashMap::new();

    // Run find on the overlay filesystem
    let args = vec!["base".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: overlay.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);

    // Should find files from base layer
    assert!(
        result.stdout.contains("file1.txt"),
        "Should find file1.txt from base"
    );
    assert!(
        result.stdout.contains("file2.txt"),
        "Should find file2.txt from base/sub1"
    );
    assert!(
        result.stdout.contains("file3.txt"),
        "Should find file3.txt from base/sub1/sub2"
    );

    // Should find files from overlay layer
    assert!(
        result.stdout.contains("overlay_dir"),
        "Should find overlay_dir"
    );
    assert!(
        result.stdout.contains("overlay_file.txt"),
        "Should find overlay_file.txt"
    );

    // Should descend into all subdirectories
    assert!(result.stdout.contains("sub1"), "Should find sub1");
    assert!(result.stdout.contains("sub2"), "Should find sub2");
}

#[tokio::test]
async fn test_find_with_mountable_fs() {
    use crate::fs::MountableFs;

    // Create root filesystem
    let root = Arc::new(InMemoryFs::new());
    root.mkdir(Path::new("/home/user"), true).await.unwrap();
    root.write_file(Path::new("/home/user/root_file.txt"), b"root")
        .await
        .unwrap();

    // Create a nested filesystem to mount
    let nested = Arc::new(InMemoryFs::new());
    nested.mkdir(Path::new("/level1"), false).await.unwrap();
    nested
        .mkdir(Path::new("/level1/level2"), false)
        .await
        .unwrap();
    nested
        .mkdir(Path::new("/level1/level2/level3"), false)
        .await
        .unwrap();
    nested
        .write_file(Path::new("/level1/nested1.txt"), b"n1")
        .await
        .unwrap();
    nested
        .write_file(Path::new("/level1/level2/nested2.txt"), b"n2")
        .await
        .unwrap();
    nested
        .write_file(Path::new("/level1/level2/level3/nested3.txt"), b"n3")
        .await
        .unwrap();

    // Create mountable filesystem and mount nested at /home/user/mounted
    let mountable = MountableFs::new(root.clone());
    mountable
        .mount("/home/user/mounted", nested.clone())
        .unwrap();

    let fs: Arc<dyn FileSystem> = Arc::new(mountable);
    let mut cwd = PathBuf::from("/home/user");
    let mut variables = HashMap::new();
    let env = HashMap::new();

    // Run find from cwd - should find both root files and mounted files
    let args: Vec<String> = vec![];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);

    // Should find root file
    assert!(
        result.stdout.contains("root_file.txt"),
        "Should find root_file.txt"
    );

    // Should find mount point
    assert!(
        result.stdout.contains("mounted"),
        "Should find mounted directory"
    );

    // Should descend into mounted filesystem
    assert!(
        result.stdout.contains("level1"),
        "Should find level1 in mounted fs"
    );
    assert!(
        result.stdout.contains("level2"),
        "Should find level2 in mounted fs"
    );
    assert!(
        result.stdout.contains("level3"),
        "Should find level3 in mounted fs"
    );

    // Should find files deep in mounted filesystem
    assert!(
        result.stdout.contains("nested1.txt"),
        "Should find nested1.txt"
    );
    assert!(
        result.stdout.contains("nested2.txt"),
        "Should find nested2.txt"
    );
    assert!(
        result.stdout.contains("nested3.txt"),
        "Should find nested3.txt"
    );
}

#[tokio::test]
async fn test_find_printf_rejects_oversized_single_entry_output() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.write_file(&cwd.join("a.txt"), b"x").await.unwrap();

    // A -printf format that produces more than FIND_MAX_OUTPUT_BYTES for a single entry.
    let fmt = "x".repeat(FIND_MAX_OUTPUT_BYTES + 1);
    let args = vec!["-printf".to_string(), fmt];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(
        result.exit_code, 1,
        "should fail when printf output exceeds cap"
    );
    assert!(
        result.stderr.contains("find: output size limit exceeded"),
        "stderr should report the limit with prefix: {}",
        result.stderr
    );
    assert!(
        result.stdout.len() <= FIND_MAX_OUTPUT_BYTES,
        "stdout must not exceed cap: len={}",
        result.stdout.len()
    );
}

#[tokio::test]
async fn test_find_printf_rejects_oversized_aggregate_output() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.write_file(&cwd.join("a.txt"), b"x").await.unwrap();
    fs.write_file(&cwd.join("b.txt"), b"x").await.unwrap();

    // Each entry produces just over half the cap; two entries together exceed it.
    let half = FIND_MAX_OUTPUT_BYTES / 2 + 1;
    let fmt = "y".repeat(half);
    let args = vec!["-printf".to_string(), fmt];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: fs.clone(),
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Find::default().execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 1, "aggregate output should hit cap");
    assert!(
        result.stderr.contains("find: output size limit exceeded"),
        "stderr should report the limit with prefix: {}",
        result.stderr
    );
    assert!(
        result.stdout.len() <= FIND_MAX_OUTPUT_BYTES,
        "stdout must not exceed cap: len={}",
        result.stdout.len()
    );
}
