//! Tests for custom FileSystem implementations
//!
//! These tests verify that all types needed to implement a custom FileSystem
//! are properly exported from the crate's public API.

use bashkit::{
    async_trait, Bash, DirEntry, Error, FileSystem, FileType, InMemoryFs, Metadata, Result,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

/// A minimal custom FileSystem implementation for testing.
/// This verifies that all required types are accessible from the public API.
struct MinimalFs {
    files: RwLock<HashMap<PathBuf, Vec<u8>>>,
}

impl MinimalFs {
    fn new() -> Self {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("/"), Vec::new());
        files.insert(PathBuf::from("/tmp"), Vec::new());
        files.insert(PathBuf::from("/home"), Vec::new());
        files.insert(PathBuf::from("/home/user"), Vec::new());
        Self {
            files: RwLock::new(files),
        }
    }

    fn normalize_path(path: &Path) -> PathBuf {
        let mut result = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::RootDir => result.push("/"),
                std::path::Component::Normal(name) => result.push(name),
                std::path::Component::ParentDir => {
                    result.pop();
                }
                _ => {}
            }
        }
        if result.as_os_str().is_empty() {
            result.push("/");
        }
        result
    }
}

#[async_trait]
impl FileSystem for MinimalFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let path = Self::normalize_path(path);
        let files = self.files.read().unwrap();
        files
            .get(&path)
            .cloned()
            .ok_or_else(|| Error::Io(std::io::Error::from(std::io::ErrorKind::NotFound)))
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = Self::normalize_path(path);
        let mut files = self.files.write().unwrap();
        files.insert(path, content.to_vec());
        Ok(())
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = Self::normalize_path(path);
        let mut files = self.files.write().unwrap();
        files.entry(path).or_default().extend_from_slice(content);
        Ok(())
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = Self::normalize_path(path);
        if recursive {
            let mut current = PathBuf::new();
            for component in path.components() {
                current.push(component);
                let mut files = self.files.write().unwrap();
                files.entry(current.clone()).or_default();
            }
        } else {
            let mut files = self.files.write().unwrap();
            files.insert(path, Vec::new());
        }
        Ok(())
    }

    async fn remove(&self, path: &Path, _recursive: bool) -> Result<()> {
        let path = Self::normalize_path(path);
        let mut files = self.files.write().unwrap();
        files.remove(&path);
        Ok(())
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        let path = Self::normalize_path(path);
        let files = self.files.read().unwrap();
        if let Some(content) = files.get(&path) {
            let is_dir = content.is_empty();
            Ok(Metadata {
                file_type: if is_dir {
                    FileType::Directory
                } else {
                    FileType::File
                },
                size: content.len() as u64,
                mode: 0o644,
                modified: SystemTime::now(),
                created: SystemTime::now(),
            })
        } else {
            Err(Error::Io(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )))
        }
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let path = Self::normalize_path(path);
        let files = self.files.read().unwrap();

        if !files.contains_key(&path) {
            return Err(Error::Io(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )));
        }

        let path_str = path.to_string_lossy();
        let prefix = if path_str == "/" {
            "/".to_string()
        } else {
            format!("{}/", path_str)
        };

        let mut entries = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for (file_path, content) in files.iter() {
            let file_str = file_path.to_string_lossy();
            if file_str.starts_with(&prefix) && file_path != &path {
                let remainder = &file_str[prefix.len()..];
                let name = if let Some(slash_idx) = remainder.find('/') {
                    &remainder[..slash_idx]
                } else {
                    remainder
                };
                if !name.is_empty() && seen.insert(name.to_string()) {
                    let is_dir = content.is_empty() || remainder.contains('/');
                    entries.push(DirEntry {
                        name: name.to_string(),
                        metadata: Metadata {
                            file_type: if is_dir {
                                FileType::Directory
                            } else {
                                FileType::File
                            },
                            size: content.len() as u64,
                            mode: 0o644,
                            modified: SystemTime::now(),
                            created: SystemTime::now(),
                        },
                    });
                }
            }
        }
        Ok(entries)
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        let path = Self::normalize_path(path);
        let files = self.files.read().unwrap();
        Ok(files.contains_key(&path))
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let from = Self::normalize_path(from);
        let to = Self::normalize_path(to);
        let mut files = self.files.write().unwrap();
        if let Some(content) = files.remove(&from) {
            files.insert(to, content);
            Ok(())
        } else {
            Err(Error::Io(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )))
        }
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        let from = Self::normalize_path(from);
        let to = Self::normalize_path(to);
        let files = self.files.read().unwrap();
        if let Some(content) = files.get(&from).cloned() {
            drop(files);
            let mut files = self.files.write().unwrap();
            files.insert(to, content);
            Ok(())
        } else {
            Err(Error::Io(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )))
        }
    }

    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        let link = Self::normalize_path(link);
        let mut files = self.files.write().unwrap();
        files.insert(link, format!("SYMLINK:{}", target.display()).into_bytes());
        Ok(())
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        let path = Self::normalize_path(path);
        let files = self.files.read().unwrap();
        if let Some(content) = files.get(&path) {
            let s = String::from_utf8_lossy(content);
            if let Some(target) = s.strip_prefix("SYMLINK:") {
                return Ok(PathBuf::from(target));
            }
        }
        Err(Error::Io(std::io::Error::other("not a symlink")))
    }

    async fn chmod(&self, path: &Path, _mode: u32) -> Result<()> {
        let path = Self::normalize_path(path);
        let files = self.files.read().unwrap();
        if files.contains_key(&path) {
            Ok(())
        } else {
            Err(Error::Io(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )))
        }
    }
}

#[tokio::test]
async fn test_custom_fs_exports_available() {
    // This test verifies that all required types are exported
    // If this compiles, the exports are correct
    let _: fn() -> FileType = || FileType::File;
    let _: fn() -> FileType = || FileType::Directory;
    let _: fn() -> FileType = || FileType::Symlink;

    let metadata = Metadata {
        file_type: FileType::File,
        size: 0,
        mode: 0o644,
        modified: SystemTime::now(),
        created: SystemTime::now(),
    };
    assert!(metadata.file_type.is_file());

    let entry = DirEntry {
        name: "test".to_string(),
        metadata,
    };
    assert_eq!(entry.name, "test");
}

#[tokio::test]
async fn test_custom_fs_basic_operations() {
    let fs = MinimalFs::new();

    // Write and read
    fs.write_file(Path::new("/tmp/test.txt"), b"hello")
        .await
        .unwrap();
    let content = fs.read_file(Path::new("/tmp/test.txt")).await.unwrap();
    assert_eq!(content, b"hello");

    // Exists
    assert!(fs.exists(Path::new("/tmp/test.txt")).await.unwrap());
    assert!(!fs.exists(Path::new("/tmp/nonexistent")).await.unwrap());

    // Stat
    let stat = fs.stat(Path::new("/tmp/test.txt")).await.unwrap();
    assert!(stat.file_type.is_file());
    assert_eq!(stat.size, 5);
}

#[tokio::test]
async fn test_custom_fs_integrates_with_bash() {
    let fs = Arc::new(MinimalFs::new());
    let mut bash = Bash::builder().fs(fs).build();

    // Basic echo and cat
    let result = bash
        .exec("echo hello > /tmp/test.txt && cat /tmp/test.txt")
        .await
        .unwrap();
    assert_eq!(result.stdout, "hello\n");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn test_custom_fs_pre_populated_files() {
    let fs = Arc::new(MinimalFs::new());

    // Pre-populate a file
    fs.write_file(Path::new("/tmp/config.txt"), b"debug=true")
        .await
        .unwrap();

    let mut bash = Bash::builder().fs(fs).build();

    // Bash can read pre-populated files
    let result = bash.exec("cat /tmp/config.txt").await.unwrap();
    assert_eq!(result.stdout, "debug=true");
}

#[tokio::test]
async fn test_custom_fs_live_visibility() {
    let fs = Arc::new(MinimalFs::new());
    let fs_dyn: Arc<dyn FileSystem> = Arc::clone(&fs) as Arc<dyn FileSystem>;
    let mut bash = Bash::builder().fs(fs_dyn).build();

    // Run a command
    bash.exec("echo step1 > /tmp/log.txt").await.unwrap();

    // External code writes to the filesystem
    fs.append_file(Path::new("/tmp/log.txt"), b"external\n")
        .await
        .unwrap();

    // Bash sees the external write
    let result = bash.exec("cat /tmp/log.txt").await.unwrap();
    assert!(result.stdout.contains("step1"));
    assert!(result.stdout.contains("external"));
}

#[tokio::test]
async fn test_custom_fs_directory_operations() {
    let fs = MinimalFs::new();

    // Create directories
    fs.mkdir(Path::new("/data"), false).await.unwrap();
    fs.mkdir(Path::new("/data/nested"), false).await.unwrap();

    // Create files
    fs.write_file(Path::new("/data/file1.txt"), b"1")
        .await
        .unwrap();
    fs.write_file(Path::new("/data/file2.txt"), b"2")
        .await
        .unwrap();

    // Read directory
    let entries = fs.read_dir(Path::new("/data")).await.unwrap();
    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"nested"));
    assert!(names.contains(&"file1.txt"));
    assert!(names.contains(&"file2.txt"));
}

#[tokio::test]
async fn test_custom_fs_file_type_helpers() {
    // Test FileType helper methods
    assert!(FileType::File.is_file());
    assert!(!FileType::File.is_dir());
    assert!(!FileType::File.is_symlink());

    assert!(!FileType::Directory.is_file());
    assert!(FileType::Directory.is_dir());
    assert!(!FileType::Directory.is_symlink());

    assert!(!FileType::Symlink.is_file());
    assert!(!FileType::Symlink.is_dir());
    assert!(FileType::Symlink.is_symlink());
}

#[tokio::test]
async fn test_custom_fs_can_use_builtin_overlay() {
    use bashkit::OverlayFs;

    // Custom FS as base, built-in overlay on top
    let base = Arc::new(MinimalFs::new());
    base.write_file(Path::new("/tmp/base.txt"), b"from base")
        .await
        .unwrap();

    let overlay = Arc::new(OverlayFs::new(base));
    let mut bash = Bash::builder().fs(overlay).build();

    // Read from base
    let result = bash.exec("cat /tmp/base.txt").await.unwrap();
    assert_eq!(result.stdout, "from base");

    // Write to overlay
    bash.exec("echo 'overlay write' > /tmp/overlay.txt")
        .await
        .unwrap();
    let result = bash.exec("cat /tmp/overlay.txt").await.unwrap();
    assert_eq!(result.stdout, "overlay write\n");
}

#[tokio::test]
async fn test_custom_fs_can_use_builtin_mountable() {
    use bashkit::MountableFs;

    let root = Arc::new(InMemoryFs::new());
    let custom = Arc::new(MinimalFs::new());

    custom
        .write_file(Path::new("/data.txt"), b"custom data")
        .await
        .unwrap();

    let mountable = MountableFs::new(root);
    mountable.mount("/mnt/custom", custom).unwrap();

    let mut bash = Bash::builder().fs(Arc::new(mountable)).build();

    // Access mounted custom filesystem
    let result = bash.exec("cat /mnt/custom/data.txt").await.unwrap();
    assert_eq!(result.stdout, "custom data");
}
