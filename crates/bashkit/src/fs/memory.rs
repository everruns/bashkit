//! In-memory filesystem implementation

use async_trait::async_trait;
use std::collections::HashMap;
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::SystemTime;

use super::traits::{DirEntry, FileSystem, FileType, Metadata};
use crate::error::Result;

/// In-memory filesystem.
///
/// Stores all files and directories in memory using a HashMap.
pub struct InMemoryFs {
    entries: RwLock<HashMap<PathBuf, FsEntry>>,
}

#[derive(Debug, Clone)]
enum FsEntry {
    File {
        content: Vec<u8>,
        metadata: Metadata,
    },
    Directory {
        metadata: Metadata,
    },
    Symlink {
        target: PathBuf,
        metadata: Metadata,
    },
}

impl Default for InMemoryFs {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryFs {
    /// Create a new in-memory filesystem.
    pub fn new() -> Self {
        let mut entries = HashMap::new();

        // Create root directory
        entries.insert(
            PathBuf::from("/"),
            FsEntry::Directory {
                metadata: Metadata {
                    file_type: FileType::Directory,
                    size: 0,
                    mode: 0o755,
                    modified: SystemTime::now(),
                    created: SystemTime::now(),
                },
            },
        );

        // Create common directories
        for dir in &["/tmp", "/home", "/home/user"] {
            entries.insert(
                PathBuf::from(dir),
                FsEntry::Directory {
                    metadata: Metadata {
                        file_type: FileType::Directory,
                        size: 0,
                        mode: 0o755,
                        modified: SystemTime::now(),
                        created: SystemTime::now(),
                    },
                },
            );
        }

        Self {
            entries: RwLock::new(entries),
        }
    }

    fn normalize_path(path: &Path) -> PathBuf {
        let mut result = PathBuf::new();

        for component in path.components() {
            match component {
                std::path::Component::RootDir => {
                    result.push("/");
                }
                std::path::Component::Normal(name) => {
                    result.push(name);
                }
                std::path::Component::ParentDir => {
                    result.pop();
                }
                std::path::Component::CurDir => {}
                std::path::Component::Prefix(_) => {}
            }
        }

        if result.as_os_str().is_empty() {
            result.push("/");
        }

        result
    }
}

#[async_trait]
impl FileSystem for InMemoryFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let path = Self::normalize_path(path);
        let entries = self.entries.read().unwrap();

        match entries.get(&path) {
            Some(FsEntry::File { content, .. }) => Ok(content.clone()),
            Some(FsEntry::Directory { .. }) => Err(IoError::other("is a directory").into()),
            Some(FsEntry::Symlink { .. }) => {
                // TODO: Follow symlinks
                Err(IoError::new(ErrorKind::NotFound, "file not found").into())
            }
            None => Err(IoError::new(ErrorKind::NotFound, "file not found").into()),
        }
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = Self::normalize_path(path);
        let mut entries = self.entries.write().unwrap();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            if !entries.contains_key(parent) && parent != Path::new("/") {
                return Err(IoError::new(ErrorKind::NotFound, "parent directory not found").into());
            }
        }

        entries.insert(
            path,
            FsEntry::File {
                content: content.to_vec(),
                metadata: Metadata {
                    file_type: FileType::File,
                    size: content.len() as u64,
                    mode: 0o644,
                    modified: SystemTime::now(),
                    created: SystemTime::now(),
                },
            },
        );

        Ok(())
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = Self::normalize_path(path);

        // Check if file exists and handle accordingly
        // We need to release the lock before potentially calling write_file
        let should_create = {
            let mut entries = self.entries.write().unwrap();

            match entries.get_mut(&path) {
                Some(FsEntry::File {
                    content: existing,
                    metadata,
                }) => {
                    existing.extend_from_slice(content);
                    metadata.size = existing.len() as u64;
                    metadata.modified = SystemTime::now();
                    return Ok(());
                }
                Some(FsEntry::Directory { .. }) => {
                    return Err(IoError::other("is a directory").into());
                }
                Some(FsEntry::Symlink { .. }) => {
                    return Err(IoError::new(ErrorKind::NotFound, "file not found").into());
                }
                None => true,
            }
        };

        if should_create {
            self.write_file(&path, content).await
        } else {
            Ok(())
        }
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = Self::normalize_path(path);
        let mut entries = self.entries.write().unwrap();

        if recursive {
            let mut current = PathBuf::from("/");
            for component in path.components().skip(1) {
                current.push(component);
                if !entries.contains_key(&current) {
                    entries.insert(
                        current.clone(),
                        FsEntry::Directory {
                            metadata: Metadata {
                                file_type: FileType::Directory,
                                size: 0,
                                mode: 0o755,
                                modified: SystemTime::now(),
                                created: SystemTime::now(),
                            },
                        },
                    );
                }
            }
        } else {
            // Check parent exists
            if let Some(parent) = path.parent() {
                if !entries.contains_key(parent) && parent != Path::new("/") {
                    return Err(
                        IoError::new(ErrorKind::NotFound, "parent directory not found").into(),
                    );
                }
            }

            if entries.contains_key(&path) {
                return Err(IoError::new(ErrorKind::AlreadyExists, "directory exists").into());
            }

            entries.insert(
                path,
                FsEntry::Directory {
                    metadata: Metadata {
                        file_type: FileType::Directory,
                        size: 0,
                        mode: 0o755,
                        modified: SystemTime::now(),
                        created: SystemTime::now(),
                    },
                },
            );
        }

        Ok(())
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = Self::normalize_path(path);
        let mut entries = self.entries.write().unwrap();

        match entries.get(&path) {
            Some(FsEntry::Directory { .. }) => {
                if recursive {
                    // Remove all entries under this path
                    let to_remove: Vec<PathBuf> = entries
                        .keys()
                        .filter(|p| p.starts_with(&path))
                        .cloned()
                        .collect();

                    for p in to_remove {
                        entries.remove(&p);
                    }
                } else {
                    // Check if directory is empty
                    let has_children = entries
                        .keys()
                        .any(|p| p != &path && p.parent() == Some(&path));

                    if has_children {
                        return Err(IoError::other("directory not empty").into());
                    }

                    entries.remove(&path);
                }
            }
            Some(FsEntry::File { .. }) | Some(FsEntry::Symlink { .. }) => {
                entries.remove(&path);
            }
            None => {
                return Err(IoError::new(ErrorKind::NotFound, "not found").into());
            }
        }

        Ok(())
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        let path = Self::normalize_path(path);
        let entries = self.entries.read().unwrap();

        match entries.get(&path) {
            Some(FsEntry::File { metadata, .. })
            | Some(FsEntry::Directory { metadata })
            | Some(FsEntry::Symlink { metadata, .. }) => Ok(metadata.clone()),
            None => Err(IoError::new(ErrorKind::NotFound, "not found").into()),
        }
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let path = Self::normalize_path(path);
        let entries = self.entries.read().unwrap();

        match entries.get(&path) {
            Some(FsEntry::Directory { .. }) => {
                let mut result = Vec::new();

                for (entry_path, entry) in entries.iter() {
                    if entry_path.parent() == Some(&path) && entry_path != &path {
                        let name = entry_path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();

                        let metadata = match entry {
                            FsEntry::File { metadata, .. }
                            | FsEntry::Directory { metadata }
                            | FsEntry::Symlink { metadata, .. } => metadata.clone(),
                        };

                        result.push(DirEntry { name, metadata });
                    }
                }

                Ok(result)
            }
            Some(_) => Err(IoError::other("not a directory").into()),
            None => Err(IoError::new(ErrorKind::NotFound, "not found").into()),
        }
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        let path = Self::normalize_path(path);
        let entries = self.entries.read().unwrap();
        Ok(entries.contains_key(&path))
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let from = Self::normalize_path(from);
        let to = Self::normalize_path(to);
        let mut entries = self.entries.write().unwrap();

        let entry = entries
            .remove(&from)
            .ok_or_else(|| IoError::new(ErrorKind::NotFound, "not found"))?;

        entries.insert(to, entry);
        Ok(())
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        let from = Self::normalize_path(from);
        let to = Self::normalize_path(to);
        let mut entries = self.entries.write().unwrap();

        let entry = entries
            .get(&from)
            .cloned()
            .ok_or_else(|| IoError::new(ErrorKind::NotFound, "not found"))?;

        entries.insert(to, entry);
        Ok(())
    }

    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        let link = Self::normalize_path(link);
        let mut entries = self.entries.write().unwrap();

        entries.insert(
            link,
            FsEntry::Symlink {
                target: target.to_path_buf(),
                metadata: Metadata {
                    file_type: FileType::Symlink,
                    size: 0,
                    mode: 0o777,
                    modified: SystemTime::now(),
                    created: SystemTime::now(),
                },
            },
        );

        Ok(())
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        let path = Self::normalize_path(path);
        let entries = self.entries.read().unwrap();

        match entries.get(&path) {
            Some(FsEntry::Symlink { target, .. }) => Ok(target.clone()),
            Some(_) => Err(IoError::other("not a symlink").into()),
            None => Err(IoError::new(ErrorKind::NotFound, "not found").into()),
        }
    }

    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        let path = Self::normalize_path(path);
        let mut entries = self.entries.write().unwrap();

        match entries.get_mut(&path) {
            Some(FsEntry::File { metadata, .. })
            | Some(FsEntry::Directory { metadata })
            | Some(FsEntry::Symlink { metadata, .. }) => {
                metadata.mode = mode;
                Ok(())
            }
            None => Err(IoError::new(ErrorKind::NotFound, "not found").into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_write_and_read_file() {
        let fs = InMemoryFs::new();

        fs.write_file(Path::new("/tmp/test.txt"), b"hello world")
            .await
            .unwrap();

        let content = fs.read_file(Path::new("/tmp/test.txt")).await.unwrap();
        assert_eq!(content, b"hello world");
    }

    #[tokio::test]
    async fn test_mkdir_and_read_dir() {
        let fs = InMemoryFs::new();

        fs.mkdir(Path::new("/tmp/mydir"), false).await.unwrap();
        fs.write_file(Path::new("/tmp/mydir/file.txt"), b"test")
            .await
            .unwrap();

        let entries = fs.read_dir(Path::new("/tmp/mydir")).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "file.txt");
    }

    #[tokio::test]
    async fn test_exists() {
        let fs = InMemoryFs::new();

        assert!(fs.exists(Path::new("/tmp")).await.unwrap());
        assert!(!fs.exists(Path::new("/tmp/nonexistent")).await.unwrap());
    }
}
