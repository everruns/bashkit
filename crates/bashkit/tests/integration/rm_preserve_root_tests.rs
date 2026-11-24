//! `rm -r /` refuses like GNU rm's default `--preserve-root` failsafe.
//!
//! No real-bash oracle here on purpose: running these against the host shell
//! is exactly what the failsafe exists to prevent.
use bashkit::Bash;

const REFUSAL: &str = "rm: it is dangerous to operate recursively on '/'\n\
                       rm: use --no-preserve-root to override this failsafe\n";

#[tokio::test]
async fn rm_rf_root_refuses_and_keeps_files() {
    let mut bash = Bash::new();
    let r = bash
        .exec("echo keep > /tmp/f; rm -rf /; echo \"rc=$?\"; cat /tmp/f")
        .await
        .unwrap();
    assert_eq!(r.stdout, "rc=1\nkeep\n");
    assert_eq!(r.stderr, REFUSAL);
}

#[tokio::test]
async fn rm_r_root_spellings_resolve_to_root() {
    for spelling in ["/", "//", "///"] {
        let mut bash = Bash::new();
        let r = bash
            .exec(&format!(
                "rm -r {spelling}; echo \"rc=$?\"; [ -d /tmp ] && echo ok"
            ))
            .await
            .unwrap();
        assert_eq!(r.stdout, "rc=1\nok\n", "{spelling}: {}", r.stderr);
        assert!(
            r.stderr.contains("dangerous to operate recursively"),
            "{spelling}: {}",
            r.stderr
        );
    }
}

#[tokio::test]
async fn rm_rejects_dot_spellings_that_resolve_to_root() {
    for spelling in ["/tmp/..", "/./"] {
        let mut bash = Bash::new();
        let result = bash.exec(&format!("rm -rf -- {spelling}")).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("refusing to remove '.' or '..'"));
        assert!(
            bash.fs()
                .exists(std::path::Path::new("/tmp"))
                .await
                .unwrap()
        );
    }
}

#[tokio::test]
async fn rm_root_failsafe_does_not_stop_other_operands() {
    let mut bash = Bash::new();
    let r = bash
        .exec("mkdir -p /tmp/gone; rm -rf / /tmp/gone; echo \"rc=$?\"; [ -e /tmp/gone ] || echo removed")
        .await
        .unwrap();
    assert_eq!(r.stdout, "rc=1\nremoved\n");
}

#[tokio::test]
async fn rm_without_recursive_on_root_is_a_directory_error() {
    let mut bash = Bash::new();
    let r = bash.exec("rm /; echo \"rc=$?\"").await.unwrap();
    assert_eq!(r.stdout, "rc=1\n");
    assert_eq!(r.stderr, "rm: cannot remove '/': Is a directory\n");
}

#[tokio::test]
async fn rm_rf_glob_under_root_still_empties_the_sandbox() {
    // Matches real bash: `/*` expands to the top-level entries, which are not
    // `/` itself, so the failsafe does not apply.
    let mut bash = Bash::new();
    let r = bash
        .exec("echo x > /tmp/f; rm -rf /*; echo \"rc=$?\"; [ -e /tmp/f ] || echo gone")
        .await
        .unwrap();
    assert_eq!(r.stdout, "rc=0\ngone\n");
}
