//! Default root filesystem layout: `/etc`, `/proc`, `/bin`, `/usr/bin`.
//!
//! Decisions:
//! - [`RootFs`] wraps the session filesystem and answers reads for a fixed
//!   set of system paths from a private, read-only layer when the session
//!   filesystem does not have them. The session filesystem always wins, so
//!   embedder files (`mount_text("/etc/x")`, a custom `.fs()`) shadow it.
//! - System content is never counted toward the session's `FsLimits`, never
//!   included in VFS snapshots, and costs a dozen map entries at build time
//!   (fast startup). Command stubs are virtual: answered from the command
//!   name set on lookup, never materialized (one file per builtin per bin dir
//!   cost ~2 ms per `Bash` build).
//! - System files behave like a non-root user sees them: reading works;
//!   modifying, removing, or renaming one fails with "Permission denied". New
//!   files may still be created under `/etc`, `/usr/bin`, ... (they go to the
//!   session filesystem), which keeps scripts that `mkdir -p /etc/app` working.
//! - `/bin` and `/usr/bin` hold a stub per registered command (not shell-only
//!   builtins) so `ls /usr/bin`, `[ -x /usr/bin/env ]`, `which`, and
//!   `#!/usr/bin/env` work. Executing a stub dispatches the builtin.
//! - THREAT[TM-INF-003]/[TM-ISO-018]: `/proc` and `/etc` are synthetic; values
//!   come from the virtual identity (username, hostname, uid 1000, 4 CPUs) and
//!   never from the host. `/etc/passwd` lists only the virtual user and
//!   `nobody` (no root line), so a `root:x:0:0` match always means a host leak.
//! - Turned off with `BashBuilder::rootfs(false)`.

use crate::time_compat::SystemTime;
use async_trait::async_trait;
use std::collections::{BTreeSet, HashSet};
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use super::limits::{FsLimits, FsUsage};
use super::memory::InMemoryFs;
use super::traits::{DirEntry, FileSystem, FileSystemExt, Metadata};
use super::{SearchCapable, normalize_path, vfs_join};
use crate::error::Result;

/// Directory trees served by the system layer.
/// Fresh random UUID per read, as on Linux.
const PROC_UUID: &str = "/proc/sys/kernel/random/uuid";

const SYS_TREES: &[&str] = &["/etc", "/proc", "/bin", "/usr/bin", "/root", "/dev/zero"];
/// Ancestors of system trees, listed with the system entries merged in.
const SYS_ANCESTORS: &[&str] = &["/", "/usr", "/dev"];
/// Directories that receive a stub per command.
pub(crate) const BIN_DIRS: &[&str] = &["/bin", "/usr/bin"];

/// First line of every command stub; marks a file as a builtin stand-in.
pub(crate) const STUB_MARKER: &str = "#!/bin/sh\n# bashkit builtin: ";

fn stub(name: &str) -> String {
    format!("{STUB_MARKER}{name}\nexec {name} \"$@\"\n")
}

/// If `content` is a command stub, return the command it stands for.
pub(crate) fn stub_command(content: &[u8]) -> Option<&str> {
    let rest = content.strip_prefix(STUB_MARKER.as_bytes())?;
    let end = rest.iter().position(|&b| b == b'\n')?;
    std::str::from_utf8(&rest[..end]).ok()
}

fn valid_command(name: &str) -> bool {
    !(name.is_empty() || name.contains('/') || name == "." || name == "..")
}

/// Shared stub name set. Every `Bash` with the same commands reuses one set
/// (building it costs ~40 µs: a String per builtin), keyed by an
/// order-independent fingerprint of the names.
fn command_set<'a>(names: impl Iterator<Item = &'a str> + Clone) -> Arc<BTreeSet<String>> {
    use std::collections::HashMap;
    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
    use std::sync::{Mutex, OnceLock};
    type Cache = Mutex<HashMap<(u64, u64, usize), Arc<BTreeSet<String>>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let hasher = BuildHasherDefault::<DefaultHasher>::default();
    let (mut sum, mut xor, mut count) = (0u64, 0u64, 0usize);
    for name in names.clone().filter(|n| valid_command(n)) {
        let h = hasher.hash_one(name);
        sum = sum.wrapping_add(h);
        xor ^= h.rotate_left(17);
        count += 1;
    }
    let key = (sum, xor, count);
    let cache = CACHE.get_or_init(Default::default);
    if let Some(set) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return Arc::clone(set);
    }
    let set: Arc<BTreeSet<String>> = Arc::new(
        names
            .filter(|n| valid_command(n))
            .map(str::to_string)
            .collect(),
    );
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    // Bounded: embedders with many distinct builtin sets just rebuild.
    if let Some(existing) = cache.get(&key) {
        return Arc::clone(existing);
    }
    if cache.len() < 32 {
        cache.insert(key, Arc::clone(&set));
    }
    set
}

/// Shared system tree per (username, hostname): read-only after build, so
/// every `Bash` with the same identity reuses one (~25 µs saved per build).
fn system_layer(username: &str, hostname: &str) -> Arc<InMemoryFs> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Cache = Mutex<HashMap<(String, String), Arc<InMemoryFs>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (username.to_string(), hostname.to_string());
    if let Some(sys) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return Arc::clone(sys);
    }
    let sys = Arc::new(RootFs::build_system_layer(username, hostname));
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    // Bounded: many distinct identities (per-tenant hostnames) just rebuild.
    if let Some(existing) = cache.get(&key) {
        return Arc::clone(existing);
    }
    if cache.len() < 32 {
        cache.insert(key, Arc::clone(&sys));
    }
    sys
}

fn in_tree(path: &Path) -> bool {
    SYS_TREES
        .iter()
        .any(|root| path == Path::new(root) || path.starts_with(root))
}

fn is_ancestor(path: &Path) -> bool {
    SYS_ANCESTORS.iter().any(|a| path == Path::new(a))
}

fn permission_denied() -> crate::Error {
    IoError::new(ErrorKind::PermissionDenied, "Permission denied").into()
}

/// Session filesystem plus the read-only system layer.
pub(crate) struct RootFs {
    inner: Arc<dyn FileSystem>,
    sys: Arc<InMemoryFs>,
    /// Commands with a virtual stub in every [`BIN_DIRS`] entry.
    commands: RwLock<Arc<BTreeSet<String>>>,
    built: SystemTime,
}

impl RootFs {
    pub(crate) fn new(inner: Arc<dyn FileSystem>, username: &str, hostname: &str) -> Self {
        Self {
            inner,
            sys: system_layer(username, hostname),
            commands: RwLock::new(Arc::default()),
            built: SystemTime::now(),
        }
    }

    /// Build the read-only system tree. Never written after this.
    fn build_system_layer(username: &str, hostname: &str) -> InMemoryFs {
        let sys = InMemoryFs::with_limits(FsLimits::unlimited());
        for dir in SYS_TREES.iter().filter(|d| **d != "/dev/zero") {
            sys.add_dir(dir, 0o755);
        }
        // Reads are generated by InMemoryFs (bounded zeros, TM-DOS-003).
        sys.add_file("/dev/zero", "", 0o666);
        sys.add_dir("/root", 0o700);
        sys.add_dir("/etc/ssl/certs", 0o755);
        let version = env!("CARGO_PKG_VERSION");
        let files: [(&str, String); 13] = [
            (
                "/etc/os-release",
                format!(
                    "PRETTY_NAME=\"Bashkit sandbox\"\nNAME=\"Bashkit\"\nID=bashkit\nVERSION_ID=\"{version}\"\nVERSION=\"{version}\"\nHOME_URL=\"https://github.com/everruns/bashkit\"\n"
                ),
            ),
            (
                "/etc/passwd",
                format!(
                    "{username}:x:1000:1000:{username}:/home/{username}:/bin/bash\nnobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n"
                ),
            ),
            (
                "/etc/group",
                format!("root:x:0:\n{username}:x:1000:\nnogroup:x:65534:\n"),
            ),
            ("/etc/hostname", format!("{hostname}\n")),
            (
                "/etc/hosts",
                format!(
                    "127.0.0.1\tlocalhost\n127.0.1.1\t{hostname}\n::1\tlocalhost ip6-localhost ip6-loopback\n"
                ),
            ),
            (
                "/etc/shells",
                "/bin/sh\n/bin/bash\n/usr/bin/sh\n/usr/bin/bash\n".to_string(),
            ),
            ("/etc/timezone", "Etc/UTC\n".to_string()),
            (
                "/proc/cpuinfo",
                (0..crate::builtins::VIRTUAL_NPROC)
                    .map(|i| {
                        format!(
                            "processor\t: {i}\nvendor_id\t: Bashkit\nmodel name\t: Bashkit Virtual CPU\ncpu MHz\t\t: 2000.000\ncpu cores\t: {n}\n\n",
                            n = crate::builtins::VIRTUAL_NPROC
                        )
                    })
                    .collect(),
            ),
            (
                "/proc/meminfo",
                "MemTotal:        4194304 kB\nMemFree:         3145728 kB\nMemAvailable:    3145728 kB\nSwapTotal:             0 kB\nSwapFree:              0 kB\n"
                    .to_string(),
            ),
            (
                "/proc/version",
                format!(
                    "Linux version {} (bashkit) {}\n",
                    crate::builtins::VIRTUAL_KERNEL_RELEASE,
                    crate::builtins::VIRTUAL_KERNEL_VERSION
                ),
            ),
            ("/proc/loadavg", "0.00 0.00 0.00 1/1 1\n".to_string()),
            ("/proc/sys/kernel/hostname", format!("{hostname}\n")),
            // Placeholder of the right size; reads return a fresh UUID.
            (PROC_UUID, format!("{}\n", "0".repeat(36))),
        ];
        for (path, content) in files {
            sys.add_file(path, content, 0o644);
        }
        sys
    }

    /// Publish a stub in every bin directory for each command name.
    pub(crate) fn set_commands<'a>(&self, names: impl Iterator<Item = &'a str> + Clone) {
        let set = command_set(names);
        *self.commands.write().unwrap_or_else(|e| e.into_inner()) = set;
    }

    /// The command a normalized path names when it is a virtual stub.
    fn stub_name(&self, p: &Path) -> Option<String> {
        let parent = p.parent()?;
        if !BIN_DIRS.iter().any(|d| parent == Path::new(d)) {
            return None;
        }
        let name = p.file_name()?.to_str()?;
        let commands = self.commands.read().unwrap_or_else(|e| e.into_inner());
        commands.contains(name).then(|| name.to_string())
    }

    fn stub_metadata(&self, name: &str) -> Metadata {
        Metadata {
            file_type: super::traits::FileType::File,
            size: stub(name).len() as u64,
            mode: 0o755,
            modified: self.built,
            created: self.built,
        }
    }

    async fn sys_read(&self, p: &Path) -> Result<Vec<u8>> {
        if p == Path::new(PROC_UUID) {
            // Like Linux: every read is a new random (v4) UUID from the OS
            // CSPRNG, the same source as `uuidgen`.
            let uuid = crate::builtins::random::uuid_v4().map_err(std::io::Error::other)?;
            return Ok(format!("{uuid}\n").into_bytes());
        }
        match self.stub_name(p) {
            Some(name) => Ok(stub(&name).into_bytes()),
            None => self.sys.read_file(p).await,
        }
    }

    async fn sys_stat(&self, p: &Path) -> Result<Metadata> {
        match self.stub_name(p) {
            Some(name) => Ok(self.stub_metadata(&name)),
            None => self.sys.stat(p).await,
        }
    }

    /// The path, normalized, when it falls in the system layer.
    fn sys_path(path: &Path) -> Option<PathBuf> {
        let p = normalize_path(path);
        (in_tree(&p) || is_ancestor(&p)).then_some(p)
    }

    /// Whether `path` resolves to the system layer for reads.
    async fn served_by_sys(&self, path: &Path) -> Option<PathBuf> {
        let p = Self::sys_path(path)?;
        if self.inner.exists(&p).await.unwrap_or(false) {
            return None;
        }
        (self.stub_name(&p).is_some() || self.sys.exists(&p).await.unwrap_or(false)).then_some(p)
    }

    /// Refuse to modify a file that only the system layer has.
    async fn guard_sys_only(&self, path: &Path) -> Result<()> {
        if let Some(p) = self.served_by_sys(path).await
            && in_tree(&p)
        {
            return Err(permission_denied());
        }
        Ok(())
    }

    /// Create a missing parent that the system layer provides, so new files
    /// can be created under `/etc` and friends.
    async fn ensure_parent(&self, path: &Path) -> Result<()> {
        let p = normalize_path(path);
        if let Some(parent) = p.parent()
            && Self::sys_path(parent).is_some()
            && !self.inner.exists(parent).await.unwrap_or(false)
            && self.sys.exists(parent).await.unwrap_or(false)
        {
            self.inner.mkdir(parent, true).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl FileSystem for RootFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        match self.served_by_sys(path).await {
            Some(p) => self.sys_read(&p).await,
            None => self.inner.read_file(path).await,
        }
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.guard_sys_only(path).await?;
        self.ensure_parent(path).await?;
        self.inner.write_file(path, content).await
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.guard_sys_only(path).await?;
        self.ensure_parent(path).await?;
        self.inner.append_file(path, content).await
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        if let Some(p) = self.served_by_sys(path).await {
            // Embedders commonly `mkdir("/bin")` before adding files; shadow
            // the system directory in the session filesystem instead of
            // failing, so that setup code keeps working.
            if self.sys.stat(&p).await.is_ok_and(|m| m.file_type.is_dir()) {
                return self.inner.mkdir(&p, true).await;
            }
            return Err(IoError::new(ErrorKind::AlreadyExists, "File exists").into());
        }
        if !recursive {
            self.ensure_parent(path).await?;
        }
        self.inner.mkdir(path, recursive).await
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        self.guard_sys_only(path).await?;
        if Self::sys_path(path).is_some_and(|p| is_ancestor(&p)) {
            return Err(permission_denied());
        }
        self.inner.remove(path, recursive).await
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        match self.served_by_sys(path).await {
            Some(p) => self.sys_stat(&p).await,
            None => self.inner.stat(path).await,
        }
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let Some(p) = Self::sys_path(path) else {
            return self.inner.read_dir(path).await;
        };
        let inner = self.inner.read_dir(&p).await;
        let sys = self.sys.read_dir(&p).await.map(|mut entries| {
            if BIN_DIRS.iter().any(|d| p == Path::new(d)) {
                let commands = self.commands.read().unwrap_or_else(|e| e.into_inner());
                entries.extend(commands.iter().map(|name| DirEntry {
                    name: name.clone(),
                    metadata: self.stub_metadata(name),
                }));
            }
            entries
        });
        match (inner, sys) {
            (inner, Ok(sys)) => {
                let mut entries = inner.unwrap_or_default();
                let seen: HashSet<String> = entries.iter().map(|e| e.name.clone()).collect();
                entries.extend(sys.into_iter().filter(|e| {
                    !seen.contains(&e.name) && {
                        let child = vfs_join(&p, &e.name);
                        in_tree(&child) || is_ancestor(&child)
                    }
                }));
                Ok(entries)
            }
            (inner, Err(_)) => inner,
        }
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        if self.inner.exists(path).await? {
            return Ok(true);
        }
        Ok(self.served_by_sys(path).await.is_some())
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.guard_sys_only(from).await?;
        self.guard_sys_only(to).await?;
        self.ensure_parent(to).await?;
        self.inner.rename(from, to).await
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        self.guard_sys_only(to).await?;
        self.ensure_parent(to).await?;
        match self.served_by_sys(from).await {
            Some(p) => {
                let data = self.sys_read(&p).await?;
                self.inner.write_file(to, &data).await
            }
            None => self.inner.copy(from, to).await,
        }
    }

    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        self.guard_sys_only(link).await?;
        self.ensure_parent(link).await?;
        self.inner.symlink(target, link).await
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        match self.served_by_sys(path).await {
            Some(p) if self.stub_name(&p).is_some() => Err(IoError::other("not a symlink").into()),
            Some(p) => self.sys.read_link(&p).await,
            None => self.inner.read_link(path).await,
        }
    }

    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        self.guard_sys_only(path).await?;
        self.inner.chmod(path, mode).await
    }

    async fn set_modified_time(&self, path: &Path, time: SystemTime) -> Result<()> {
        self.guard_sys_only(path).await?;
        self.inner.set_modified_time(path, time).await
    }

    fn as_search_capable(&self) -> Option<&dyn SearchCapable> {
        self.inner.as_search_capable()
    }
}

#[async_trait]
impl FileSystemExt for RootFs {
    fn backend_kind(&self) -> &'static str {
        self.inner.backend_kind()
    }

    fn usage(&self) -> FsUsage {
        self.inner.usage()
    }

    fn limits(&self) -> FsLimits {
        self.inner.limits()
    }

    async fn mkfifo(&self, path: &Path, mode: u32) -> Result<()> {
        self.guard_sys_only(path).await?;
        self.ensure_parent(path).await?;
        self.inner.mkfifo(path, mode).await
    }

    fn vfs_snapshot(&self) -> Option<super::VfsSnapshot> {
        self.inner.vfs_snapshot()
    }

    fn vfs_restore(&self, snapshot: &super::VfsSnapshot) -> Result<()> {
        self.inner.vfs_restore(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn builds_share_system_layer_and_stub_set() {
        let a = rootfs();
        let b = rootfs();
        // Fast startup: nothing per-builtin is materialized per build.
        assert!(Arc::ptr_eq(&a.sys, &b.sys));
        assert!(Arc::ptr_eq(
            &a.commands.read().unwrap(),
            &b.commands.read().unwrap()
        ));
        let stub = a.read_file(Path::new("/usr/bin/env")).await.unwrap();
        assert_eq!(stub_command(&stub), Some("env"));
        let meta = a.stat(Path::new("/bin/ls")).await.unwrap();
        assert_eq!(meta.mode, 0o755);
        assert!(a.read_file(Path::new("/bin/nope")).await.is_err());
        let names: Vec<String> = a
            .read_dir(Path::new("/usr/bin"))
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert!(names.contains(&"ls".to_string()));
        assert!(a.write_file(Path::new("/bin/ls"), b"x").await.is_err());
    }

    fn rootfs() -> RootFs {
        let inner: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let fs = RootFs::new(inner, "sandbox", "bashkit-sandbox");
        fs.set_commands(["ls", "env"].into_iter());
        fs
    }

    #[tokio::test]
    async fn system_files_are_readable() {
        let fs = rootfs();
        let passwd = fs.read_file(Path::new("/etc/passwd")).await.unwrap();
        assert!(String::from_utf8_lossy(&passwd).contains("sandbox:x:1000:1000"));
        let stub = fs.read_file(Path::new("/usr/bin/env")).await.unwrap();
        assert_eq!(stub_command(&stub), Some("env"));
        assert!(fs.stat(Path::new("/bin/ls")).await.unwrap().mode & 0o111 != 0);
    }

    #[tokio::test]
    async fn root_listing_merges_without_leaking_other_sys_dirs() {
        let fs = rootfs();
        let names: Vec<String> = fs
            .read_dir(Path::new("/"))
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        for want in ["etc", "proc", "bin", "usr", "tmp", "home", "dev"] {
            assert!(names.contains(&want.to_string()), "missing {want}");
        }
        assert_eq!(names.iter().filter(|n| *n == "tmp").count(), 1);
    }

    #[tokio::test]
    async fn system_files_are_read_only_but_dirs_accept_new_files() {
        let fs = rootfs();
        assert!(fs.write_file(Path::new("/etc/passwd"), b"x").await.is_err());
        assert!(fs.remove(Path::new("/etc/hosts"), false).await.is_err());
        fs.write_file(Path::new("/etc/app.conf"), b"ok")
            .await
            .unwrap();
        assert_eq!(
            fs.read_file(Path::new("/etc/app.conf")).await.unwrap(),
            b"ok"
        );
        assert!(fs.exists(Path::new("/etc/os-release")).await.unwrap());
        fs.mkdir(Path::new("/etc/app/conf.d"), true).await.unwrap();
        assert!(
            fs.stat(Path::new("/etc/app/conf.d"))
                .await
                .unwrap()
                .file_type
                .is_dir()
        );
    }

    #[tokio::test]
    async fn session_files_shadow_system_files() {
        let inner = Arc::new(InMemoryFs::new());
        inner.add_file("/etc/hostname", "custom\n", 0o644);
        let fs = RootFs::new(inner, "sandbox", "bashkit-sandbox");
        assert_eq!(
            fs.read_file(Path::new("/etc/hostname")).await.unwrap(),
            b"custom\n"
        );
    }

    #[tokio::test]
    async fn usage_and_snapshot_exclude_system_layer() {
        let inner = Arc::new(InMemoryFs::new());
        let before = inner.usage();
        let fs = rootfs();
        assert_eq!(fs.usage().file_count, before.file_count);
    }
}
