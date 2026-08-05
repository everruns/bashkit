//! Tests for [`HostMounts`] — mapping a VFS path back to the host path.
//!
//! The traps being pinned here are the ones an embedder hits when hand-rolling
//! this mapping: string-prefix matching that swallows sibling directories, and
//! overlapping mounts where the wrong one wins.

#![cfg(feature = "realfs")]

use bashkit::{Bash, HostMount, HostMounts};
use std::path::{Path, PathBuf};

fn mounts() -> HostMounts {
    HostMounts::new([
        HostMount {
            host_path: PathBuf::from("/srv/root"),
            vfs_path: PathBuf::from("/"),
        },
        HostMount {
            host_path: PathBuf::from("/home/user/proj"),
            vfs_path: PathBuf::from("/workspace"),
        },
    ])
}

#[test]
fn resolves_a_path_under_a_mount() {
    assert_eq!(
        mounts().resolve(Path::new("/workspace/src/main.rs")),
        Some(PathBuf::from("/home/user/proj/src/main.rs"))
    );
}

#[test]
fn resolves_the_mount_point_itself() {
    assert_eq!(
        mounts().resolve(Path::new("/workspace")),
        Some(PathBuf::from("/home/user/proj"))
    );
}

/// The trap: a plain string prefix match puts `/workspace2` inside
/// `/workspace`. Matching whole components sends it to the root mount instead.
#[test]
fn sibling_of_a_mount_does_not_land_inside_it() {
    let resolved = mounts().resolve(Path::new("/workspace2/src")).unwrap();
    assert_ne!(resolved, PathBuf::from("/home/user/proj2/src"));
    assert_ne!(resolved, PathBuf::from("/home/user/proj/src"));
    assert_eq!(resolved, PathBuf::from("/srv/root/workspace2/src"));
}

/// A specific mount nested inside a root overlay must win over the overlay.
#[test]
fn longest_matching_mount_wins() {
    assert_eq!(
        mounts().resolve(Path::new("/workspace/deep/file")),
        Some(PathBuf::from("/home/user/proj/deep/file"))
    );
    // …and a path the specific mount does not cover still falls to the root.
    assert_eq!(
        mounts().resolve(Path::new("/tmp/scratch")),
        Some(PathBuf::from("/srv/root/tmp/scratch"))
    );
}

/// VFS paths are POSIX-style on every host. On Windows a leading `/` is not
/// "absolute" — that needs a drive prefix — so a root check written as
/// `is_absolute` resolves nothing at all there, and every host-bridged command
/// loses its working directory.
#[test]
fn posix_vfs_paths_resolve_regardless_of_host_platform() {
    assert!(
        Path::new("/workspace").has_root(),
        "a VFS path is root-ed on every platform"
    );
    assert_eq!(
        mounts().resolve(Path::new("/workspace/src")),
        Some(PathBuf::from("/home/user/proj").join("src")),
    );
}

#[test]
fn relative_paths_do_not_resolve() {
    assert_eq!(mounts().resolve(Path::new("src")), None);
    assert_eq!(mounts().resolve(Path::new("../escape")), None);
}

#[test]
fn unmapped_path_is_none_when_nothing_is_mounted() {
    let empty = HostMounts::default();
    assert!(empty.is_empty());
    assert_eq!(empty.resolve(Path::new("/anywhere")), None);
}

/// Without a root overlay there is no catch-all, so an uncovered path must
/// report `None` rather than silently landing in some other mount.
#[test]
fn uncovered_path_without_a_root_mount_is_none() {
    let only_workspace = HostMounts::new([HostMount {
        host_path: PathBuf::from("/home/user/proj"),
        vfs_path: PathBuf::from("/workspace"),
    }]);
    assert_eq!(only_workspace.resolve(Path::new("/etc/passwd")), None);
}

#[test]
fn a_plain_instance_reports_no_host_mounts() {
    let bash = Bash::new();
    assert!(bash.host_mounts().is_empty());
    assert_eq!(bash.host_path_for("/anything"), None);
}

/// The table reflects what actually mounted, resolving through the real
/// builder path (including canonicalization).
#[test]
fn builder_records_real_mounts() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "hi\n").unwrap();
    let host = std::fs::canonicalize(dir.path()).unwrap();

    let bash = Bash::builder()
        .allowed_mount_paths([host.clone()])
        .mount_real_readwrite_at(host.clone(), "/workspace")
        .build();

    assert_eq!(bash.host_mounts().all().len(), 1);
    assert_eq!(
        bash.host_path_for("/workspace/hello.txt"),
        Some(host.join("hello.txt"))
    );
}

/// A mount rejected by the allowlist must not appear resolvable — reporting a
/// host path for a directory that was never mounted is worse than reporting
/// nothing.
#[test]
fn skipped_mounts_are_not_recorded() {
    let allowed = tempfile::tempdir().unwrap();
    let rejected = tempfile::tempdir().unwrap();
    let allowed_host = std::fs::canonicalize(allowed.path()).unwrap();
    let rejected_host = std::fs::canonicalize(rejected.path()).unwrap();

    let bash = Bash::builder()
        .allowed_mount_paths([allowed_host.clone()])
        .mount_real_readwrite_at(allowed_host.clone(), "/allowed")
        .mount_real_readwrite_at(rejected_host, "/rejected")
        .build();

    assert_eq!(bash.host_mounts().all().len(), 1);
    assert_eq!(bash.host_path_for("/rejected/x"), None);
    assert_eq!(
        bash.host_path_for("/allowed/x"),
        Some(allowed_host.join("x"))
    );
}
