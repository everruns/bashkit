//! Process substitution paths: `<(cmd)` / `>(cmd)` expand to `/dev/fd/N`
//! with bash's numbering (63, 62, ... per command), resolved in a per-shell
//! fd namespace, never as files in the (possibly shared) VFS.
//!
//! Expected output verified against GNU bash 5.2.

use bashkit::{
    Bash, DirEntry, FileSystem, FileSystemExt, InMemoryFs, Metadata, Result, async_trait,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

async fn run(script: &str) -> bashkit::ExecResult {
    let mut bash = Bash::builder().build();
    bash.exec(script).await.unwrap()
}

#[tokio::test]
async fn single_substitution_is_fd_63() {
    let r = run("echo <(true)").await;
    assert_eq!(r.stdout, "/dev/fd/63\n");
}

#[tokio::test]
async fn several_in_one_command_count_down() {
    let r = run("echo <(true) <(true) >(true) <(true)").await;
    assert_eq!(r.stdout, "/dev/fd/63 /dev/fd/62 /dev/fd/61 /dev/fd/60\n");
}

#[tokio::test]
async fn numbers_are_reused_by_the_next_command() {
    let r =
        run("echo <(true); echo <(true)\nfor i in 1 2; do echo <(true); done\n{ echo <(true); }")
            .await;
    assert_eq!(r.stdout, "/dev/fd/63\n".repeat(5));
}

#[tokio::test]
async fn nested_and_command_substitution_start_at_63() {
    let r = run(
        "echo $(echo <(true))\necho <(echo <(true))\ncat <(echo <(true) <(true))\n( echo <(true) )",
    )
    .await;
    assert_eq!(
        r.stdout,
        "/dev/fd/63\n/dev/fd/63\n/dev/fd/63 /dev/fd/62\n/dev/fd/63\n"
    );
}

#[tokio::test]
async fn live_caller_fds_are_skipped() {
    // The function's argument keeps fd 63 open while its body runs.
    let r = run("f() { echo <(true); cat \"$1\"; }; f <(echo outer)\n\
         g() { echo \"in g: $1\"; echo <(true) <(true); }; g <(true) <(true)\n\
         cat <(true) <(echo $(echo <(true)))")
    .await;
    assert_eq!(
        r.stdout,
        "/dev/fd/62\nouter\nin g: /dev/fd/63\n/dev/fd/61 /dev/fd/60\n/dev/fd/62\n"
    );
}

#[tokio::test]
async fn exec_opened_fd_is_skipped() {
    let r = run("exec 63>/dev/null; echo <(true); exec 63>&-; echo <(true)").await;
    assert_eq!(r.stdout, "/dev/fd/62\n/dev/fd/63\n");
}

#[tokio::test]
async fn consumers_read_the_substitution() {
    let r = run("cat <(echo a) <(echo b)\n\
         diff <(echo a) <(echo b); echo rc=$?\n\
         while read l; do echo \"got $l\"; done < <(printf '1\\n2\\n')\n\
         paste <(printf '1\\n2\\n') <(printf 'a\\nb\\n')\n\
         source <(echo 'echo sourced')\n\
         bash <(echo 'echo ran')\n\
         exec 3< <(echo three); cat <&3\n\
         [ -e <(true) ] && echo exists\n\
         test -r <(true) && echo readable\n\
         test -p <(true) && echo pipe\n\
         for x in 1; do cat <(echo loop); done < <(echo ignored); echo <(true)")
    .await;
    assert_eq!(
        r.stdout,
        "a\nb\n1c1\n< a\n---\n> b\nrc=1\ngot 1\ngot 2\n1\ta\n2\tb\nsourced\nran\nthree\nexists\nreadable\npipe\nloop\n/dev/fd/63\n"
    );
}

#[tokio::test]
async fn writers_feed_the_output_substitution() {
    let r = run("echo hi > >(tr a-z A-Z)\n\
         echo abc | tee >(rev) >/dev/null\n\
         cat > >(cat) <<< heredoc\n\
         set -C; echo noclobber > >(cat)")
    .await;
    assert_eq!(r.stdout, "HI\ncba\nheredoc\nnoclobber\n");
}

#[tokio::test]
async fn closed_substitution_is_gone() {
    // The fd closes when the command that created it finishes.
    let r = run("x=$(echo <(echo hi)); cat $x; echo rc=$?; cat /dev/fd/63; echo rc=$?").await;
    assert_eq!(r.stdout, "rc=1\nrc=1\n");
    assert!(r.stderr.contains("/dev/fd/63: No such file or directory"));
}

/// Records every path the shell hands to the shared filesystem.
struct RecordingFs {
    inner: Arc<dyn FileSystem>,
    touched: Mutex<Vec<PathBuf>>,
}

impl RecordingFs {
    fn note(&self, path: &Path) {
        self.touched.lock().unwrap().push(path.to_path_buf());
    }
    fn touched_dev_fd(&self) -> Vec<PathBuf> {
        self.touched
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.starts_with("/dev/fd"))
            .cloned()
            .collect()
    }
}

#[async_trait]
impl FileSystemExt for RecordingFs {}

#[async_trait]
impl FileSystem for RecordingFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.note(path);
        self.inner.read_file(path).await
    }
    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.note(path);
        self.inner.write_file(path, content).await
    }
    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.note(path);
        self.inner.append_file(path, content).await
    }
    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        self.note(path);
        self.inner.mkdir(path, recursive).await
    }
    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        self.note(path);
        self.inner.remove(path, recursive).await
    }
    async fn stat(&self, path: &Path) -> Result<Metadata> {
        self.note(path);
        self.inner.stat(path).await
    }
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        self.inner.read_dir(path).await
    }
    async fn exists(&self, path: &Path) -> Result<bool> {
        self.note(path);
        self.inner.exists(path).await
    }
    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.note(from);
        self.note(to);
        self.inner.rename(from, to).await
    }
    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        self.note(from);
        self.note(to);
        self.inner.copy(from, to).await
    }
    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        self.note(link);
        self.inner.symlink(target, link).await
    }
    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        self.note(path);
        self.inner.read_link(path).await
    }
    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        self.note(path);
        self.inner.chmod(path, mode).await
    }
    async fn set_modified_time(&self, path: &Path, time: SystemTime) -> Result<()> {
        self.note(path);
        self.inner.set_modified_time(path, time).await
    }
}

// THREAT[TM-ISO-028]: process substitution data lives in the shell's own fd
// namespace. Nothing reaches the shared filesystem, so a second interpreter
// on the same VFS can neither read nor clobber it.
#[tokio::test]
async fn substitution_never_reaches_shared_filesystem() {
    let shared = Arc::new(RecordingFs {
        inner: Arc::new(InMemoryFs::new()),
        touched: Mutex::new(Vec::new()),
    });
    let mut bash = Bash::builder()
        .fs(Arc::clone(&shared) as Arc<dyn FileSystem>)
        .build();
    let r = bash
        .exec(
            "cat <(echo secret); echo hi > >(cat); diff <(echo a) <(echo a) && echo same\n\
             f() { cat \"$1\"; [ -e \"$1\" ] && echo live; }; f <(echo arg)",
        )
        .await
        .unwrap();
    assert_eq!(r.stdout, "secret\nhi\nsame\narg\nlive\n");
    assert_eq!(shared.touched_dev_fd(), Vec::<PathBuf>::new());
}

#[tokio::test]
async fn second_interpreter_does_not_see_first_ones_fd() {
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    let mut a = Bash::builder().fs(Arc::clone(&fs)).build();
    let mut b = Bash::builder().fs(Arc::clone(&fs)).build();
    // `a` keeps /dev/fd/63 open across its function call; `b` runs while
    // `a`'s script is suspended between commands of the same exec.
    let ra = a
        .exec("f() { cat \"$1\"; }; f <(echo tenant-a)")
        .await
        .unwrap();
    assert_eq!(ra.stdout, "tenant-a\n");
    let rb = b
        .exec("cat /dev/fd/63; echo rc=$?; [ -e /dev/fd/63 ] || echo absent; echo x > /dev/fd/63; cat <(echo tenant-b)")
        .await
        .unwrap();
    assert_eq!(rb.stdout, "rc=1\nabsent\ntenant-b\n");
    assert!(!fs.exists(Path::new("/dev/fd/63")).await.unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_interpreters_keep_their_own_fd_63() {
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    let mut tasks = Vec::new();
    for t in 0..4 {
        let fs = Arc::clone(&fs);
        tasks.push(tokio::spawn(async move {
            let mut bash = Bash::builder().fs(fs).build();
            let script = format!(
                "for i in $(seq 1 40); do f() {{ sleep 0; cat \"$1\"; }}; f <(echo t{t}-$i); done"
            );
            let r = bash.exec(&script).await.unwrap();
            let expected: String = (1..=40).map(|i| format!("t{t}-{i}\n")).collect();
            assert_eq!(r.stdout, expected);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert!(!fs.exists(Path::new("/dev/fd/63")).await.unwrap());
}

#[tokio::test]
async fn exec_onto_output_substitution_does_not_abort() {
    // bash prints "kept" via cat; Bashkit drops it (limitations.md) but must
    // not fail the script on the closed fd.
    let r = run("exec 3> >(cat); echo kept >&3; exec 3>&-; echo after").await;
    assert!(r.stdout.ends_with("after\n"), "{}", r.stdout);
    assert_eq!(r.exit_code, 0);
}
