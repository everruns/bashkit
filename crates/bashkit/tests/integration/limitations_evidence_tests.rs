//! Evidence tests for `knowledge/operations/limitations.md`.
//!
//! Each test demonstrates one intentional limitation (L-* row) so the
//! negative spec stays executable: if a limitation is ever lifted, the
//! matching test starts failing and the row must be removed in the same
//! change. Test names are cited in the doc's Evidence column;
//! `limitations_doc_tests` checks the citations resolve.

use bashkit::Bash;

/// L-PROC-002: no stop/continue job control — background jobs run
/// concurrently, but `kill -STOP` cannot suspend one and `suspend` does
/// not exist.
#[tokio::test]
async fn l_proc_002_no_job_control() {
    let mut bash = Bash::new();
    let result = bash
        .exec("sleep 0.2 & kill -STOP %1; wait %1; echo rc=$?; suspend")
        .await
        .unwrap();
    // The job was not stopped: it ran to completion.
    assert!(result.stdout.contains("rc=0"), "stdout: {}", result.stdout);
    assert_eq!(result.exit_code, 127);
    assert!(result.stderr.contains("suspend: command not found"));
}

/// L-PROC-003: no process spawning — names outside the builtin registry,
/// functions, and aliases never reach a host exec; they fail as unknown.
#[tokio::test]
async fn l_proc_003_no_process_spawning() {
    let mut bash = Bash::new();
    // A host program path is not spawnable.
    let result = bash.exec("/usr/bin/gcc -v").await.unwrap();
    assert_eq!(result.exit_code, 127);
    assert!(
        result.stderr.contains("No such file or directory"),
        "stderr: {}",
        result.stderr
    );
    // `/bin/sh` is a root-filesystem stub for the in-process interpreter: it
    // sees the VFS, never the host.
    let result = bash
        .exec("echo vfs > /tmp/f; /bin/sh -c 'cat /tmp/f; [ \"$(readlink /proc/1/exe)\" = /bin/bash ] && echo no-host'")
        .await
        .unwrap();
    assert_eq!(result.stdout, "vfs\nno-host\n");

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
    // `kill -SIG $$` is the shell signalling itself, which the shell delivers:
    // the trap fires. No host signal can arrive from outside, so nothing else
    // ever triggers a handler.
    let result = bash
        .exec(r#"trap 'echo TRAPPED' INT; kill -INT $$; echo after"#)
        .await
        .unwrap();
    assert!(
        result.stdout.contains("TRAPPED"),
        "a self-sent INT must fire the trap: {}",
        result.stdout
    );
    assert!(result.stdout.contains("after"));

    // Without a handler the default action ends the script with 128 + signal.
    let result = bash
        .exec(r#"echo body; kill -TERM $$; echo never"#)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 143);
    assert!(!result.stdout.contains("never"), "{}", result.stdout);

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

/// L-GREP-002: GNU grep >= 3.8 warns "stray \ before /"; bashkit matches
/// the literal without the warning.
#[tokio::test]
async fn l_grep_002_no_stray_backslash_warning() {
    let mut bash = Bash::new();
    let r = bash.exec(r"echo a/b | grep 'a\/b'").await.unwrap();
    assert_eq!(r.stdout, "a/b\n");
    assert_eq!(r.stderr, "");
}

/// L-GREP-003: recursive grep visits entries in name order, not readdir
/// order.
#[tokio::test]
async fn l_grep_003_recursive_name_order() {
    let mut bash = Bash::new();
    let r = bash
        .exec("mkdir -p /t/d; echo x > /t/d/b; echo x > /t/d/a; grep -r x /t/d")
        .await
        .unwrap();
    assert_eq!(r.stdout, "/t/d/a:x\n/t/d/b:x\n");
}

/// L-TERM-001: `vi` is a subset — e.g. `:!cmd` (shell escape) is rejected as
/// an unknown editor command and runs nothing.
#[cfg(feature = "terminal")]
#[tokio::test]
async fn l_term_001_vi_is_a_subset() {
    use bashkit::terminal::{Terminal, TerminalStatus};
    let mut term = Terminal::new(Bash::builder());
    term.run_until_idle().await;
    term.send("vi /tmp/f\r:!touch /tmp/escaped\r");
    assert_eq!(term.run_until_idle().await, TerminalStatus::Idle);
    assert!(
        term.screen_text().contains("E492"),
        "{}",
        term.screen_text()
    );
    term.send(":q\r");
    term.run_until_idle().await;
    assert!(!term.fs().exists("/tmp/escaped".as_ref()).await.unwrap());
}

/// L-TERM-002: `read` in a terminal session gets EOF instead of waiting for
/// the next typed line.
#[cfg(feature = "terminal")]
#[tokio::test]
async fn l_term_002_cat_does_not_wait_for_terminal_input() {
    use bashkit::terminal::Terminal;
    let mut term = Terminal::new(Bash::builder());
    term.run_until_idle().await;
    term.send("cat; echo rc=$?\r");
    term.run_until_idle().await;
    assert!(
        term.screen_text().ends_with("rc=0\n$"),
        "{}",
        term.screen_text()
    );
}

/// L-PIPE-001: a leading single-builtin stage (other than the streaming
/// `yes`/`seq`/`cat`/`grep`/`tr`) runs to completion before `head` reads,
/// and never sees SIGPIPE: bash reports `141 0` here.
#[tokio::test]
async fn l_pipe_001_stages_run_sequentially() {
    let mut bash = Bash::new();
    let result = bash
        .exec("seq 20000 > /tmp/big; sort -n /tmp/big | head -1; echo \"${PIPESTATUS[*]}\"")
        .await
        .unwrap();
    assert_eq!(result.stdout, "1\n0 0\n");
}

/// Line filters stream too: `grep` and `tr` between an endless producer and
/// `head` stop with SIGPIPE, as in bash.
#[tokio::test]
async fn l_pipe_001_grep_tr_stream() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            "seq 20000 > /tmp/big; grep . /tmp/big | head -1; echo \"${PIPESTATUS[*]}\"\n\
             while :; do echo y; done | grep y | head -2; echo \"${PIPESTATUS[*]}\"\n\
             yes | tr y n | head -1; echo \"${PIPESTATUS[*]}\"\n\
             yes | tr -d '\\n' | head -c 5; echo \" ${PIPESTATUS[*]}\"\n\
             yes | grep -n y | head -2\n\
             yes | grep -o -m3 y | cat; echo \"${PIPESTATUS[*]}\"",
        )
        .await
        .unwrap();
    assert_eq!(
        result.stdout,
        "1\n141 0\ny\ny\n141 141 0\nn\n141 141 0\nyyyyy 141 141 0\n1:y\n2:y\ny\ny\ny\n141 0 0\n"
    );
    assert!(result.stderr.is_empty(), "{}", result.stderr);
}

/// Plain `cat` streams as a stage, so it gets SIGPIPE like bash, and a
/// `cat` between an endless loop and `head` no longer runs to a limit.
#[tokio::test]
async fn l_pipe_001_cat_streams() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            "seq 20000 > /tmp/big; cat /tmp/big | head -1; echo \"${PIPESTATUS[*]}\"\n\
             while :; do echo y; done | cat | head -2; echo \"${PIPESTATUS[*]}\"",
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "1\n141 0\ny\ny\n141 141 0\n");
    assert!(result.stderr.is_empty(), "{}", result.stderr);
}

/// The generators stream: `yes | head -1` stops with SIGPIPE, as in bash.
#[tokio::test]
async fn l_pipe_001_generators_stream() {
    let mut bash = Bash::new();
    let result = bash
        .exec("yes | head -1; echo \"${PIPESTATUS[*]}\"")
        .await
        .unwrap();
    assert_eq!(result.stdout, "y\n141 0\n");
    assert!(result.stderr.is_empty(), "{}", result.stderr);
}

/// Options that need the whole input still see all of it when the stage
/// reads a streaming pipe (`grep -c`, `tr -s`, context lines).
#[tokio::test]
async fn l_pipe_001_buffered_filter_options_drain_pipe() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            "src() { for i in 1 2 3 3 4; do echo \"a$i\"; done; }\n\
             src | grep -c a | cat\n\
             src | grep -A1 a2 | cat\n\
             src | tr -s 3 | tr -d '\\n' | cat; echo\n\
             src | grep -q a4 | cat; echo \"${PIPESTATUS[*]}\"",
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "5\na2\na3\na1a2a3a3a4\n0 0 0\n");
}

/// L-PROC-004: child shells nest at most 8 deep.
#[tokio::test]
async fn l_proc_004_child_shell_depth() {
    let mut bash = Bash::new();
    // /tmp/dN.sh runs /tmp/d(N+1).sh; /tmp/d9.sh prints.
    let setup = "for i in 1 2 3 4 5 6 7 8; do echo \"sh /tmp/d$((i+1)).sh\" > /tmp/d$i.sh; done; echo 'echo deep' > /tmp/d9.sh";
    bash.exec(setup).await.unwrap();
    // 8 nested shells: d2.sh .. d9.sh.
    let r = bash.exec("sh /tmp/d2.sh").await.unwrap();
    assert_eq!(r.stdout, "deep\n");
    let r = bash.exec("sh /tmp/d1.sh").await.unwrap();
    assert_eq!(r.stdout, "");
    assert!(
        r.stderr.contains("maximum nesting depth exceeded"),
        "{}",
        r.stderr
    );
}

/// L-MAKE-001: `$(eval)` and `$(file)` stop make with a fatal error instead
/// of being half-supported.
#[tokio::test]
async fn l_make_001_eval_and_file_unsupported() {
    let mut bash = Bash::new();
    for f in ["$(eval X=1)", "$(file >out,x)"] {
        let script = format!("printf 'all:\\n\\t@echo %s\\n' '{f}' > Makefile; make");
        let result = bash.exec(&script).await.unwrap();
        assert_eq!(result.exit_code, 2, "{f}");
        assert!(
            result.stderr.contains("L-MAKE-001"),
            "{f}: {}",
            result.stderr
        );
    }
}

/// L-MAKE-002: no built-in implicit rules, pattern rules do not chain
/// through intermediate files, and `vpath`/`VPATH` are not searched.
#[tokio::test]
async fn l_make_002_no_builtin_or_chained_rules() {
    let mut bash = Bash::new();
    // GNU make would compile x.c with its built-in %.o: %.c rule.
    let result = bash
        .exec("cd /tmp && touch x.c && printf 'all: x.o\\n' > Makefile && make")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(result.stderr.contains("No rule to make target 'x.o'"));
    // GNU make chains x.a -> x.b -> x.c.
    let result = bash
        .exec(
            "cd /tmp && touch x.a && printf '%%.b: %%.a\\n\\tcp $< $@\\n%%.c: %%.b\\n\\tcp $< $@\\nall: y.c\\n' > Makefile; mv x.a y.a; make",
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 2, "{}", result.stdout);
    assert!(result.stderr.contains("No rule to make target 'y.c'"));
    // GNU make finds src/z.c through vpath.
    let result = bash
        .exec("cd /tmp && mkdir -p src && touch src/z.c && printf 'vpath %%.c src\\nall: z.c\\n\\t@echo built\\n' > Makefile && make")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(!result.stdout.contains("built"));
}

/// L-MAKE-003: recipes always run in the sandbox shell one at a time:
/// `SHELL` is ignored, `-j` is accepted but sequential, and `$(MAKE)`
/// recursion fails at MAKELEVEL 4.
#[tokio::test]
async fn l_make_003_sequential_sandbox_shell() {
    let mut bash = Bash::new();
    let result = bash
        .exec("cd /tmp && printf 'SHELL=/bin/zsh\\nall: a b\\na:\\n\\t@echo a\\nb:\\n\\t@echo b\\n' > Makefile && make -j4")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "a\nb\n");
    let result = bash
        .exec("cd /tmp && printf 'r:\\n\\t@echo $(MAKELEVEL)\\n\\t@$(MAKE) -s r\\n' > Makefile && make -s r")
        .await
        .unwrap();
    assert_ne!(result.exit_code, 0);
    assert_eq!(result.stdout.lines().last(), Some("3"), "{}", result.stdout);
    assert!(result.stderr.contains("recursive make depth exceeds 4"));
}
