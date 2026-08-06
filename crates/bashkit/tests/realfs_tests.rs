//! Integration tests for RealFs feature.
//!
//! Tests the full pipeline: host directory → RealFs → PosixFs → Bash interpreter.

#![cfg(feature = "realfs")]

use bashkit::{Bash, BashBuilder};
use std::path::Path;

#[cfg(windows)]
mod windows_containment {
    use super::*;
    use bashkit::{FileSystem, FsBackend, InMemoryFs, OverlayFs, RealFs, RealFsMode};
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::Arc;

    fn device_path(path: &Path) -> PathBuf {
        PathBuf::from(format!(r"\\?\{}", path.display()))
    }

    fn create_junction(link: &Path, target: &Path) {
        let status = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .status()
            .unwrap();
        assert!(status.success(), "failed to create test junction");
    }

    #[tokio::test]
    async fn windows_containment_vfs_and_overlay_are_rooted_and_case_sensitive() {
        let lower = Arc::new(InMemoryFs::new());
        lower.mkdir(Path::new("/Case"), false).await.unwrap();
        lower
            .write_file(Path::new("/Case/file.txt"), b"lower")
            .await
            .unwrap();
        let overlay = OverlayFs::new(lower);

        assert_eq!(
            overlay
                .read_file(Path::new(r"\Case\file.txt"))
                .await
                .unwrap(),
            b"lower"
        );
        assert!(!overlay.exists(Path::new("/case/file.txt")).await.unwrap());

        overlay
            .write_file(Path::new(r"C:\Case\upper.txt"), b"upper")
            .await
            .unwrap();
        assert_eq!(
            overlay
                .read_file(Path::new("/Case/upper.txt"))
                .await
                .unwrap(),
            b"upper"
        );
    }

    #[tokio::test]
    async fn windows_containment_realfs_rejects_drive_and_device_absolute_escape() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("root");
        let outside = sandbox.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("secret.txt");
        std::fs::write(&secret, b"outside-secret").unwrap();

        let fs = RealFs::open(&root, RealFsMode::ReadOnly).await.unwrap();
        for hostile in [secret.clone(), device_path(&secret)] {
            let result = fs.read(&hostile).await;
            assert!(
                !matches!(result, Ok(ref bytes) if bytes == b"outside-secret"),
                "host path escaped RealFs root: {}",
                hostile.display()
            );
        }
    }

    #[tokio::test]
    async fn windows_containment_realfs_keeps_missing_descendants_under_root() {
        let root = tempfile::tempdir().unwrap();
        let fs = RealFs::open(root.path(), RealFsMode::ReadWrite)
            .await
            .unwrap();

        fs.write(Path::new(r"\missing\deep\file.txt"), b"inside")
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(root.path().join("missing/deep/file.txt")).unwrap(),
            b"inside"
        );
    }

    #[tokio::test]
    async fn windows_containment_realfs_rejects_drive_relative_symlink_target() {
        let root = tempfile::tempdir().unwrap();
        let fs = RealFs::open(root.path(), RealFsMode::ReadWrite)
            .await
            .unwrap();

        assert!(
            fs.symlink(Path::new(r"C:target.txt"), Path::new("/link"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn windows_containment_realfs_handles_case_and_alternate_separators_inside_root() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("MixedCase.TXT"), b"inside").unwrap();
        let fs = RealFs::open(root.path(), RealFsMode::ReadOnly)
            .await
            .unwrap();

        assert_eq!(
            fs.read(Path::new(r"\mixedcase.txt")).await.unwrap(),
            b"inside"
        );
    }

    #[tokio::test]
    async fn windows_containment_builder_refuses_drive_root_without_allowlist() {
        let sandbox = tempfile::tempdir().unwrap();
        let probe = sandbox.path().join("drive-root-probe.txt");
        std::fs::write(&probe, b"host-secret").unwrap();
        let drive_root = sandbox.path().ancestors().last().unwrap();
        let relative_probe = probe
            .strip_prefix(drive_root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");

        let bash = Bash::builder()
            .mount_real_readonly_at(drive_root, "/host")
            .build();
        let result = bash
            .fs()
            .read_file(Path::new(&format!("/host/{relative_probe}")))
            .await;
        assert!(!matches!(result, Ok(ref bytes) if bytes == b"host-secret"));
    }

    #[tokio::test]
    async fn windows_containment_realfs_blocks_symlink_and_junction_targets() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("root");
        let root_prefix_sibling = sandbox.path().join("root-evil");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&root_prefix_sibling).unwrap();
        std::fs::write(root_prefix_sibling.join("secret.txt"), b"outside-secret").unwrap();

        std::os::windows::fs::symlink_file(
            root_prefix_sibling.join("secret.txt"),
            root.join("file-link"),
        )
        .unwrap();
        create_junction(&root.join("junction"), &root_prefix_sibling);

        let fs = RealFs::open(&root, RealFsMode::ReadOnly).await.unwrap();
        for hostile in [
            Path::new("/file-link"),
            Path::new("/junction/secret.txt"),
            Path::new("/junction/missing/descendant.txt"),
        ] {
            let result = fs.read(hostile).await;
            assert!(
                !matches!(result, Ok(ref bytes) if bytes == b"outside-secret"),
                "reparse target escaped RealFs root: {}",
                hostile.display()
            );
        }
    }
}

#[path = "support/filesystem_security_conformance.rs"]
mod filesystem_security_conformance;

// macOS temp dirs canonicalize under /private, which RealFs treats as
// sensitive. Tests allowlist only the temp fixtures they mount.
fn builder_allowing_host_paths(paths: &[&Path]) -> BashBuilder {
    Bash::builder().allowed_mount_paths(paths.iter().map(|path| (*path).to_path_buf()))
}

fn setup_host_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "hello world\n").unwrap();
    std::fs::create_dir(dir.path().join("subdir")).unwrap();
    std::fs::write(dir.path().join("subdir/nested.txt"), "nested\n").unwrap();
    std::fs::write(dir.path().join("data.csv"), "a,1\nb,2\nc,3\n").unwrap();
    dir
}

#[tokio::test]
async fn realfs_passes_shared_filesystem_security_conformance() {
    use bashkit::{PosixFs, RealFs, RealFsMode};

    let dir = tempfile::tempdir().unwrap();
    let backend = RealFs::open(dir.path(), RealFsMode::ReadWrite)
        .await
        .unwrap();
    let fs = PosixFs::new(backend);
    filesystem_security_conformance::certify_path_and_data_contract("realfs", &fs).await;
}

#[cfg(unix)]
#[tokio::test]
async fn realfs_copy_and_rename_preserve_symlink_identity() {
    use bashkit::{FileSystem, PosixFs, RealFs, RealFsMode};

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("target"), b"target").unwrap();
    std::os::unix::fs::symlink("target", dir.path().join("link")).unwrap();
    let fs = PosixFs::new(
        RealFs::open(dir.path(), RealFsMode::ReadWrite)
            .await
            .unwrap(),
    );

    fs.copy(Path::new("/link"), Path::new("/copied"))
        .await
        .unwrap();
    fs.rename(Path::new("/link"), Path::new("/moved"))
        .await
        .unwrap();

    assert!(
        fs.stat(Path::new("/copied"))
            .await
            .unwrap()
            .file_type
            .is_symlink()
    );
    assert!(
        fs.stat(Path::new("/moved"))
            .await
            .unwrap()
            .file_type
            .is_symlink()
    );
    assert_eq!(
        fs.read_link(Path::new("/copied")).await.unwrap(),
        Path::new("target")
    );
    assert_eq!(
        fs.read_link(Path::new("/moved")).await.unwrap(),
        Path::new("target")
    );
}

#[tokio::test]
async fn realfs_failed_copy_retains_destination_and_removes_staging_file() {
    use bashkit::{FsBackend, PosixFs, RealFs, RealFsMode};

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("source-dir")).unwrap();
    std::fs::write(dir.path().join("destination"), b"original").unwrap();
    let fs = PosixFs::new(
        RealFs::open(dir.path(), RealFsMode::ReadWrite)
            .await
            .unwrap(),
    );

    assert!(
        fs.backend()
            .copy(Path::new("/source-dir"), Path::new("/destination"))
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(dir.path().join("destination")).unwrap(),
        b"original"
    );
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".bashkit-tmp-")
    }));
}

#[tokio::test]
async fn realfs_replacement_race_never_exposes_partial_content() {
    use bashkit::{FileSystem, PosixFs, RealFs, RealFsMode};
    use std::sync::Arc;

    let dir = tempfile::tempdir().unwrap();
    let fs = Arc::new(PosixFs::new(
        RealFs::open(dir.path(), RealFsMode::ReadWrite)
            .await
            .unwrap(),
    ));
    let first = vec![b'a'; 64 * 1024];
    let second = vec![b'b'; 64 * 1024];
    fs.write_file(Path::new("/raced"), &first).await.unwrap();

    let writer_fs = Arc::clone(&fs);
    let writer_first = first.clone();
    let writer_second = second.clone();
    let writer = async move {
        for iteration in 0..64 {
            let content = if iteration % 2 == 0 {
                &writer_second
            } else {
                &writer_first
            };
            writer_fs
                .write_file(Path::new("/raced"), content)
                .await
                .unwrap();
        }
    };
    let reader = async {
        for _ in 0..256 {
            let content = fs.read_file(Path::new("/raced")).await.unwrap();
            assert!(content == first || content == second);
        }
    };

    tokio::join!(writer, reader);
}

#[cfg(unix)]
mod async_runtime_regression {
    use super::*;
    use bashkit::{
        Builtin, BuiltinContext, ExecResult, FileSystem, PosixFs, RealFs, RealFsMode, async_trait,
    };
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    struct DelayedFifoWriter {
        host_path: PathBuf,
        rescued: Arc<AtomicBool>,
    }

    #[async_trait]
    impl Builtin for DelayedFifoWriter {
        async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
            let host_path = self.host_path.clone();
            let rescued = Arc::clone(&self.rescued);
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                if !rescued.load(Ordering::SeqCst) {
                    let _writer = tokio::fs::OpenOptions::new()
                        .write(true)
                        .open(host_path)
                        .await
                        .unwrap();
                }
            });
            Ok(ExecResult::ok("captured output\n".to_string()))
        }
    }

    enum RuntimeKind {
        CurrentThread,
        MultiThread,
    }

    fn run_fifo_pipeline(kind: RuntimeKind) {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("wake.fifo");
        let status = Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(status.success());

        let rescued = Arc::new(AtomicBool::new(false));
        let (done_tx, done_rx) = mpsc::channel();
        let runtime_dir = dir.path().to_path_buf();
        let runtime_fifo = fifo.clone();
        let runtime_rescued = Arc::clone(&rescued);

        let runtime_thread = std::thread::spawn(move || {
            let mut builder = tokio::runtime::Builder::new_current_thread();
            if matches!(kind, RuntimeKind::MultiThread) {
                builder = tokio::runtime::Builder::new_multi_thread();
                builder.worker_threads(2);
            }
            let runtime = builder.enable_all().build().unwrap();
            let result = runtime.block_on(async move {
                let backend = RealFs::open(&runtime_dir, RealFsMode::ReadWrite)
                    .await
                    .unwrap();
                let fs: Arc<dyn FileSystem> = Arc::new(PosixFs::new(backend));
                let mut bash = Bash::builder()
                    .builtin(
                        "wake-fifo",
                        Box::new(DelayedFifoWriter {
                            host_path: runtime_fifo,
                            rescued: runtime_rescued,
                        }),
                    )
                    .build();
                bash.mount("/workspace", fs).unwrap();
                bash.exec("wake-fifo | touch /workspace/wake.fifo; echo done")
                    .await
                    .map(|output| output.stdout)
            });
            done_tx.send(result).unwrap();
        });

        let result = match done_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                rescued.store(true, Ordering::SeqCst);
                let rescue_fifo = fifo.clone();
                let rescue = std::thread::spawn(move || {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(rescue_fifo)
                        .unwrap()
                });
                runtime_thread.join().unwrap();
                drop(rescue.join().unwrap());
                panic!("RealFs blocked the Tokio runtime during async pipeline execution");
            }
            Err(error) => panic!("runtime thread disconnected: {error}"),
        };

        runtime_thread.join().unwrap();
        assert_eq!(result.unwrap(), "done\n");
    }

    #[test]
    fn realfs_pipeline_does_not_block_current_thread_runtime() {
        run_fifo_pipeline(RuntimeKind::CurrentThread);
    }

    #[test]
    fn realfs_pipeline_completes_on_multi_thread_runtime() {
        run_fifo_pipeline(RuntimeKind::MultiThread);
    }
}

// --- Use case 1: readonly overlay at root ---

#[tokio::test]
async fn readonly_root_overlay_cat() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly(dir.path())
        .build();

    let result = bash.exec("cat /hello.txt").await.unwrap();
    assert_eq!(result.stdout, "hello world\n");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn readonly_root_overlay_ls() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly(dir.path())
        .build();

    let result = bash.exec("ls /").await.unwrap();
    assert!(result.stdout.contains("hello.txt"));
    assert!(result.stdout.contains("subdir"));
}

#[tokio::test]
async fn readonly_root_overlay_nested() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly(dir.path())
        .build();

    let result = bash.exec("cat /subdir/nested.txt").await.unwrap();
    assert_eq!(result.stdout, "nested\n");
}

#[tokio::test]
async fn readonly_root_overlay_write_goes_to_memory() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly(dir.path())
        .build();

    // Write a new file - should go to in-memory overlay
    bash.exec("echo 'vfs only' > /new_file.txt").await.unwrap();
    let result = bash.exec("cat /new_file.txt").await.unwrap();
    assert_eq!(result.stdout, "vfs only\n");

    // Host should NOT have this file
    assert!(!dir.path().join("new_file.txt").exists());
}

#[tokio::test]
async fn readonly_root_overlay_pipes() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly(dir.path())
        .build();

    let result = bash.exec("cat /data.csv | grep b").await.unwrap();
    assert_eq!(result.stdout, "b,2\n");
}

#[tokio::test]
async fn readonly_root_overlay_wc() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly(dir.path())
        .build();

    let result = bash.exec("wc -l < /data.csv").await.unwrap();
    assert_eq!(result.stdout.trim(), "3");
}

// --- Use case 2: readonly mount at specific path ---

#[tokio::test]
async fn readonly_mount_at_path_cat() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    let result = bash.exec("cat /mnt/data/hello.txt").await.unwrap();
    assert_eq!(result.stdout, "hello world\n");
}

#[tokio::test]
async fn readonly_mount_at_path_ls() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    let result = bash.exec("ls /mnt/data").await.unwrap();
    assert!(result.stdout.contains("hello.txt"));
    assert!(result.stdout.contains("subdir"));
}

#[tokio::test]
async fn readonly_mount_at_path_vfs_root_intact() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    // VFS root should still have default dirs
    let result = bash
        .exec("test -d /tmp && echo yes || echo no")
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "yes");

    // Can write to VFS normally
    bash.exec("echo test > /tmp/test.txt").await.unwrap();
    let result = bash.exec("cat /tmp/test.txt").await.unwrap();
    assert_eq!(result.stdout, "test\n");
}

#[tokio::test]
async fn readonly_filesystem_blocks_copy_from_mount_to_tmp() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .readonly_filesystem(true)
        .build();

    let read = bash.exec("cat /mnt/data/hello.txt").await.unwrap();
    assert_eq!(read.stdout, "hello world\n");

    let copy = bash
        .exec("cp /mnt/data/hello.txt /tmp/copied.txt")
        .await
        .unwrap();
    assert_ne!(copy.exit_code, 0);
    assert!(copy.stderr.contains("read-only"));

    let redirect = bash.exec("printf nope > /tmp/nope.txt").await.unwrap();
    assert_ne!(redirect.exit_code, 0);
    assert!(redirect.stderr.contains("read-only"));
}

// --- Use case 3: readwrite mount ---

#[tokio::test]
async fn readwrite_mount_modifies_host() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readwrite_at(dir.path(), "/workspace")
        .build();

    // Read existing file
    let result = bash.exec("cat /workspace/hello.txt").await.unwrap();
    assert_eq!(result.stdout, "hello world\n");

    // Write to host file (overwrite)
    bash.exec("echo 'modified by bash' > /workspace/hello.txt")
        .await
        .unwrap();

    // Verify on host
    let content = std::fs::read_to_string(dir.path().join("hello.txt")).unwrap();
    assert_eq!(content, "modified by bash\n");

    // Append to host file
    bash.exec("echo 'appended line' >> /workspace/hello.txt")
        .await
        .unwrap();

    let content = std::fs::read_to_string(dir.path().join("hello.txt")).unwrap();
    assert!(
        content.contains("appended line"),
        "append should modify host file, got: {:?}",
        content
    );
}

#[tokio::test]
async fn readwrite_mount_creates_files_on_host() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readwrite_at(dir.path(), "/workspace")
        .build();

    bash.exec("echo 'new' > /workspace/created.txt")
        .await
        .unwrap();

    assert!(dir.path().join("created.txt").exists());
    let content = std::fs::read_to_string(dir.path().join("created.txt")).unwrap();
    assert_eq!(content, "new\n");
}

#[tokio::test]
async fn readwrite_mount_creates_dirs_on_host() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readwrite_at(dir.path(), "/workspace")
        .build();

    bash.exec("mkdir -p /workspace/a/b/c").await.unwrap();
    assert!(dir.path().join("a/b/c").is_dir());
}

#[tokio::test]
async fn readwrite_root_overlay() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readwrite(dir.path())
        .build();

    let result = bash.exec("cat /hello.txt").await.unwrap();
    assert_eq!(result.stdout, "hello world\n");

    // Write goes to overlay (in-memory), not host, because OverlayFs wraps it
    bash.exec("echo 'overlay' > /overlay_file.txt")
        .await
        .unwrap();
    let result = bash.exec("cat /overlay_file.txt").await.unwrap();
    assert_eq!(result.stdout, "overlay\n");
}

// --- Multiple mounts ---

#[tokio::test]
async fn multiple_readonly_mounts() {
    let dir1 = setup_host_dir();
    let dir2 = tempfile::tempdir().unwrap();
    std::fs::write(dir2.path().join("other.txt"), "from dir2\n").unwrap();

    let mut bash = builder_allowing_host_paths(&[dir1.path(), dir2.path()])
        .mount_real_readonly_at(dir1.path(), "/mnt/a")
        .mount_real_readonly_at(dir2.path(), "/mnt/b")
        .build();

    let result = bash.exec("cat /mnt/a/hello.txt").await.unwrap();
    assert_eq!(result.stdout, "hello world\n");

    let result = bash.exec("cat /mnt/b/other.txt").await.unwrap();
    assert_eq!(result.stdout, "from dir2\n");
}

#[tokio::test]
async fn mixed_readonly_and_text_mounts() {
    let dir = setup_host_dir();

    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly_at(dir.path(), "/mnt/host")
        .mount_text("/config/app.toml", "key = 'value'\n")
        .build();

    let result = bash.exec("cat /mnt/host/hello.txt").await.unwrap();
    assert_eq!(result.stdout, "hello world\n");

    let result = bash.exec("cat /config/app.toml").await.unwrap();
    assert_eq!(result.stdout, "key = 'value'\n");
}

// --- Security: path traversal ---

#[tokio::test]
async fn path_traversal_blocked_via_bash() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    // Attempt traversal - should not leak files outside the mount root
    let result = bash
        .exec("cat /mnt/data/../../etc/passwd 2>&1")
        .await
        .unwrap();
    // This should fail or return content from VFS, not from host /etc/passwd
    assert!(result.exit_code != 0 || !result.stdout.contains("root:"));
}

// --- Direct filesystem API ---

#[tokio::test]
async fn direct_fs_api_read() {
    let dir = setup_host_dir();
    let bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    let fs = bash.fs();
    let content = fs
        .read_file(Path::new("/mnt/data/hello.txt"))
        .await
        .unwrap();
    assert_eq!(content, b"hello world\n");
}

#[tokio::test]
async fn direct_fs_api_stat() {
    let dir = setup_host_dir();
    let bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    let fs = bash.fs();
    let meta = fs.stat(Path::new("/mnt/data/hello.txt")).await.unwrap();
    assert!(meta.file_type.is_file());
    assert_eq!(meta.size, 12); // "hello world\n"
}

#[tokio::test]
async fn direct_fs_api_exists() {
    let dir = setup_host_dir();
    let bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    let fs = bash.fs();
    assert!(fs.exists(Path::new("/mnt/data/hello.txt")).await.unwrap());
    assert!(!fs.exists(Path::new("/mnt/data/nope.txt")).await.unwrap());
}

// ==================== Symlink sandbox escape prevention (Issue #979) ====================

#[tokio::test]
async fn realfs_symlink_absolute_escape_blocked() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readwrite_at(dir.path(), "/mnt/workspace")
        .build();

    // Attempt to create a symlink pointing to /etc/passwd
    let r = bash
        .exec("ln -s /etc/passwd /mnt/workspace/escape 2>&1; echo $?")
        .await
        .unwrap();
    // Should fail with non-zero exit code
    assert!(
        r.stdout.trim().ends_with('1')
            || r.stdout.contains("not allowed")
            || r.stdout.contains("Permission denied"),
        "Symlink creation should be blocked, got: {}",
        r.stdout
    );
}

#[tokio::test]
async fn realfs_symlink_relative_escape_blocked() {
    let dir = setup_host_dir();
    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readwrite_at(dir.path(), "/mnt/workspace")
        .build();

    // Attempt relative path traversal via symlink
    let r = bash
        .exec("ln -s ../../../../etc/passwd /mnt/workspace/escape 2>&1; echo $?")
        .await
        .unwrap();
    assert!(
        r.stdout.trim().ends_with('1')
            || r.stdout.contains("not allowed")
            || r.stdout.contains("Permission denied"),
        "Relative symlink escape should be blocked, got: {}",
        r.stdout
    );
}

#[tokio::test]
async fn realfs_symlink_within_mount_allowed() {
    let dir = setup_host_dir();
    std::fs::write(dir.path().join("original.txt"), "content").unwrap();

    let mut bash = builder_allowing_host_paths(&[dir.path()])
        .mount_real_readwrite_at(dir.path(), "/mnt/workspace")
        .build();

    // Relative symlink within mount should succeed (exit code 0)
    let r = bash
        .exec("ln -s original.txt /mnt/workspace/link.txt 2>&1; echo $?")
        .await
        .unwrap();
    assert!(
        r.stdout.trim().ends_with('0'),
        "Symlink within mount should succeed, got stdout: {} stderr: {}",
        r.stdout,
        r.stderr
    );
}

// --- Mount path validation ---

#[tokio::test]
async fn mount_allowlist_blocks_unlisted_path() {
    let dir = setup_host_dir();
    std::fs::write(dir.path().join("data.txt"), "secret").unwrap();

    // Mount with allowlist that does NOT include the dir
    let mut bash = Bash::builder()
        .allowed_mount_paths(["/nonexistent/allowed"])
        .mount_real_readonly_at(dir.path(), "/mnt/data")
        .build();

    // The mount should have been skipped — file should not be accessible
    let r = bash
        .exec("cat /mnt/data/data.txt 2>&1; echo $?")
        .await
        .unwrap();
    assert!(
        r.stdout.trim().ends_with('1') || r.stdout.contains("No such file"),
        "Mount outside allowlist should be blocked, got: {}",
        r.stdout
    );
}

#[tokio::test]
async fn mount_sensitive_path_blocked() {
    // Attempting to mount /proc should be silently blocked
    let mut bash = Bash::builder()
        .mount_real_readonly_at("/proc", "/mnt/proc")
        .build();

    let r = bash.exec("ls /mnt/proc 2>&1; echo $?").await.unwrap();
    assert!(
        r.stdout.trim().ends_with('1') || r.stdout.contains("No such file"),
        "Sensitive path /proc should be blocked, got: {}",
        r.stdout
    );
}

/// THREAT[TM-FS-013]: Each broad host root must be refused without an
/// explicit `allowed_mount_paths` opt-in. Each path is tested independently
/// because canonicalization can fail for some on a given host (e.g. /Users
/// only exists on macOS). A path that doesn't exist canonicalizes to an
/// error and the mount is skipped before reaching the sensitive-path check;
/// that is also a refusal, so the regression invariant holds either way.
#[tokio::test]
async fn mount_root_filesystem_blocked_without_allowlist() {
    let mut bash = Bash::builder()
        .mount_real_readonly_at("/", "/mnt/host")
        .build();
    let r = bash.exec("ls /mnt/host 2>&1; echo $?").await.unwrap();
    assert!(
        r.stdout.trim().ends_with('1') || r.stdout.contains("No such file"),
        "Mounting / must be refused without allowlist, got: {}",
        r.stdout
    );
}

#[tokio::test]
async fn mount_etc_blocked_without_allowlist() {
    let mut bash = Bash::builder()
        .mount_real_readonly_at("/etc", "/mnt/etc")
        .build();
    let r = bash.exec("ls /mnt/etc 2>&1; echo $?").await.unwrap();
    assert!(
        r.stdout.trim().ends_with('1') || r.stdout.contains("No such file"),
        "Mounting /etc must be refused without allowlist, got: {}",
        r.stdout
    );
}

#[tokio::test]
async fn mount_dev_blocked_without_allowlist() {
    let mut bash = Bash::builder()
        .mount_real_readonly_at("/dev", "/mnt/dev")
        .build();
    let r = bash.exec("ls /mnt/dev 2>&1; echo $?").await.unwrap();
    assert!(
        r.stdout.trim().ends_with('1') || r.stdout.contains("No such file"),
        "Mounting /dev must be refused without allowlist, got: {}",
        r.stdout
    );
}

#[tokio::test]
async fn mount_sys_blocked_without_allowlist() {
    let mut bash = Bash::builder()
        .mount_real_readonly_at("/sys", "/mnt/sys")
        .build();
    let r = bash.exec("ls /mnt/sys 2>&1; echo $?").await.unwrap();
    assert!(
        r.stdout.trim().ends_with('1') || r.stdout.contains("No such file"),
        "Mounting /sys must be refused without allowlist, got: {}",
        r.stdout
    );
}

#[cfg(unix)]
#[tokio::test]
async fn mount_secret_dir_component_blocked_without_allowlist() {
    use std::os::unix::fs::PermissionsExt;
    // Create a fake .ssh directory inside a sandbox and try to mount it.
    // The path component check must refuse it regardless of where it lives.
    let sandbox = tempfile::tempdir().unwrap();
    let secret_dir = sandbox.path().join(".ssh");
    std::fs::create_dir_all(&secret_dir).unwrap();
    let key_path = secret_dir.join("id_rsa");
    std::fs::write(&key_path, "PRIVATE KEY").unwrap();
    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let mut bash = Bash::builder()
        .mount_real_readonly_at(&secret_dir, "/mnt/keys")
        .build();
    let r = bash
        .exec("cat /mnt/keys/id_rsa 2>&1; echo $?")
        .await
        .unwrap();
    assert!(
        !r.stdout.contains("PRIVATE KEY"),
        "Mounting a path containing .ssh must be refused, got: {}",
        r.stdout
    );
}

/// THREAT[TM-FS-013]: An explicit `allowed_mount_paths` opt-in is the
/// documented escape hatch. When the embedder allowlists a sensitive path,
/// the mount succeeds.
#[cfg(unix)]
#[tokio::test]
async fn mount_secret_dir_component_allowed_via_explicit_allowlist() {
    let sandbox = tempfile::tempdir().unwrap();
    let secret_dir = sandbox.path().join(".aws");
    std::fs::create_dir_all(&secret_dir).unwrap();
    let cfg = secret_dir.join("config");
    std::fs::write(&cfg, "[default]\nregion=us-east-1\n").unwrap();

    let canonical = std::fs::canonicalize(&secret_dir).unwrap();
    let mut bash = Bash::builder()
        .allowed_mount_paths([&canonical])
        .mount_real_readonly_at(&secret_dir, "/mnt/aws")
        .build();
    let r = bash.exec("cat /mnt/aws/config 2>&1").await.unwrap();
    assert!(
        r.stdout.contains("us-east-1"),
        "Explicit allowlist must allow the mount, got: {}",
        r.stdout
    );
}

#[tokio::test]
async fn mount_allowlist_blocks_dotdot_escape() {
    let sandbox = tempfile::tempdir().unwrap();
    let allowed_root = sandbox.path().join("allowed");
    let secret_root = sandbox.path().join("secret");
    std::fs::create_dir_all(&allowed_root).unwrap();
    std::fs::create_dir_all(&secret_root).unwrap();
    std::fs::write(secret_root.join("data.txt"), "top-secret\n").unwrap();

    let escaped_mount = allowed_root.join("../secret");
    let mut bash = Bash::builder()
        .allowed_mount_paths([&allowed_root])
        .mount_real_readonly_at(&escaped_mount, "/mnt/data")
        .build();

    let r = bash
        .exec("cat /mnt/data/data.txt 2>&1; echo $?")
        .await
        .unwrap();
    assert!(
        r.stdout.trim().ends_with('1') || r.stdout.contains("No such file"),
        "Dot-dot allowlist escape should be blocked, got: {}",
        r.stdout
    );
}

#[cfg(unix)]
#[tokio::test]
async fn mount_allowlist_blocks_symlink_escape() {
    use std::os::unix::fs::symlink;

    let sandbox = tempfile::tempdir().unwrap();
    let allowed_root = sandbox.path().join("allowed");
    let secret_root = sandbox.path().join("secret");
    std::fs::create_dir_all(&allowed_root).unwrap();
    std::fs::create_dir_all(&secret_root).unwrap();
    std::fs::write(secret_root.join("data.txt"), "top-secret\n").unwrap();

    let link_path = allowed_root.join("escape_link");
    symlink(&secret_root, &link_path).unwrap();

    let mut bash = Bash::builder()
        .allowed_mount_paths([&allowed_root])
        .mount_real_readonly_at(&link_path, "/mnt/data")
        .build();

    let r = bash
        .exec("cat /mnt/data/data.txt 2>&1; echo $?")
        .await
        .unwrap();
    assert!(
        r.stdout.trim().ends_with('1') || r.stdout.contains("No such file"),
        "Symlink allowlist escape should be blocked, got: {}",
        r.stdout
    );
}

// --- Runtime mount/unmount (exercises Bash::mount / Bash::unmount) ---

#[tokio::test]
async fn runtime_mount_readonly() {
    use bashkit::{PosixFs, RealFs, RealFsMode};
    use std::sync::Arc;

    let dir = setup_host_dir();
    let mut bash = Bash::new();

    let backend = RealFs::open(dir.path(), RealFsMode::ReadOnly)
        .await
        .unwrap();
    let fs: Arc<dyn bashkit::FileSystem> = Arc::new(PosixFs::new(backend));
    bash.mount("/mnt/host", fs).unwrap();

    let result = bash.exec("cat /mnt/host/hello.txt").await.unwrap();
    assert_eq!(result.stdout, "hello world\n");
}

#[tokio::test]
async fn runtime_unmount() {
    use bashkit::{PosixFs, RealFs, RealFsMode};
    use std::sync::Arc;

    let dir = setup_host_dir();
    let mut bash = Bash::new();

    let backend = RealFs::open(dir.path(), RealFsMode::ReadOnly)
        .await
        .unwrap();
    let fs: Arc<dyn bashkit::FileSystem> = Arc::new(PosixFs::new(backend));
    bash.mount("/mnt/host", fs).unwrap();

    let result = bash.exec("cat /mnt/host/hello.txt").await.unwrap();
    assert_eq!(result.exit_code, 0);

    bash.unmount("/mnt/host").unwrap();

    let result = bash.exec("cat /mnt/host/hello.txt 2>&1").await.unwrap();
    assert_ne!(
        result.exit_code, 0,
        "file should not be accessible after unmount"
    );
}

#[tokio::test]
async fn runtime_mount_readwrite() {
    use bashkit::{PosixFs, RealFs, RealFsMode};
    use std::sync::Arc;

    let dir = setup_host_dir();
    let mut bash = Bash::new();

    let backend = RealFs::open(dir.path(), RealFsMode::ReadWrite)
        .await
        .unwrap();
    let fs: Arc<dyn bashkit::FileSystem> = Arc::new(PosixFs::new(backend));
    bash.mount("/workspace", fs).unwrap();

    bash.exec("echo 'runtime write' > /workspace/runtime.txt")
        .await
        .unwrap();

    let content = std::fs::read_to_string(dir.path().join("runtime.txt")).unwrap();
    assert_eq!(content, "runtime write\n");
}
