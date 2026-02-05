//! Custom FsBackend implementation example
//!
//! Demonstrates how to implement a simple storage backend using `FsBackend`
//! and wrap it with `PosixFs` to get POSIX-like semantics automatically.
//!
//! This is the **recommended** approach for custom filesystems because:
//! - You only implement raw storage operations
//! - POSIX semantics (type checking, parent directories) are handled by `PosixFs`
//! - Less code, fewer bugs
//!
//! Run with: cargo run --example custom_backend

use bashkit::{async_trait, Bash, DirEntry, FileType, FsBackend, Metadata, PosixFs, Result};
use std::collections::HashMap;
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::SystemTime;

/// Entry in our simple storage backend.
#[derive(Clone)]
enum StorageEntry {
    File(Vec<u8>),
    Directory,
}

/// A simple in-memory storage backend.
///
/// This implements `FsBackend` - just raw storage operations.
/// No POSIX semantics checking needed here.
pub struct SimpleStorage {
    entries: RwLock<HashMap<PathBuf, StorageEntry>>,
}

impl SimpleStorage {
    pub fn new() -> Self {
        let mut entries = HashMap::new();
        // Initialize with root and common directories
        entries.insert(PathBuf::from("/"), StorageEntry::Directory);
        entries.insert(PathBuf::from("/tmp"), StorageEntry::Directory);
        entries.insert(PathBuf::from("/home"), StorageEntry::Directory);
        entries.insert(PathBuf::from("/home/user"), StorageEntry::Directory);

        Self {
            entries: RwLock::new(entries),
        }
    }

    fn normalize(path: &Path) -> PathBuf {
        let mut result = PathBuf::from("/");
        for component in path.components() {
            match component {
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

impl Default for SimpleStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FsBackend for SimpleStorage {
    async fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let path = Self::normalize(path);
        let entries = self.entries.read().unwrap();

        match entries.get(&path) {
            Some(StorageEntry::File(content)) => Ok(content.clone()),
            Some(StorageEntry::Directory) => Err(IoError::other("is a directory").into()),
            None => Err(IoError::new(ErrorKind::NotFound, "file not found").into()),
        }
    }

    async fn write(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = Self::normalize(path);
        let mut entries = self.entries.write().unwrap();
        entries.insert(path, StorageEntry::File(content.to_vec()));
        Ok(())
    }

    async fn append(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = Self::normalize(path);
        let mut entries = self.entries.write().unwrap();

        match entries.get_mut(&path) {
            Some(StorageEntry::File(existing)) => {
                existing.extend_from_slice(content);
                Ok(())
            }
            Some(StorageEntry::Directory) => Err(IoError::other("is a directory").into()),
            None => {
                entries.insert(path, StorageEntry::File(content.to_vec()));
                Ok(())
            }
        }
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = Self::normalize(path);

        if recursive {
            let mut current = PathBuf::from("/");
            let mut entries = self.entries.write().unwrap();
            for component in path.components().skip(1) {
                current.push(component);
                entries
                    .entry(current.clone())
                    .or_insert(StorageEntry::Directory);
            }
        } else {
            let mut entries = self.entries.write().unwrap();
            if entries.contains_key(&path) {
                return Err(IoError::new(ErrorKind::AlreadyExists, "already exists").into());
            }
            entries.insert(path, StorageEntry::Directory);
        }
        Ok(())
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = Self::normalize(path);
        let mut entries = self.entries.write().unwrap();

        if !entries.contains_key(&path) {
            return Err(IoError::new(ErrorKind::NotFound, "not found").into());
        }

        if recursive {
            let path_str = path.to_string_lossy().to_string();
            let prefix = if path_str == "/" {
                "/".to_string()
            } else {
                format!("{}/", path_str)
            };
            let to_remove: Vec<_> = entries
                .keys()
                .filter(|k| {
                    let k_str = k.to_string_lossy();
                    k_str.starts_with(&prefix) || *k == &path
                })
                .cloned()
                .collect();
            for key in to_remove {
                entries.remove(&key);
            }
        } else {
            entries.remove(&path);
        }
        Ok(())
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        let path = Self::normalize(path);
        let entries = self.entries.read().unwrap();

        match entries.get(&path) {
            Some(StorageEntry::File(content)) => Ok(Metadata {
                file_type: FileType::File,
                size: content.len() as u64,
                mode: 0o644,
                modified: SystemTime::now(),
                created: SystemTime::now(),
            }),
            Some(StorageEntry::Directory) => Ok(Metadata {
                file_type: FileType::Directory,
                size: 0,
                mode: 0o755,
                modified: SystemTime::now(),
                created: SystemTime::now(),
            }),
            None => Err(IoError::new(ErrorKind::NotFound, "not found").into()),
        }
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let path = Self::normalize(path);
        let entries = self.entries.read().unwrap();

        if !entries.contains_key(&path) {
            return Err(IoError::new(ErrorKind::NotFound, "not found").into());
        }

        let path_str = path.to_string_lossy().to_string();
        let prefix = if path_str == "/" {
            "/".to_string()
        } else {
            format!("{}/", path_str)
        };

        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for (entry_path, entry) in entries.iter() {
            let entry_str = entry_path.to_string_lossy();
            if entry_str.starts_with(&prefix) && entry_path != &path {
                let remainder = &entry_str[prefix.len()..];
                let name = remainder.split('/').next().unwrap_or("");
                if !name.is_empty() && seen.insert(name.to_string()) {
                    let (file_type, size) = match entry {
                        StorageEntry::File(content) => (FileType::File, content.len() as u64),
                        StorageEntry::Directory => (FileType::Directory, 0),
                    };
                    result.push(DirEntry {
                        name: name.to_string(),
                        metadata: Metadata {
                            file_type,
                            size,
                            mode: 0o644,
                            modified: SystemTime::now(),
                            created: SystemTime::now(),
                        },
                    });
                }
            }
        }

        result.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(result)
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        let path = Self::normalize(path);
        let entries = self.entries.read().unwrap();
        Ok(entries.contains_key(&path))
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let from = Self::normalize(from);
        let to = Self::normalize(to);
        let mut entries = self.entries.write().unwrap();

        if let Some(entry) = entries.remove(&from) {
            entries.insert(to, entry);
            Ok(())
        } else {
            Err(IoError::new(ErrorKind::NotFound, "source not found").into())
        }
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        let from = Self::normalize(from);
        let to = Self::normalize(to);
        let entries = self.entries.read().unwrap();

        if let Some(entry) = entries.get(&from) {
            let entry = entry.clone();
            drop(entries);
            let mut entries = self.entries.write().unwrap();
            entries.insert(to, entry);
            Ok(())
        } else {
            Err(IoError::new(ErrorKind::NotFound, "source not found").into())
        }
    }

    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        // Simplified: store as special file
        let link = Self::normalize(link);
        let content = format!("SYMLINK:{}", target.display()).into_bytes();
        let mut entries = self.entries.write().unwrap();
        entries.insert(link, StorageEntry::File(content));
        Ok(())
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        let path = Self::normalize(path);
        let entries = self.entries.read().unwrap();

        if let Some(StorageEntry::File(content)) = entries.get(&path) {
            let content_str = String::from_utf8_lossy(content);
            if let Some(target) = content_str.strip_prefix("SYMLINK:") {
                return Ok(PathBuf::from(target));
            }
        }
        Err(IoError::other("not a symbolic link").into())
    }

    async fn chmod(&self, path: &Path, _mode: u32) -> Result<()> {
        let path = Self::normalize(path);
        let entries = self.entries.read().unwrap();
        if entries.contains_key(&path) {
            Ok(())
        } else {
            Err(IoError::new(ErrorKind::NotFound, "not found").into())
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== FsBackend + PosixFs Example ===\n");

    // Step 1: Create your simple storage backend
    let backend = SimpleStorage::new();

    // Step 2: Wrap with PosixFs to get POSIX semantics
    let fs = std::sync::Arc::new(PosixFs::new(backend));

    // Step 3: Use with Bash
    let mut bash = Bash::builder().fs(fs.clone()).build();

    println!("1. Basic file operations:");
    bash.exec("echo 'Hello, World!' > /tmp/hello.txt").await?;
    let result = bash.exec("cat /tmp/hello.txt").await?;
    println!("   {}", result.stdout.trim());

    println!("\n2. POSIX semantics are enforced by PosixFs:");

    // Try to write to a directory (should fail)
    bash.exec("mkdir -p /tmp/mydir").await?;
    let result = bash
        .exec("echo test > /tmp/mydir 2>/dev/null; echo $?")
        .await?;
    println!("   Write to directory: exit code {}", result.stdout.trim());

    // Try mkdir on existing file (should fail)
    bash.exec("echo test > /tmp/myfile").await?;
    let result = bash.exec("mkdir /tmp/myfile 2>/dev/null; echo $?").await?;
    println!("   mkdir on file: exit code {}", result.stdout.trim());

    println!("\n3. The backend doesn't need POSIX checks - PosixFs handles them!");
    println!("   This makes implementing custom backends much simpler.");

    println!("\n=== Complete ===");
    println!("\nCompare this to custom_filesystem_impl.rs which implements");
    println!("FileSystem directly with all POSIX checks inline.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bashkit::FileSystem;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_posix_wrapper_type_checks() {
        let backend = SimpleStorage::new();
        let fs = PosixFs::new(backend);

        // Create a directory
        fs.mkdir(Path::new("/tmp/testdir"), false).await.unwrap();

        // Try to write to directory - PosixFs should prevent this
        let result = fs.write_file(Path::new("/tmp/testdir"), b"test").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("directory"));
    }

    #[tokio::test]
    async fn test_posix_wrapper_mkdir_on_file() {
        let backend = SimpleStorage::new();
        let fs = PosixFs::new(backend);

        // Create a file
        fs.write_file(Path::new("/tmp/testfile"), b"test")
            .await
            .unwrap();

        // Try to mkdir on file - PosixFs should prevent this
        let result = fs.mkdir(Path::new("/tmp/testfile"), false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_with_bash() {
        let backend = SimpleStorage::new();
        let fs = Arc::new(PosixFs::new(backend));
        let mut bash = Bash::builder().fs(fs).build();

        let result = bash
            .exec("echo hello > /tmp/test.txt && cat /tmp/test.txt")
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello\n");
    }
}
