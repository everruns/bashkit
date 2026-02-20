//! Tests for executing script files by path and $PATH search.
//!
//! Covers: absolute path, relative path, arguments, shebang stripping,
//! missing file, directory, permission denied, exit code propagation,
//! nested paths, and $PATH search.

use bashkit::Bash;
use std::path::Path;

/// Execute script by absolute path
#[tokio::test]
async fn exec_script_by_absolute_path() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/test.sh"), b"#!/bin/bash\necho hello")
        .await
        .unwrap();
    fs.chmod(Path::new("/test.sh"), 0o755).await.unwrap();

    let result = bash.exec("/test.sh").await.unwrap();
    assert_eq!(result.stdout.trim(), "hello");
    assert_eq!(result.exit_code, 0);
}

/// Execute script without shebang line
#[tokio::test]
async fn exec_script_without_shebang() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/no_shebang.sh"), b"echo no shebang")
        .await
        .unwrap();
    fs.chmod(Path::new("/no_shebang.sh"), 0o755).await.unwrap();

    let result = bash.exec("/no_shebang.sh").await.unwrap();
    assert_eq!(result.stdout.trim(), "no shebang");
    assert_eq!(result.exit_code, 0);
}

/// Execute script with arguments ($1, $2)
#[tokio::test]
async fn exec_script_with_args() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(
        Path::new("/greet.sh"),
        b"#!/bin/bash\necho \"Hello, $1 and $2!\"",
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/greet.sh"), 0o755).await.unwrap();

    let result = bash.exec("/greet.sh world moon").await.unwrap();
    assert_eq!(result.stdout.trim(), "Hello, world and moon!");
    assert_eq!(result.exit_code, 0);
}

/// $0 is set to the script name
#[tokio::test]
async fn exec_script_dollar_zero() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/show_name.sh"), b"#!/bin/bash\necho $0")
        .await
        .unwrap();
    fs.chmod(Path::new("/show_name.sh"), 0o755).await.unwrap();

    let result = bash.exec("/show_name.sh").await.unwrap();
    assert_eq!(result.stdout.trim(), "/show_name.sh");
    assert_eq!(result.exit_code, 0);
}

/// Nonexistent file returns "No such file or directory" (exit 127)
#[tokio::test]
async fn exec_script_missing_file() {
    let mut bash = Bash::new();

    let result = bash.exec("/missing.sh").await.unwrap();
    assert!(result.stderr.contains("No such file or directory"));
    assert_eq!(result.exit_code, 127);
}

/// Directory returns "Is a directory" (exit 126)
#[tokio::test]
async fn exec_script_is_directory() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.mkdir(Path::new("/mydir"), false).await.unwrap();

    let result = bash.exec("/mydir").await.unwrap();
    assert!(result.stderr.contains("Is a directory"));
    assert_eq!(result.exit_code, 126);
}

/// Not executable returns "Permission denied" (exit 126)
#[tokio::test]
async fn exec_script_permission_denied() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/noperm.sh"), b"echo nope")
        .await
        .unwrap();
    // Default mode is 0o644 — not executable

    let result = bash.exec("/noperm.sh").await.unwrap();
    assert!(result.stderr.contains("Permission denied"));
    assert_eq!(result.exit_code, 126);
}

/// Exit code propagation from script
#[tokio::test]
async fn exec_script_exit_code_propagation() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/fail.sh"), b"#!/bin/bash\nexit 42")
        .await
        .unwrap();
    fs.chmod(Path::new("/fail.sh"), 0o755).await.unwrap();

    let result = bash.exec("/fail.sh\necho $?").await.unwrap();
    assert_eq!(result.stdout.trim(), "42");
}

/// Nested directory paths work
#[tokio::test]
async fn exec_script_nested_path() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.mkdir(Path::new("/workspace/.agents/skills/nav/scripts"), true)
        .await
        .unwrap();
    fs.write_file(
        Path::new("/workspace/.agents/skills/nav/scripts/nav.sh"),
        b"#!/bin/bash\necho \"nav: $1\"",
    )
    .await
    .unwrap();
    fs.chmod(
        Path::new("/workspace/.agents/skills/nav/scripts/nav.sh"),
        0o755,
    )
    .await
    .unwrap();

    let result = bash
        .exec("/workspace/.agents/skills/nav/scripts/nav.sh dist")
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "nav: dist");
    assert_eq!(result.exit_code, 0);
}

/// $PATH search finds and executes script
#[tokio::test]
async fn exec_script_via_path_search() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.mkdir(Path::new("/usr/local/bin"), true).await.unwrap();
    fs.write_file(
        Path::new("/usr/local/bin/myscript"),
        b"#!/bin/bash\necho found",
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/usr/local/bin/myscript"), 0o755)
        .await
        .unwrap();

    let result = bash.exec("PATH=/usr/local/bin\nmyscript").await.unwrap();
    assert_eq!(result.stdout.trim(), "found");
    assert_eq!(result.exit_code, 0);
}

/// $PATH search skips non-executable files
#[tokio::test]
async fn path_search_skips_non_executable() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.mkdir(Path::new("/bin1"), false).await.unwrap();
    fs.mkdir(Path::new("/bin2"), false).await.unwrap();
    // /bin1/cmd exists but not executable
    fs.write_file(Path::new("/bin1/cmd"), b"echo wrong")
        .await
        .unwrap();
    // /bin2/cmd is executable
    fs.write_file(Path::new("/bin2/cmd"), b"echo right")
        .await
        .unwrap();
    fs.chmod(Path::new("/bin2/cmd"), 0o755).await.unwrap();

    let result = bash.exec("PATH=/bin1:/bin2\ncmd").await.unwrap();
    assert_eq!(result.stdout.trim(), "right");
}

/// $PATH search returns "command not found" when no match
#[tokio::test]
async fn path_search_command_not_found() {
    let mut bash = Bash::new();

    let result = bash.exec("PATH=\nnosuchcmd").await.unwrap();
    assert!(result.stderr.contains("command not found"));
    assert_eq!(result.exit_code, 127);
}

/// Script with relative path (contains /)
#[tokio::test]
async fn exec_script_relative_path() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.mkdir(Path::new("/workspace"), false).await.unwrap();
    fs.write_file(Path::new("/workspace/run.sh"), b"echo relative works")
        .await
        .unwrap();
    fs.chmod(Path::new("/workspace/run.sh"), 0o755)
        .await
        .unwrap();

    // Set cwd to /workspace so ./run.sh resolves
    let result = bash.exec("cd /workspace\n./run.sh").await.unwrap();
    assert_eq!(result.stdout.trim(), "relative works");
    assert_eq!(result.exit_code, 0);
}

/// Script calls another script by path
#[tokio::test]
async fn exec_script_calls_script() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/inner.sh"), b"#!/bin/bash\necho inner")
        .await
        .unwrap();
    fs.chmod(Path::new("/inner.sh"), 0o755).await.unwrap();

    fs.write_file(
        Path::new("/outer.sh"),
        b"#!/bin/bash\necho outer\n/inner.sh",
    )
    .await
    .unwrap();
    fs.chmod(Path::new("/outer.sh"), 0o755).await.unwrap();

    let result = bash.exec("/outer.sh").await.unwrap();
    assert_eq!(result.stdout, "outer\ninner\n");
    assert_eq!(result.exit_code, 0);
}

/// Script written via echo/redirect then chmod +x then executed
#[tokio::test]
async fn exec_script_chmod_then_run() {
    let mut bash = Bash::new();

    let result = bash
        .exec(
            "echo '#!/bin/bash\necho script ran' > /tmp/test_exec.sh\n\
             chmod +x /tmp/test_exec.sh\n\
             /tmp/test_exec.sh",
        )
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "script ran");
    assert_eq!(result.exit_code, 0);
}

/// $PATH search with args
#[tokio::test]
async fn path_search_with_args() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.mkdir(Path::new("/mybin"), false).await.unwrap();
    fs.write_file(Path::new("/mybin/greeter"), b"#!/bin/bash\necho \"hi $1\"")
        .await
        .unwrap();
    fs.chmod(Path::new("/mybin/greeter"), 0o755).await.unwrap();

    let result = bash.exec("PATH=/mybin\ngreeter alice").await.unwrap();
    assert_eq!(result.stdout.trim(), "hi alice");
}

/// Script $# shows argument count
#[tokio::test]
async fn exec_script_dollar_hash() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/count.sh"), b"#!/bin/bash\necho $#")
        .await
        .unwrap();
    fs.chmod(Path::new("/count.sh"), 0o755).await.unwrap();

    let result = bash.exec("/count.sh a b c").await.unwrap();
    assert_eq!(result.stdout.trim(), "3");
}

/// Script $@ shows all arguments
#[tokio::test]
async fn exec_script_dollar_at() {
    let mut bash = Bash::new();
    let fs = bash.fs();

    fs.write_file(Path::new("/all.sh"), b"#!/bin/bash\necho $@")
        .await
        .unwrap();
    fs.chmod(Path::new("/all.sh"), 0o755).await.unwrap();

    let result = bash.exec("/all.sh x y z").await.unwrap();
    assert_eq!(result.stdout.trim(), "x y z");
}
