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
