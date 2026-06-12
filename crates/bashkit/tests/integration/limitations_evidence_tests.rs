//! Evidence tests for `specs/limitations.md`.
//!
//! Each test demonstrates one intentional limitation (L-* row) so the
//! negative spec stays executable: if a limitation is ever lifted, the
//! matching test starts failing and the row must be removed in the same
//! change. Test names are cited in the doc's Evidence column;
//! `limitations_doc_tests` checks the citations resolve.

use bashkit::Bash;

/// L-PROC-002: no job control — `jobs`/`fg`/`bg` are not commands.
#[tokio::test]
async fn l_proc_002_no_job_control() {
    let mut bash = Bash::new();
    for cmd in ["jobs", "fg", "bg"] {
        let result = bash.exec(cmd).await.unwrap();
        assert_eq!(result.exit_code, 127, "{cmd} must be unknown");
        assert!(
            result.stderr.contains("command not found"),
            "{cmd}: {}",
            result.stderr
        );
    }
}

/// L-PROC-003: no process spawning — names outside the builtin registry,
/// functions, and aliases never reach a host exec; they fail as unknown.
#[tokio::test]
async fn l_proc_003_no_process_spawning() {
    let mut bash = Bash::new();
    // `sh -c 'echo hi'` style host escape: /bin/sh is not a spawnable path.
    let result = bash.exec("/bin/sh -c 'echo escaped'").await.unwrap();
    assert_eq!(result.exit_code, 127);
    assert!(
        result.stderr.contains("No such file or directory"),
        "stderr: {}",
        result.stderr
    );
    assert!(!result.stdout.contains("escaped"));

    let result = bash.exec("definitely-not-a-command").await.unwrap();
    assert_eq!(result.exit_code, 127);
    assert!(result.stderr.contains("command not found"));
}

/// L-FS-002: no permission enforcement — mode 000 files remain readable
/// and writable in the single-tenant VFS.
#[tokio::test]
async fn l_fs_002_no_permission_enforcement() {
    let mut bash = Bash::new();
    let result = bash
        .exec("echo secret > /f && chmod 000 /f && cat /f")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "secret\n");
}

/// L-NET-001: no raw sockets — bash's `/dev/tcp/HOST/PORT` pseudo-device
/// does not exist; redirecting to it fails instead of opening a socket.
#[tokio::test]
async fn l_net_001_no_raw_sockets() {
    let mut bash = Bash::new();
    let result = bash.exec("echo x > /dev/tcp/example.com/80").await.unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(
        result.stderr.contains("/dev/tcp"),
        "stderr: {}",
        result.stderr
    );
}

/// L-NET-002: default-deny networking — with no allowlist configured,
/// curl cannot reach any host (and nothing is ever DNS-resolved).
#[cfg(feature = "http_client")]
#[tokio::test]
async fn l_net_002_default_deny_no_resolution() {
    let mut bash = Bash::new();
    let result = bash.exec("curl -s https://example.com").await.unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(
        result.stderr.contains("network access not configured"),
        "must fail with the default-deny diagnostic, got: {}",
        result.stderr
    );
}

/// L-SIG-001: INT/TERM trap handlers are stored but never delivered in
/// virtual mode; EXIT traps do fire.
#[tokio::test]
async fn l_sig_001_signal_traps_not_delivered() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"trap 'echo TRAPPED' INT; kill -INT $$; echo after"#)
        .await
        .unwrap();
    assert!(
        !result.stdout.contains("TRAPPED"),
        "INT trap must not fire: {}",
        result.stdout
    );
    assert!(result.stdout.contains("after"));

    let result = bash
        .exec(r#"trap 'echo EXITED' EXIT; echo body"#)
        .await
        .unwrap();
    assert!(
        result.stdout.contains("EXITED"),
        "EXIT trap must fire: {}",
        result.stdout
    );
}

/// L-GREP-001: `--color`/`--line-buffered` accepted as no-ops — output is
/// byte-identical to plain grep.
#[tokio::test]
async fn l_grep_001_noop_flags() {
    let mut bash = Bash::new();
    let plain = bash.exec("printf 'a\\nb\\n' | grep a").await.unwrap();
    let colored = bash
        .exec("printf 'a\\nb\\n' | grep --color=always --line-buffered a")
        .await
        .unwrap();
    assert_eq!(plain.exit_code, 0);
    assert_eq!(colored.exit_code, 0);
    assert_eq!(plain.stdout, colored.stdout);
}
