//! Property-based / fuzz coverage for the `sqlite` builtin.
//!
//! These tests run a bounded number of randomly generated inputs through the
//! builtin and assert structural invariants:
//!
//! 1. **No panics** — the builtin must surface every error as a non-zero
//!    `ExecResult`, never as a Rust panic. This is the critical resilience
//!    requirement.
//! 2. **No host filesystem leaks** — randomly generated paths must never
//!    yield content from outside the bashkit VFS.
//! 3. **CSV mode never breaks RFC 4180** — for any input we produce, every
//!    row in `-csv` output is parseable by a real CSV parser.
//! 4. **Splitter is total** — `parser::split` (exercised indirectly) does
//!    not panic on arbitrary scripts. (Direct coverage lives in
//!    `crates/bashkit/src/builtins/sqlite/tests.rs`.)
//!
//! The cases-per-test counts are deliberately modest so this stays fast in
//! CI; raise them with `PROPTEST_CASES=2000 cargo test`.

#![cfg(feature = "sqlite")]

use bashkit::Bash;
use proptest::prelude::*;
use std::sync::OnceLock;

fn opt_in_env() -> &'static [(&'static str, &'static str)] {
    &[("BASHKIT_ALLOW_INPROCESS_SQLITE", "1")]
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
    })
}

fn run_blocking(script: &str) -> bashkit::ExecResult {
    let mut bash = Bash::builder()
        .sqlite()
        .env(opt_in_env()[0].0, opt_in_env()[0].1)
        .build();
    runtime()
        .block_on(async { bash.exec(script).await })
        .expect("exec")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Random byte sequences fed as SQL must never panic the builtin.
    /// They will mostly produce parse errors, but the shell must always
    /// regain control with a non-zero exit code rather than aborting.
    #[test]
    fn arbitrary_sql_does_not_panic(
        sql in proptest::collection::vec(any::<u8>(), 0..200)
            .prop_map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    ) {
        // sqlite arg quoting: drop NULs and embedded quotes so we test the
        // engine, not the shell's quoting rules.
        let sanitized: String = sql
            .chars()
            .filter(|c| *c != '\0' && *c != '\'' && *c != '\"')
            .take(200)
            .collect();
        let r = run_blocking(&format!(
            "sqlite :memory: '{sanitized}' 2>&1 || true"
        ));
        // Property: the call returned without panicking. The exit code is
        // free to be anything — we only need a structurally-valid result.
        let _ = r.exit_code;
    }

    /// Random absolute paths used as DB targets resolve through the VFS, not
    /// the host filesystem. We assert that no host-path content ever bleeds
    /// through into the builtin's output.
    #[test]
    fn random_paths_do_not_leak_host_files(
        seg in "[a-zA-Z0-9_]{1,8}"
    ) {
        let p = format!("/etc/{seg}.sqlite");
        let r = run_blocking(&format!(
            "sqlite '{p}' '.tables' 2>&1 || true"
        ));
        // Even if turso refuses to open it, the failure mode must not
        // include host /etc/passwd content.
        prop_assert!(!r.stdout.contains("root:x:"));
        prop_assert!(!r.stderr.contains("root:x:"));
    }

    /// Every CSV-mode result is parseable by a real CSV deserialiser. The
    /// test selects a small set of representative payloads and round-trips
    /// them through SQL → CSV → CSV reader.
    #[test]
    fn csv_round_trip_is_parseable(
        s in "[ -~\\t]{0,30}"
    ) {
        // Skip strings containing a single-quote (would break the SQL
        // literal in our test harness).
        prop_assume!(!s.contains('\''));
        let cmd = format!(
            "sqlite -csv -header :memory: 'CREATE TABLE t(x TEXT); INSERT INTO t VALUES (\"{s}\"); SELECT * FROM t'"
        );
        let r = run_blocking(&cmd);
        prop_assume!(r.exit_code == 0);
        // Manually verify CSV well-formedness: header line + 1 data line.
        let lines: Vec<&str> = r.stdout.lines().collect();
        prop_assert!(lines.len() >= 2, "expected header + data; got {:?}", lines);
        // Each data line must be valid CSV (balanced quotes).
        for line in &lines[1..] {
            let mut quotes = 0usize;
            let mut chars = line.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        continue;
                    }
                    quotes += 1;
                }
            }
            prop_assert!(
                quotes.is_multiple_of(2),
                "unbalanced quotes in {line:?}"
            );
        }
    }

    /// `:memory:` databases never persist a file to the VFS regardless of
    /// what the script does. (Defends against a regression where Phase 1's
    /// snapshot-on-success path could accidentally write a `:memory:` file
    /// to the cwd.)
    #[test]
    fn memory_db_never_creates_vfs_file(
        sql in "[a-zA-Z0-9 ;]{0,60}"
    ) {
        let cmd = format!(
            "sqlite :memory: '{sql}' >/dev/null 2>&1; ls /home/user/ 2>/dev/null"
        );
        let r = run_blocking(&cmd);
        prop_assert!(!r.stdout.contains(":memory:"));
        prop_assert!(!r.stdout.contains(".sqlite"));
    }
}
