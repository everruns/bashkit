//! The VFS is `/`-separated on every host (TM-ESC-033, #2425, #2933).
//!
//! `Path::join`/`PathBuf::push` join with `\` on Windows, so code that builds
//! VFS paths with them hands a custom filesystem `/a\b`, and string matches
//! such as `/dev/stderr` miss. Linux cannot show the bug (its separator is
//! already `/`), so these tests run on the Windows CI runner via the
//! `windows_containment` filter; `crates/bashkit/clippy.toml` bans the
//! methods statically. `CheckedFs` records every path the filesystem gets.

use bashkit::Bash;
use bashkit::testing::CheckedFs;
use std::sync::Arc;

async fn run(script: &str) -> (Arc<CheckedFs>, bashkit::ExecResult) {
    let fs = Arc::new(CheckedFs::new());
    let mut bash = Bash::builder().fs(fs.clone()).build();
    let result = bash.exec(script).await.unwrap();
    (fs, result)
}

#[tokio::test]
async fn windows_containment_dev_stderr_redirect_stays_in_interpreter() {
    let (fs, r) =
        run("echo err > /dev/stderr; echo out 2>&1 >/dev/stdout; echo two >> /dev/fd/2").await;
    assert_eq!(r.stderr, "err\ntwo\n");
    assert_eq!(r.stdout, "out\n");
    let writes: Vec<_> = fs
        .seen()
        .into_iter()
        .filter(|(op, p)| op.ends_with("_file") && p.starts_with("/dev"))
        .collect();
    assert!(
        writes.is_empty(),
        "device redirect reached the VFS: {writes:?}"
    );
    fs.assert_slash_only("dev redirect");
}

#[tokio::test]
async fn windows_containment_mkdir_p_and_cd_dotdot_use_slash_paths() {
    let (fs, r) = run("mkdir -p /a/b/c && cd /a/b/c/.. && pwd").await;
    assert_eq!(r.stdout, "/a/b\n", "stderr: {}", r.stderr);
    fs.assert_slash_only("mkdir -p / cd ..");
}

#[tokio::test]
async fn windows_containment_readlink_f_walks_slash_paths() {
    let (fs, r) =
        run("mkdir -p /a/b/c && ln -s /a/b /l && readlink -f /l/c && readlink -f /a/b/c/../c")
            .await;
    assert_eq!(r.stdout, "/a/b/c\n/a/b/c\n", "stderr: {}", r.stderr);
    assert!(
        fs.seen()
            .iter()
            .any(|(op, p)| *op == "read_link" && p == "/a/b"),
        "readlink -f should probe each prefix"
    );
    fs.assert_slash_only("readlink -f");
}

mod random_scripts {
    //! Random path-heavy scripts through `CheckedFs`. On Linux this only
    //! proves the harness; on the Windows runners (PR `windows_containment`
    //! filter, nightly full suite) any `\`-joined path fails it.

    use super::*;
    use proptest::prelude::*;

    fn path() -> impl Strategy<Value = String> {
        (
            any::<bool>(),
            prop::collection::vec(prop::sample::select(vec!["a", "b", "c", ".", ".."]), 1..5),
        )
            .prop_map(|(abs, segs)| {
                let p = segs.join("/");
                if abs { format!("/{p}") } else { p }
            })
    }

    fn command() -> impl Strategy<Value = String> {
        (
            prop::sample::select(vec![
                "mkdir -p {p}",
                "touch {p}",
                "cd {p}",
                "cat {p}",
                "ls -R {p}",
                "find {p}",
                "du {p}",
                "grep -r x {p}",
                "cp -r {p} {q}",
                "mv {p} {q}",
                "rm -rf {p}",
                "realpath {p}",
                "readlink -f {p}",
                "ln -s {p} {q}",
                "echo x > {p}",
                "echo x >> {p}",
                "wc -l {p}",
                "tar -cf {q}.tar {p}",
                "echo x > /dev/stderr",
                "echo x > /dev/fd/1",
            ]),
            path(),
            path(),
        )
            .prop_map(|(tpl, p, q)| tpl.replace("{p}", &p).replace("{q}", &q))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn windows_containment_random_path_scripts_stay_slash_only(
            cmds in prop::collection::vec(command(), 1..8)
        ) {
            let script = format!("mkdir -p /a/b/c; cd /a; {}", cmds.join("; "));
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let fs = Arc::new(CheckedFs::new());
            let mut bash = Bash::builder().fs(fs.clone()).build();
            let _ = rt.block_on(bash.exec(&script));
            let bad = fs.separator_violations();
            prop_assert!(bad.is_empty(), "script `{}` sent host-separator paths: {:?}", script, bad);
        }
    }
}
