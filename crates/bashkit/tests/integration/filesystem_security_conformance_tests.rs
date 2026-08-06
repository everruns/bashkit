//! Shared security certification for production `FileSystem` implementations.

use async_trait::async_trait;
use bashkit::{
    Bash, DirEntry, Error, FileSystem, FileSystemExt, FsLimits, InMemoryFs, Metadata, MountableFs,
    NamespaceFs, OverlayFs, ReadOnlyFs,
};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[path = "../support/filesystem_security_conformance.rs"]
mod shared;

fn writable_adapters() -> Vec<(&'static str, Arc<dyn FileSystem>)> {
    let memory = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;

    let lower = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
    let overlay = Arc::new(OverlayFs::new(lower)) as Arc<dyn FileSystem>;

    let mountable = Arc::new(MountableFs::new(Arc::new(InMemoryFs::new()))) as Arc<dyn FileSystem>;

    let namespace_source = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
    let namespace = Arc::new(
        NamespaceFs::builder()
            .mount_readwrite("/", namespace_source)
            .unwrap()
            .build(),
    ) as Arc<dyn FileSystem>;

    vec![
        ("memory", memory),
        ("overlay", overlay),
        ("mountable", mountable),
        ("namespace", namespace),
    ]
}

fn io_kind(error: Error) -> ErrorKind {
    match error {
        Error::Io(error) => error.kind(),
        other => panic!("expected I/O error, got {other}"),
    }
}

#[tokio::test]
async fn shared_contract_certifies_all_virtual_writable_adapters() {
    for (name, fs) in writable_adapters() {
        shared::certify_path_and_data_contract(name, fs.as_ref()).await;
    }
}

#[tokio::test]
async fn writable_adapters_do_not_follow_virtual_symlinks() {
    for (name, fs) in writable_adapters() {
        fs.mkdir(Path::new("/cert"), false).await.unwrap();
        fs.write_file(Path::new("/cert/secret"), b"secret")
            .await
            .unwrap();
        fs.symlink(Path::new("/cert/secret"), Path::new("/cert/link"))
            .await
            .unwrap();

        assert!(
            fs.stat(Path::new("/cert/link"))
                .await
                .unwrap()
                .file_type
                .is_symlink(),
            "{name}"
        );
        assert_eq!(
            io_kind(fs.read_file(Path::new("/cert/link")).await.unwrap_err()),
            ErrorKind::NotFound,
            "{name}"
        );
        assert_eq!(
            fs.read_link(Path::new("/cert/link")).await.unwrap(),
            PathBuf::from("/cert/secret"),
            "{name}"
        );
    }
}

#[tokio::test]
async fn writable_adapters_leave_state_and_quota_unchanged_after_failed_mutations() {
    for (name, fs) in writable_adapters() {
        fs.mkdir(Path::new("/cert"), false).await.unwrap();
        fs.write_file(Path::new("/cert/source"), b"source")
            .await
            .unwrap();
        fs.write_file(Path::new("/cert/destination"), b"destination")
            .await
            .unwrap();
        let before = fs.usage();

        assert!(
            fs.copy(Path::new("/cert/missing"), Path::new("/cert/destination"))
                .await
                .is_err()
        );
        assert_eq!(
            fs.read_file(Path::new("/cert/destination")).await.unwrap(),
            b"destination",
            "{name}"
        );

        assert!(
            fs.rename(Path::new("/cert/source"), Path::new("/missing/destination"))
                .await
                .is_err(),
            "{name}"
        );
        assert_eq!(
            fs.read_file(Path::new("/cert/source")).await.unwrap(),
            b"source",
            "{name}"
        );
        assert!(
            !fs.exists(Path::new("/missing/destination")).await.unwrap(),
            "{name}"
        );
        let after = fs.usage();
        assert_eq!(after.total_bytes, before.total_bytes, "{name}");
        assert_eq!(after.file_count, before.file_count, "{name}");
        assert_eq!(after.dir_count, before.dir_count, "{name}");
    }
}

#[tokio::test]
async fn quota_rejection_retains_content_and_accounted_usage() {
    let limits = FsLimits::new().max_total_bytes(8).max_file_size(8);
    let memory = Arc::new(InMemoryFs::with_limits(limits.clone())) as Arc<dyn FileSystem>;
    let overlay = Arc::new(OverlayFs::with_limits(Arc::new(InMemoryFs::new()), limits))
        as Arc<dyn FileSystem>;

    for (name, fs) in [("memory", memory), ("overlay", overlay)] {
        fs.write_file(Path::new("/tmp/data"), b"123456")
            .await
            .unwrap();
        let before = fs.usage();
        assert!(
            fs.write_file(Path::new("/tmp/data"), b"123456789")
                .await
                .is_err(),
            "{name}"
        );
        assert_eq!(
            fs.read_file(Path::new("/tmp/data")).await.unwrap(),
            b"123456",
            "{name}"
        );
        let after = fs.usage();
        assert_eq!(after.total_bytes, before.total_bytes, "{name}");
        assert_eq!(after.file_count, before.file_count, "{name}");
    }
}

#[tokio::test]
async fn failed_type_conflicts_do_not_replace_or_merge_entries() {
    for (name, fs) in writable_adapters() {
        fs.mkdir(Path::new("/source-dir"), false).await.unwrap();
        fs.write_file(Path::new("/source-dir/source"), b"source")
            .await
            .unwrap();
        fs.mkdir(Path::new("/destination-dir"), false)
            .await
            .unwrap();
        fs.write_file(Path::new("/destination-dir/destination"), b"destination")
            .await
            .unwrap();

        assert!(
            fs.rename(Path::new("/source-dir"), Path::new("/destination-dir"))
                .await
                .is_err(),
            "{name}"
        );
        assert!(
            fs.copy(
                Path::new("/source-dir/source"),
                Path::new("/destination-dir")
            )
            .await
            .is_err(),
            "{name}"
        );
        assert_eq!(
            fs.read_file(Path::new("/source-dir/source")).await.unwrap(),
            b"source",
            "{name}"
        );
        assert_eq!(
            fs.read_file(Path::new("/destination-dir/destination"))
                .await
                .unwrap(),
            b"destination",
            "{name}"
        );
        assert!(
            fs.stat(Path::new("/destination-dir"))
                .await
                .unwrap()
                .file_type
                .is_dir(),
            "{name}"
        );
    }
}

#[tokio::test]
async fn directory_rename_moves_the_complete_subtree() {
    for (name, fs) in writable_adapters() {
        fs.mkdir(Path::new("/tree/nested"), true).await.unwrap();
        fs.write_file(Path::new("/tree/nested/data"), b"data")
            .await
            .unwrap();

        fs.rename(Path::new("/tree"), Path::new("/moved"))
            .await
            .unwrap_or_else(|error| panic!("{name}: {error}"));

        assert!(!fs.exists(Path::new("/tree")).await.unwrap(), "{name}");
        assert_eq!(
            fs.read_file(Path::new("/moved/nested/data")).await.unwrap(),
            b"data",
            "{name}"
        );
    }
}

#[tokio::test]
async fn readonly_wrapper_certifies_reads_and_denies_every_mutation() {
    let inner = Arc::new(InMemoryFs::new());
    inner
        .write_file(Path::new("/tmp/data"), b"data")
        .await
        .unwrap();
    let fs = ReadOnlyFs::new(inner);
    assert_eq!(fs.read_file(Path::new("/tmp/data")).await.unwrap(), b"data");

    let mutations = [
        fs.write_file(Path::new("/tmp/data"), b"x").await,
        fs.append_file(Path::new("/tmp/data"), b"x").await,
        fs.mkdir(Path::new("/tmp/new"), false).await,
        fs.remove(Path::new("/tmp/data"), false).await,
        fs.copy(Path::new("/tmp/data"), Path::new("/tmp/copy"))
            .await,
        fs.rename(Path::new("/tmp/data"), Path::new("/tmp/moved"))
            .await,
        fs.symlink(Path::new("data"), Path::new("/tmp/link")).await,
        fs.chmod(Path::new("/tmp/data"), 0o600).await,
    ];
    for result in mutations {
        assert_eq!(io_kind(result.unwrap_err()), ErrorKind::PermissionDenied);
    }
    assert_eq!(fs.read_file(Path::new("/tmp/data")).await.unwrap(), b"data");
}

struct FailRemoveFs {
    inner: Arc<dyn FileSystem>,
}

#[async_trait]
impl FileSystemExt for FailRemoveFs {
    fn usage(&self) -> bashkit::FsUsage {
        self.inner.usage()
    }

    fn limits(&self) -> FsLimits {
        self.inner.limits()
    }
}

#[async_trait]
impl FileSystem for FailRemoveFs {
    async fn read_file(&self, path: &Path) -> bashkit::Result<Vec<u8>> {
        self.inner.read_file(path).await
    }
    async fn write_file(&self, path: &Path, content: &[u8]) -> bashkit::Result<()> {
        self.inner.write_file(path, content).await
    }
    async fn append_file(&self, path: &Path, content: &[u8]) -> bashkit::Result<()> {
        self.inner.append_file(path, content).await
    }
    async fn mkdir(&self, path: &Path, recursive: bool) -> bashkit::Result<()> {
        self.inner.mkdir(path, recursive).await
    }
    async fn remove(&self, _path: &Path, _recursive: bool) -> bashkit::Result<()> {
        Err(std::io::Error::other("injected remove failure").into())
    }
    async fn stat(&self, path: &Path) -> bashkit::Result<Metadata> {
        self.inner.stat(path).await
    }
    async fn read_dir(&self, path: &Path) -> bashkit::Result<Vec<DirEntry>> {
        self.inner.read_dir(path).await
    }
    async fn exists(&self, path: &Path) -> bashkit::Result<bool> {
        self.inner.exists(path).await
    }
    async fn rename(&self, from: &Path, to: &Path) -> bashkit::Result<()> {
        self.inner.rename(from, to).await
    }
    async fn copy(&self, from: &Path, to: &Path) -> bashkit::Result<()> {
        self.inner.copy(from, to).await
    }
    async fn symlink(&self, target: &Path, link: &Path) -> bashkit::Result<()> {
        self.inner.symlink(target, link).await
    }
    async fn read_link(&self, path: &Path) -> bashkit::Result<PathBuf> {
        self.inner.read_link(path).await
    }
    async fn chmod(&self, path: &Path, mode: u32) -> bashkit::Result<()> {
        self.inner.chmod(path, mode).await
    }
}

#[tokio::test]
async fn mountable_cross_mount_move_rolls_back_destination_on_source_failure() {
    let source = Arc::new(InMemoryFs::new());
    source
        .write_file(Path::new("/tmp/source"), b"source")
        .await
        .unwrap();
    let source: Arc<dyn FileSystem> = Arc::new(FailRemoveFs { inner: source });
    let destination = Arc::new(InMemoryFs::new());
    destination
        .write_file(Path::new("/tmp/moved"), b"original destination")
        .await
        .unwrap();
    let fs = MountableFs::new(source.clone());
    fs.mount("/destination", destination.clone()).unwrap();

    assert!(
        fs.rename(
            Path::new("/tmp/source"),
            Path::new("/destination/tmp/moved")
        )
        .await
        .is_err()
    );
    assert!(source.exists(Path::new("/tmp/source")).await.unwrap());
    assert_eq!(
        destination
            .read_file(Path::new("/tmp/moved"))
            .await
            .unwrap(),
        b"original destination"
    );
}

fn append_tar_entry(archive: &mut Vec<u8>, name: &str, content: &[u8]) {
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    let size = format!("{:011o}\0", content.len());
    header[124..136].copy_from_slice(size.as_bytes());
    header[156] = b'0';
    archive.extend_from_slice(&header);
    archive.extend_from_slice(content);
    archive.resize(archive.len().div_ceil(512) * 512, 0);
}

#[tokio::test]
async fn tar_extract_rejects_late_unsafe_entry_without_partial_changes() {
    let fs = Arc::new(InMemoryFs::new());
    let mut archive = Vec::new();
    append_tar_entry(&mut archive, "safe.txt", b"safe");
    append_tar_entry(&mut archive, "../escape.txt", b"escape");
    archive.resize(archive.len() + 1024, 0);
    fs.write_file(Path::new("/tmp/archive.tar"), &archive)
        .await
        .unwrap();

    let mut bash = Bash::builder().fs(fs.clone()).build();
    let result = bash.exec("cd /tmp && tar -xf archive.tar").await.unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(!fs.exists(Path::new("/tmp/safe.txt")).await.unwrap());
    assert!(!fs.exists(Path::new("/escape.txt")).await.unwrap());
}

#[tokio::test]
async fn tar_extract_preflights_aggregate_quota_without_partial_changes() {
    let lower = Arc::new(InMemoryFs::new());
    let mut archive = Vec::new();
    append_tar_entry(&mut archive, "first.txt", b"1234");
    append_tar_entry(&mut archive, "second.txt", b"5678");
    archive.resize(archive.len() + 1024, 0);
    lower
        .write_file(Path::new("/tmp/archive.tar"), &archive)
        .await
        .unwrap();
    let limits = FsLimits::new()
        .max_file_size(10)
        .max_total_bytes(archive.len() as u64 + 6);
    let fs = Arc::new(OverlayFs::with_limits(lower, limits));

    let mut bash = Bash::builder().fs(fs.clone()).build();
    let result = bash.exec("cd /tmp && tar -xf archive.tar").await.unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(!fs.exists(Path::new("/tmp/first.txt")).await.unwrap());
    assert!(!fs.exists(Path::new("/tmp/second.txt")).await.unwrap());
}
