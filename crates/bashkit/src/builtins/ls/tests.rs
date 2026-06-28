//! Tests for ls, find, rmdir, and glob_match.

use super::find::{Find, build_find_exec_commands, parse_find_args};
use super::glob_match;
use super::list::Ls;
use super::rmdir::Rmdir;
use crate::builtins::limits::FIND_MAX_OUTPUT_BYTES;
use crate::builtins::{Builtin, Context, ExecutionPlan};

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

// ==================== ls tests ====================

#[tokio::test]
async fn test_ls_empty_dir() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "");
}

#[tokio::test]
async fn test_ls_with_files() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    // Create some files
    fs.write_file(&cwd.join("file1.txt"), b"content1")
        .await
        .unwrap();
    fs.write_file(&cwd.join("file2.txt"), b"content2")
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("file1.txt"));
    assert!(result.stdout.contains("file2.txt"));
}

#[tokio::test]
async fn test_ls_hidden_files() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.write_file(&cwd.join(".hidden"), b"hidden")
        .await
        .unwrap();
    fs.write_file(&cwd.join("visible"), b"visible")
        .await
        .unwrap();

    // Without -a
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(!result.stdout.contains(".hidden"));
    assert!(result.stdout.contains("visible"));

    // With -a
    let args = vec!["-a".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains(".hidden"));
    assert!(result.stdout.contains("visible"));
}

#[tokio::test]
async fn test_ls_long_format() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.write_file(&cwd.join("test.txt"), b"content")
        .await
        .unwrap();

    let args = vec!["-l".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    // Long format should include permissions
    assert!(result.stdout.contains("rw"));
    assert!(result.stdout.contains("test.txt"));
}

#[tokio::test]
async fn test_ls_nonexistent() {
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(result.stderr.contains("No such file or directory"));
}

#[tokio::test]
async fn test_ls_invalid_option() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    let args = vec!["-z".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 2);
    // clap-rendered diagnostic differs from GNU coreutils' wording
    // ("invalid option") but both flag the unknown short flag.
    // `### bash_diff` documents the GNU divergence in spec tests.
    let combined = format!("{}{}", result.stdout, result.stderr);
    assert!(
        combined.contains("unexpected argument") || combined.contains("invalid option"),
        "expected an unknown-flag diagnostic, got: {}",
        combined
    );
}

#[tokio::test]
async fn test_ls_sort_by_time() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    // Create files with different modification times
    fs.write_file(&cwd.join("older.txt"), b"older")
        .await
        .unwrap();
    fs.write_file(&cwd.join("newer.txt"), b"newer")
        .await
        .unwrap();

    let args = vec!["-t".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    // -t should be accepted (not cause an error)
    assert!(result.stdout.contains("older.txt"));
    assert!(result.stdout.contains("newer.txt"));
}

#[tokio::test]
async fn test_ls_file() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.write_file(&cwd.join("test.txt"), b"content")
        .await
        .unwrap();

    let args = vec!["test.txt".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("test.txt"));
}

#[tokio::test]
async fn test_ls_recursive() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.mkdir(&cwd.join("subdir"), false).await.unwrap();
    fs.write_file(&cwd.join("file.txt"), b"content")
        .await
        .unwrap();
    fs.write_file(&cwd.join("subdir/nested.txt"), b"nested")
        .await
        .unwrap();

    let args = vec!["-R".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("file.txt"));
    assert!(result.stdout.contains("subdir"));
    assert!(result.stdout.contains("nested.txt"));
}

/// TM-INF-024 regression: a host-set `TIME_STYLE` (or any other env var
/// uutils' `uu_app()` attaches to an Arg via `.env(...)`) MUST NOT
/// reach the clap parser. uutils' upstream wires
/// `Arg::new(TIME_STYLE).env("TIME_STYLE")` so the option defaults
/// from the host process — bashkit strips that at codegen time, drops
/// the `env` clap feature workspace-wide as defence-in-depth, and
/// re-implements env-default precedence against `ctx.env` only via
/// `apply_env_defaults`. Without those guards a plain `ls` in a
/// container that exports `TIME_STYLE=long-iso` would trip the
/// unsupported-option gate (since bashkit hasn't implemented
/// `--time-style` yet) and break `ls` for every script running on
/// that host.
#[tokio::test]
#[serial_test::serial]
async fn ls_ignores_host_time_style_and_tabsize() {
    // SAFETY: serial_test::serial serializes against other tests that
    // touch the process env. Setting + unsetting around a single
    // bashkit `Ls.execute()` is the only way to exercise the
    // sandbox-leak regression: we're asserting bashkit does NOT
    // observe these even when they're present on the host process.
    unsafe {
        std::env::set_var("TIME_STYLE", "long-iso");
        std::env::set_var("TABSIZE", "16");
    }

    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    fs.write_file(&cwd.join("a.txt"), b"a").await.unwrap();
    let env = HashMap::new();
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

    let result = Ls.execute(ctx).await.unwrap();

    unsafe {
        std::env::remove_var("TIME_STYLE");
        std::env::remove_var("TABSIZE");
    }

    assert_eq!(
        result.exit_code, 0,
        "host TIME_STYLE/TABSIZE leaked into bashkit ls parser \
         (TM-INF-024): stderr={}",
        result.stderr
    );
    assert!(result.stdout.contains("a.txt"));
    assert!(
        !result.stderr.contains("not yet implemented"),
        "host env tunneled through clap as a value source: stderr={}",
        result.stderr
    );
}

/// Counterpart to `ls_ignores_host_time_style_and_tabsize`: when
/// `TIME_STYLE` lives in bashkit's *virtual* env (`ctx.env`), the
/// codegen-emitted `LS_ENV_DEFAULTS` table feeds it through
/// `apply_env_defaults` and clap honours it just as upstream
/// uutils would honour `std::env::var("TIME_STYLE")`. We assert
/// that path by observing the clap value-source: ls today rejects
/// `--time-style` with a "not yet implemented" message, but only
/// because clap saw the option at all. If the env→argv shim were
/// silently broken, ls would succeed.
#[tokio::test]
async fn ls_honors_virtual_env_time_style() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    fs.write_file(&cwd.join("a.txt"), b"a").await.unwrap();
    let mut env = HashMap::new();
    env.insert("TIME_STYLE".to_string(), "long-iso".to_string());
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

    let result = Ls.execute(ctx).await.unwrap();

    assert_eq!(
        result.exit_code, 2,
        "virtual TIME_STYLE should reach clap's parser; got stdout={} \
         stderr={}",
        result.stdout, result.stderr
    );
    assert!(
        result.stderr.contains("not yet implemented") && result.stderr.contains("time-style"),
        "expected unsupported-option error mentioning time-style; got stderr={}",
        result.stderr
    );
}

/// Argv-set `--time-style` must take precedence over a `ctx.env`
/// value, matching clap's documented "argv > env > default"
/// precedence and `apply_env_defaults`'s contract. Easiest way to
/// observe it: pass an explicit `--time-style=iso`, set a
/// *different* value in `ctx.env`, and assert the error message
/// surfaces the explicit one (or at least that env did not
/// double-inject — the unsupported-option path collapses both
/// into the same diagnostic, so we just confirm exit==2 and that
/// argv wasn't mangled).
#[tokio::test]
async fn ls_argv_time_style_overrides_virtual_env() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let mut env = HashMap::new();
    env.insert("TIME_STYLE".to_string(), "long-iso".to_string());
    let args: Vec<String> = vec!["--time-style=iso".to_string()];
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
    let result = Ls.execute(ctx).await.unwrap();
    // Same unsupported-option gate as the virtual-env-only test;
    // the precedence-correctness signal is that we don't see
    // *two* sources or a clap parse failure complaining about
    // duplicate `--time-style`.
    assert_eq!(result.exit_code, 2);
    assert!(
        !result.stderr.contains("supplied more than once"),
        "shim double-injected --time-style: stderr={}",
        result.stderr
    );
}

// ==================== find tests ====================

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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("unknown type"));
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

    let result = Find.execute(ctx).await.unwrap();
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
async fn test_ls_recursive_deep() {
    // Test that ls -R descends into all subdirectory levels
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    // Create a deep directory structure: a/b/c/deep.txt
    fs.mkdir(&cwd.join("a"), false).await.unwrap();
    fs.mkdir(&cwd.join("a/b"), false).await.unwrap();
    fs.mkdir(&cwd.join("a/b/c"), false).await.unwrap();
    fs.write_file(&cwd.join("a/b/c/deep.txt"), b"deep content")
        .await
        .unwrap();

    let args = vec!["-R".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);

    // Should list all directories and their contents
    assert!(result.stdout.contains("a"), "Should list dir a");
    assert!(result.stdout.contains("b"), "Should list dir b under a");
    assert!(result.stdout.contains("c"), "Should list dir c under a/b");
    assert!(
        result.stdout.contains("deep.txt"),
        "Should list deep.txt under a/b/c"
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

    let result = Find.execute(ctx).await.unwrap();
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

    let find_result = Find.execute(ctx_find).await.unwrap();

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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
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

    let result2 = Find.execute(ctx2).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("invalid mindepth"));
}

// ==================== rmdir tests ====================

#[tokio::test]
async fn test_rmdir_empty() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.mkdir(&cwd.join("emptydir"), false).await.unwrap();

    let args = vec!["emptydir".to_string()];
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

    let result = Rmdir.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(!fs.exists(&cwd.join("emptydir")).await.unwrap());
}

#[tokio::test]
async fn test_rmdir_not_empty() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.mkdir(&cwd.join("notempty"), false).await.unwrap();
    fs.write_file(&cwd.join("notempty/file.txt"), b"content")
        .await
        .unwrap();

    let args = vec!["notempty".to_string()];
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

    let result = Rmdir.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("not empty"));
}

#[tokio::test]
async fn test_rmdir_nonexistent() {
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

    let result = Rmdir.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("No such file or directory"));
}

#[tokio::test]
async fn test_rmdir_not_directory() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.write_file(&cwd.join("file.txt"), b"content")
        .await
        .unwrap();

    let args = vec!["file.txt".to_string()];
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

    let result = Rmdir.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("Not a directory"));
}

#[tokio::test]
async fn test_rmdir_parents() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.mkdir(&cwd.join("a/b/c"), true).await.unwrap();

    let args = vec!["-p".to_string(), "a/b/c".to_string()];
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

    let result = Rmdir.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(!fs.exists(&cwd.join("a/b/c")).await.unwrap());
    assert!(!fs.exists(&cwd.join("a/b")).await.unwrap());
    assert!(!fs.exists(&cwd.join("a")).await.unwrap());
}

#[tokio::test]
async fn test_rmdir_missing_operand() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

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

    let result = Rmdir.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("missing operand"));
}

// ==================== Custom filesystem tests ====================

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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
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
async fn test_ls_recursive_with_overlay_fs() {
    use crate::fs::OverlayFs;

    // Create base filesystem with nested structure
    let base = Arc::new(InMemoryFs::new());
    base.mkdir(Path::new("/home/user"), true).await.unwrap();
    base.mkdir(Path::new("/home/user/dir"), false)
        .await
        .unwrap();
    base.mkdir(Path::new("/home/user/dir/subdir"), false)
        .await
        .unwrap();
    base.write_file(Path::new("/home/user/dir/base.txt"), b"base")
        .await
        .unwrap();
    base.write_file(Path::new("/home/user/dir/subdir/deep.txt"), b"deep")
        .await
        .unwrap();

    let overlay: Arc<dyn FileSystem> = Arc::new(OverlayFs::new(base));

    let mut cwd = PathBuf::from("/home/user");
    let mut variables = HashMap::new();
    let env = HashMap::new();

    let args = vec!["-R".to_string(), "dir".to_string()];
    let ctx = Context {
        args: &args,
        env: &env,
        variables: &mut variables,
        cwd: &mut cwd,
        fs: overlay,
        stdin: None,
        #[cfg(feature = "http_client")]
        http_client: None,
        #[cfg(feature = "git")]
        git_client: None,
        #[cfg(feature = "ssh")]
        ssh_client: None,
        shell: None,
    };

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);

    assert!(result.stdout.contains("base.txt"), "Should list base.txt");
    assert!(result.stdout.contains("subdir"), "Should list subdir");
    assert!(result.stdout.contains("deep.txt"), "Should list deep.txt");
}

#[tokio::test]
async fn test_ls_recursive_with_mountable_fs() {
    use crate::fs::MountableFs;

    let root = Arc::new(InMemoryFs::new());
    root.mkdir(Path::new("/home/user"), true).await.unwrap();

    let mounted = Arc::new(InMemoryFs::new());
    mounted.mkdir(Path::new("/a"), false).await.unwrap();
    mounted.mkdir(Path::new("/a/b"), false).await.unwrap();
    mounted
        .write_file(Path::new("/a/file_a.txt"), b"a")
        .await
        .unwrap();
    mounted
        .write_file(Path::new("/a/b/file_b.txt"), b"b")
        .await
        .unwrap();

    let mountable = MountableFs::new(root);
    mountable.mount("/home/user/mnt", mounted.clone()).unwrap();

    let fs: Arc<dyn FileSystem> = Arc::new(mountable);
    let mut cwd = PathBuf::from("/home/user");
    let mut variables = HashMap::new();
    let env = HashMap::new();

    let args = vec!["-R".to_string(), "mnt".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);

    assert!(result.stdout.contains("a"), "Should list directory a");
    assert!(result.stdout.contains("b"), "Should list directory b");
    assert!(
        result.stdout.contains("file_a.txt"),
        "Should list file_a.txt"
    );
    assert!(
        result.stdout.contains("file_b.txt"),
        "Should list file_b.txt"
    );
}

// ==================== root directory tests ====================

#[tokio::test]
async fn test_ls_root_directory() {
    // Test listing the root directory directly
    let fs = Arc::new(InMemoryFs::new());
    let mut cwd = PathBuf::from("/home/user");
    let mut variables = HashMap::new();
    let env = HashMap::new();

    // ls / should work
    let args = vec!["/".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "ls / should succeed: {}",
        result.stderr
    );
    // Root should contain at least tmp, home, dev
    assert!(result.stdout.contains("tmp"), "Root should contain tmp");
    assert!(result.stdout.contains("home"), "Root should contain home");
    assert!(result.stdout.contains("dev"), "Root should contain dev");
}

#[tokio::test]
async fn test_ls_dot_from_root() {
    // Test: when cwd is /, ls . should list root contents
    let fs = Arc::new(InMemoryFs::new());
    let mut cwd = PathBuf::from("/");
    let mut variables = HashMap::new();
    let env = HashMap::new();

    // ls . with cwd=/ should work
    let args = vec![".".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "ls . from / should succeed: {}",
        result.stderr
    );
    assert!(result.stdout.contains("tmp"), "Root should contain tmp");
    assert!(result.stdout.contains("home"), "Root should contain home");
}

#[tokio::test]
async fn test_ls_default_from_root() {
    // Test: when cwd is /, ls (no args) should list root contents
    let fs = Arc::new(InMemoryFs::new());
    let mut cwd = PathBuf::from("/");
    let mut variables = HashMap::new();
    let env = HashMap::new();

    // ls with no args and cwd=/ should work
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "ls from / should succeed: {}",
        result.stderr
    );
    assert!(result.stdout.contains("tmp"), "Root should contain tmp");
    assert!(result.stdout.contains("home"), "Root should contain home");
}

#[tokio::test]
async fn test_ls_root_with_overlay_fs() {
    // Test: ls / with OverlayFs should work
    use crate::fs::OverlayFs;

    let base = Arc::new(InMemoryFs::new());
    let overlay: Arc<dyn FileSystem> = Arc::new(OverlayFs::new(base));

    let mut cwd = PathBuf::from("/");
    let mut variables = HashMap::new();
    let env = HashMap::new();

    // ls / should work with overlay
    let args = vec!["/".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "ls / with overlay should succeed: {}",
        result.stderr
    );
    assert!(result.stdout.contains("tmp"), "Root should contain tmp");
    assert!(result.stdout.contains("home"), "Root should contain home");
}

#[tokio::test]
async fn test_ls_dot_from_root_with_overlay_fs() {
    // Test: cd / && ls . with OverlayFs should work
    use crate::fs::OverlayFs;

    let base = Arc::new(InMemoryFs::new());
    let overlay: Arc<dyn FileSystem> = Arc::new(OverlayFs::new(base));

    let mut cwd = PathBuf::from("/");
    let mut variables = HashMap::new();
    let env = HashMap::new();

    // ls . from / with overlay
    let args = vec![".".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "ls . from / with overlay should succeed: {}",
        result.stderr
    );
    assert!(result.stdout.contains("tmp"), "Root should contain tmp");
    assert!(result.stdout.contains("home"), "Root should contain home");
}

#[tokio::test]
async fn test_resolve_path_slash_dot_normalized() {
    // Verify that "/." path (from /join(".")) resolves correctly to root
    let fs = Arc::new(InMemoryFs::new());

    // The path "/." should normalize to "/" and exist
    assert!(fs.exists(Path::new("/.")).await.unwrap(), "/. should exist");
    assert!(fs.exists(Path::new("/")).await.unwrap(), "/ should exist");

    // Both should return the same stat info
    let stat_root = fs.stat(Path::new("/")).await.unwrap();
    let stat_dot = fs.stat(Path::new("/.")).await.unwrap();
    assert!(stat_root.file_type.is_dir());
    assert!(stat_dot.file_type.is_dir());
}

// ==================== negative tests ====================

#[tokio::test]
async fn test_ls_nonexistent_path() {
    // Negative test: ls on path that doesn't exist should fail
    let fs = Arc::new(InMemoryFs::new());
    let mut cwd = PathBuf::from("/home/user");
    let mut variables = HashMap::new();
    let env = HashMap::new();

    let args = vec!["/nonexistent/path".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 2, "ls on nonexistent path should fail");
    assert!(
        result.stderr.contains("No such file or directory"),
        "Should report file not found"
    );
}

#[tokio::test]
async fn test_ls_path_traversal_normalized() {
    // Positive test: path traversal with .. should be normalized and work
    let fs = Arc::new(InMemoryFs::new());
    let mut cwd = PathBuf::from("/home/user");
    let mut variables = HashMap::new();
    let env = HashMap::new();

    // /home/user/../user should normalize to /home/user
    let args = vec!["../user".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    // /home/user is an empty directory by default, so it should succeed with empty output
    assert_eq!(
        result.exit_code, 0,
        "ls with .. should succeed after normalization: {}",
        result.stderr
    );
}

#[tokio::test]
async fn test_ls_excessive_dotdot_stays_at_root() {
    // Positive test: excessive .. should stay at root
    let fs = Arc::new(InMemoryFs::new());
    let mut cwd = PathBuf::from("/home/user");
    let mut variables = HashMap::new();
    let env = HashMap::new();

    // ../../../../.. from /home/user should normalize to /
    let args = vec!["../../../../..".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "Excessive .. should normalize to root: {}",
        result.stderr
    );
    // Should list root contents
    assert!(result.stdout.contains("tmp"), "Should list root (tmp)");
    assert!(result.stdout.contains("home"), "Should list root (home)");
}

#[tokio::test]
async fn test_ls_dot_in_middle_of_path() {
    // Positive test: . in middle of path should be normalized
    let fs = Arc::new(InMemoryFs::new());
    let mut cwd = PathBuf::from("/");
    let mut variables = HashMap::new();
    let env = HashMap::new();

    // /./home/./user/. should normalize to /home/user
    let args = vec!["./home/./user/.".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(
        result.exit_code, 0,
        "Path with . components should work: {}",
        result.stderr
    );
}

// ==================== glob_match tests ====================

// ==================== file size reporting tests ====================

#[tokio::test]
async fn test_ls_long_format_shows_correct_file_size() {
    // Positive test: file with known content shows correct size
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    // Create file with exactly 13 bytes: "hello world\n" (11 chars + newline from echo)
    let content = b"hello world\n";
    fs.write_file(&cwd.join("test.txt"), content).await.unwrap();

    let args = vec!["-l".to_string(), "test.txt".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    // File size should be 12 bytes (content.len())
    assert!(
        result.stdout.contains("12"),
        "Expected size 12 in output, got: {}",
        result.stdout
    );
}

#[tokio::test]
async fn test_ls_long_format_empty_file_shows_zero_size() {
    // Negative test: empty file shows size 0
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.write_file(&cwd.join("empty.txt"), b"").await.unwrap();

    let args = vec!["-l".to_string(), "empty.txt".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    // Empty file should show size 0
    // Format is: -rw-r--r--        0 YYYY-MM-DD HH:MM empty.txt
    assert!(
        result.stdout.contains("       0"),
        "Expected size 0 in output, got: {}",
        result.stdout
    );
}

#[tokio::test]
async fn test_ls_long_format_directory_shows_zero_size() {
    // Negative test: directory shows size 0
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.mkdir(&cwd.join("subdir"), false).await.unwrap();

    let args = vec!["-l".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);
    // Directory line should contain size 0
    // Format is: drwxr-xr-x        0 YYYY-MM-DD HH:MM subdir
    let lines: Vec<&str> = result.stdout.lines().collect();
    let subdir_line = lines.iter().find(|l| l.contains("subdir")).unwrap();
    assert!(
        subdir_line.contains("       0"),
        "Expected directory size 0, got: {}",
        subdir_line
    );
}

#[tokio::test]
async fn test_ls_long_format_multiple_files_correct_sizes() {
    // Positive test: multiple files show their respective sizes
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    // Create files with different sizes
    fs.write_file(&cwd.join("small.txt"), b"hi").await.unwrap(); // 2 bytes
    fs.write_file(&cwd.join("medium.txt"), b"hello world")
        .await
        .unwrap(); // 11 bytes
    fs.write_file(
        &cwd.join("large.txt"),
        b"this is a longer content string for testing",
    )
    .await
    .unwrap(); // 43 bytes

    let args = vec!["-l".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0);

    let lines: Vec<&str> = result.stdout.lines().collect();

    // Check small.txt has size 2
    let small_line = lines.iter().find(|l| l.contains("small.txt")).unwrap();
    assert!(
        small_line.contains("       2"),
        "Expected small.txt size 2, got: {}",
        small_line
    );

    // Check medium.txt has size 11
    let medium_line = lines.iter().find(|l| l.contains("medium.txt")).unwrap();
    assert!(
        medium_line.contains("      11"),
        "Expected medium.txt size 11, got: {}",
        medium_line
    );

    // Check large.txt has size 43
    let large_line = lines.iter().find(|l| l.contains("large.txt")).unwrap();
    assert!(
        large_line.contains("      43"),
        "Expected large.txt size 43, got: {}",
        large_line
    );
}

#[test]
fn test_glob_match_star() {
    assert!(glob_match("file.txt", "*.txt"));
    assert!(glob_match("test.txt", "*.txt"));
    assert!(!glob_match("file.md", "*.txt"));
}

#[test]
fn test_glob_match_question() {
    assert!(glob_match("ab", "a?"));
    assert!(glob_match("ac", "a?"));
    assert!(!glob_match("abc", "a?"));
}

#[test]
fn test_glob_match_exact() {
    assert!(glob_match("test", "test"));
    assert!(!glob_match("test", "other"));
}

#[test]
fn test_glob_match_star_middle() {
    assert!(glob_match("test.backup.txt", "test*.txt"));
    assert!(glob_match("test.txt", "test*.txt"));
}

// ==================== parse_find_args tests ====================

#[test]
fn test_parse_find_args_defaults_to_dot() {
    let args: Vec<String> = vec![];
    let (paths, opts) = parse_find_args(&args).unwrap();
    assert_eq!(paths, vec!["."]);
    assert!(opts.exec_args.is_empty());
    assert!(!opts.exec_batch);
}

#[test]
fn test_parse_find_args_exec_per_file() {
    let args: Vec<String> = vec![
        ".".into(),
        "-name".into(),
        "*.txt".into(),
        "-exec".into(),
        "cat".into(),
        "{}".into(),
        ";".into(),
    ];
    let (paths, opts) = parse_find_args(&args).unwrap();
    assert_eq!(paths, vec!["."]);
    assert_eq!(opts.name_pattern.as_deref(), Some("*.txt"));
    assert_eq!(opts.exec_args, vec!["cat", "{}"]);
    assert!(!opts.exec_batch);
}

#[test]
fn test_parse_find_args_exec_batch() {
    let args: Vec<String> = vec!["-exec".into(), "rm".into(), "{}".into(), "+".into()];
    let (_paths, opts) = parse_find_args(&args).unwrap();
    assert_eq!(opts.exec_args, vec!["rm", "{}"]);
    assert!(opts.exec_batch);
}

#[test]
fn test_parse_find_args_error_missing_name() {
    let args: Vec<String> = vec!["-name".into()];
    assert!(parse_find_args(&args).is_err());
}

#[test]
fn test_parse_find_args_error_unknown_predicate() {
    let args: Vec<String> = vec!["-bogus".into()];
    assert!(parse_find_args(&args).is_err());
}

// ==================== build_find_exec_commands tests ====================

#[test]
fn test_build_find_exec_commands_per_file() {
    let exec_args = vec!["echo".to_string(), "{}".to_string()];
    let paths = vec!["a.txt".to_string(), "b.txt".to_string()];
    let cmds = build_find_exec_commands(&exec_args, &paths, false);
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].name, "echo");
    assert_eq!(cmds[0].args, vec!["a.txt"]);
    assert_eq!(cmds[1].name, "echo");
    assert_eq!(cmds[1].args, vec!["b.txt"]);
}

#[test]
fn test_build_find_exec_commands_batch() {
    let exec_args = vec!["rm".to_string(), "{}".to_string()];
    let paths = vec!["a.txt".to_string(), "b.txt".to_string()];
    let cmds = build_find_exec_commands(&exec_args, &paths, true);
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].name, "rm");
    assert_eq!(cmds[0].args, vec!["a.txt", "b.txt"]);
}

#[test]
fn test_build_find_exec_commands_empty_paths() {
    let exec_args = vec!["echo".to_string(), "{}".to_string()];
    let cmds = build_find_exec_commands(&exec_args, &[], false);
    assert!(cmds.is_empty());
}

#[test]
fn test_build_find_exec_commands_empty_exec() {
    let paths = vec!["a.txt".to_string()];
    let cmds = build_find_exec_commands(&[], &paths, false);
    assert!(cmds.is_empty());
}

#[test]
fn test_build_find_exec_commands_multiple_placeholders() {
    let exec_args = vec!["cp".to_string(), "{}".to_string(), "{}.bak".to_string()];
    let paths = vec!["a.txt".to_string()];
    let cmds = build_find_exec_commands(&exec_args, &paths, false);
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].name, "cp");
    assert_eq!(cmds[0].args, vec!["a.txt", "a.txt.bak"]);
}

// ==================== find execution_plan tests ====================

#[tokio::test]
async fn test_find_plan_no_exec_returns_none() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

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

    let plan = Find.execution_plan(&ctx).await.unwrap();
    assert!(plan.is_none());
}

#[tokio::test]
async fn test_find_plan_exec_with_matches() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    // Create test files
    fs.write_file(&PathBuf::from("/home/user/a.txt"), b"hello")
        .await
        .unwrap();
    fs.write_file(&PathBuf::from("/home/user/b.txt"), b"world")
        .await
        .unwrap();

    let args = vec![
        ".".to_string(),
        "-name".to_string(),
        "*.txt".to_string(),
        "-exec".to_string(),
        "cat".to_string(),
        "{}".to_string(),
        ";".to_string(),
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

    let plan = Find.execution_plan(&ctx).await.unwrap();
    match plan {
        Some(ExecutionPlan::Batch { commands }) => {
            assert_eq!(commands.len(), 2);
            assert_eq!(commands[0].name, "cat");
            // Each command should have a single arg (the found path)
            assert_eq!(commands[0].args.len(), 1);
            assert_eq!(commands[1].args.len(), 1);
        }
        _ => panic!("expected Batch plan"),
    }
}

#[tokio::test]
async fn test_find_plan_exec_no_matches_returns_none() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    let args = vec![
        ".".to_string(),
        "-name".to_string(),
        "*.xyz".to_string(),
        "-exec".to_string(),
        "echo".to_string(),
        "{}".to_string(),
        ";".to_string(),
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

    let plan = Find.execution_plan(&ctx).await.unwrap();
    assert!(plan.is_none());
}

#[tokio::test]
async fn test_find_plan_exec_with_missing_path_returns_status_plan() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.write_file(&PathBuf::from("/home/user/a.txt"), b"hello")
        .await
        .unwrap();

    let args = vec![
        "/home/user".to_string(),
        "/home/missing".to_string(),
        "-name".to_string(),
        "*.txt".to_string(),
        "-exec".to_string(),
        "echo".to_string(),
        "{}".to_string(),
        ";".to_string(),
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

    let plan = Find.execution_plan(&ctx).await.unwrap();
    match plan {
        Some(ExecutionPlan::BatchWithStatus {
            commands,
            stderr_prefix,
            force_error_exit,
        }) => {
            assert_eq!(commands.len(), 1);
            assert!(stderr_prefix.contains("No such file or directory"));
            assert!(force_error_exit);
        }
        _ => panic!("expected BatchWithStatus plan"),
    }
}

// ==================== find output cap tests ====================

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

    let result = Find.execute(ctx).await.unwrap();
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

    let result = Find.execute(ctx).await.unwrap();
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

// ==================== ls -d / --directory tests ====================

/// `ls -d DIR` lists the directory entry itself, not its contents.
#[tokio::test]
async fn test_ls_directory_flag() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.mkdir(&cwd.join("subdir"), false).await.unwrap();
    fs.write_file(&cwd.join("subdir/inner.txt"), b"x")
        .await
        .unwrap();

    let args = vec!["-d".to_string(), "subdir".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "subdir\n");
    // The directory's contents must NOT be listed.
    assert!(!result.stdout.contains("inner.txt"));
}

/// `--directory` long form behaves like `-d`.
#[tokio::test]
async fn test_ls_directory_long_flag() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.mkdir(&cwd.join("d1"), false).await.unwrap();

    let args = vec!["--directory".to_string(), "d1".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "d1\n");
}

/// `ls -d` of multiple directory args (the `ls -d */` idiom, after the
/// shell expands the glob) lists each directory entry, not their contents.
#[tokio::test]
async fn test_ls_directory_multiple() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.mkdir(&cwd.join("alpha"), false).await.unwrap();
    fs.mkdir(&cwd.join("beta"), false).await.unwrap();
    fs.write_file(&cwd.join("alpha/inner.txt"), b"x")
        .await
        .unwrap();

    let args = vec!["-d".to_string(), "alpha".to_string(), "beta".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert!(result.stdout.contains("alpha"));
    assert!(result.stdout.contains("beta"));
    // No header lines and no descent into contents.
    assert!(!result.stdout.contains("alpha:"));
    assert!(!result.stdout.contains("inner.txt"));
}

/// `ls -dF DIR` appends the `/` classify suffix to the directory name.
#[tokio::test]
async fn test_ls_directory_classify() {
    let (fs, mut cwd, mut variables) = create_test_ctx().await;
    let env = HashMap::new();

    fs.mkdir(&cwd.join("mydir"), false).await.unwrap();

    let args = vec!["-d".to_string(), "-F".to_string(), "mydir".to_string()];
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

    let result = Ls.execute(ctx).await.unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "mydir/\n");
}
