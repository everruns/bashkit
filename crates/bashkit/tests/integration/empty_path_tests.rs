//! File operands must retain their meaning before VFS path normalization.
use bashkit::Bash;
use std::path::Path;

#[tokio::test]
async fn empty_path_find_rejects_only_the_invalid_starting_point() {
    let mut bash = Bash::new();
    bash.exec("mkdir /tmp/valid").await.unwrap();
    let result = bash
        .exec("find '' /tmp/valid -maxdepth 0 -print")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("No such file or directory"));
    assert_eq!(result.stdout, "/tmp/valid\n");

    for script in ["find -maxdepth 0 -print", "find . -maxdepth 0 -print"] {
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 0, "{script}: {}", result.stderr);
        assert_eq!(result.stdout, ".\n");
    }
}

#[tokio::test]
async fn empty_path_shell_file_tests_return_false() {
    let mut bash = Bash::new();
    for script in ["test -e ''", "[ -d '' ]", "[[ -d '' ]]"] {
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 1, "{script}");
        assert!(result.stderr.is_empty(), "{script}: {}", result.stderr);
    }
    for op in [
        "-e", "-d", "-L", "-h", "-b", "-c", "-p", "-S", "-u", "-g", "-k", "-N",
    ] {
        let result = bash.exec(&format!("[[ {op} '' ]]")).await.unwrap();
        assert_eq!(result.exit_code, 1, "{op}: {}", result.stderr);
    }
    assert_eq!(bash.exec("test -d .").await.unwrap().exit_code, 0);
}

#[tokio::test]
async fn empty_path_shell_file_comparisons_resolve_relative_to_cwd() {
    let mut bash = Bash::new();
    let setup = bash
        .exec("mkdir /tmp/work; cd /tmp/work; touch -t 200001010000 older; touch -t 200101010000 newer")
        .await
        .unwrap();
    assert_eq!(setup.exit_code, 0, "{}", setup.stderr);
    for (expression, code) in [
        ("newer -nt older", 0),
        ("older -ot newer", 0),
        ("older -ef /tmp/work/older", 0),
        ("'' -ef /tmp/work", 1),
        ("'' -ot older", 0),
        ("'' -nt older", 1),
    ] {
        let script = format!("[[ {expression} ]]");
        let result = bash.exec(&script).await.unwrap();
        assert_eq!(result.exit_code, code, "{script}: {}", result.stderr);
    }
}

#[tokio::test]
async fn empty_path_file_operations_keep_existing_files() {
    let mut bash = Bash::new();
    let setup = bash
        .exec("mkdir -p /tmp/work /tmp/empty; echo keep >/tmp/work/keep; cd /tmp/work")
        .await
        .unwrap();
    assert_eq!(setup.exit_code, 0, "{}", setup.stderr);
    let fs = bash.fs();
    let dir = Path::new("/tmp/work");
    let keep = Path::new("/tmp/work/keep");
    let before = fs.stat(dir).await.unwrap();
    for script in [
        "rm -rf -- ''",
        "rm -rf -- .",
        "rm -rf -- /tmp/work/../",
        "mv '' /tmp/moved",
        "mv /tmp/empty ''",
        "cp -r '' /tmp/copied",
        "cp -r /tmp/empty ''",
        "ln -s /tmp/empty ''",
        "ln -sf '' keep",
        "mkdir -p ''",
        "chmod 000 ''",
        "touch -t 200001010000 ''",
        "touch -r '' /tmp/created",
        "install -d -m 000 ''",
    ] {
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 1, "{script}: {}", result.stderr);
        assert!(!result.stderr.is_empty(), "{script}");
        assert_eq!(fs.read_file(keep).await.unwrap(), b"keep\n", "{script}");
    }
    let after = fs.stat(dir).await.unwrap();
    assert_eq!((after.mode, after.modified), (before.mode, before.modified));
    for path in [
        "/tmp/moved",
        "/tmp/copied",
        "/tmp/created",
        "/tmp/work/empty",
    ] {
        assert!(!fs.exists(Path::new(path)).await.unwrap(), "{path}");
    }

    let result = bash.exec("cd /tmp/empty; rmdir ''").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(fs.exists(Path::new("/tmp/empty")).await.unwrap());
}

#[tokio::test]
async fn empty_path_rm_processes_valid_operands_and_keeps_force_semantics() {
    let mut bash = Bash::new();
    let fs = bash.fs();
    let removable = Path::new("/tmp/remove-me");
    fs.write_file(removable, b"temporary").await.unwrap();

    let result = bash.exec("rm -f -- '' /tmp/remove-me").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert_eq!(
        result.stderr,
        "rm: cannot remove '': No such file or directory\n"
    );
    assert!(!fs.exists(removable).await.unwrap());

    for script in ["rm -f", "rm -f /tmp/missing", "touch -c ''"] {
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 0, "{script}: {}", result.stderr);
        assert!(result.stderr.is_empty(), "{script}");
    }
}

#[tokio::test]
async fn empty_path_direct_filesystem_operations_return_invalid_input() {
    let bash = Bash::new();
    let fs = bash.fs();
    let valid = Path::new("/tmp/keep");
    let empty = Path::new("");
    fs.write_file(valid, b"keep").await.unwrap();

    for result in [
        fs.remove(empty, true).await,
        fs.rename(empty, valid).await,
        fs.rename(valid, empty).await,
    ] {
        assert!(
            matches!(result, Err(bashkit::Error::Io(e)) if e.kind() == std::io::ErrorKind::InvalidInput)
        );
    }
    assert_eq!(fs.read_file(valid).await.unwrap(), b"keep");
}

#[cfg(feature = "python")]
#[tokio::test]
async fn empty_path_monty_os_calls_leave_working_directory_in_place() {
    let mut bash = Bash::builder()
        .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
        .python()
        .build();
    let result = bash.exec("mkdir /tmp/work; cd /tmp/work").await.unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);

    let result = bash
        .exec(r#"python -c 'import os; os.rmdir("")'"#)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(
        result.stderr.contains("FileNotFoundError"),
        "{}",
        result.stderr
    );
    assert!(bash.fs().exists(Path::new("/tmp/work")).await.unwrap());
}
