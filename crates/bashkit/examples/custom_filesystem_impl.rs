//! Custom FileSystem implementation example
//!
//! Demonstrates how to implement the FileSystem trait for a custom backend.
//! This example shows a "session file store" adapter pattern - useful for
//! bridging bashkit to external storage systems.
//!
//! Run with: cargo run --example custom_filesystem_impl

use bashkit::{async_trait, Bash, DirEntry, Error, FileSystem, FileType, Metadata, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

/// A mock session file store representing an external storage system.
/// In a real implementation, this would be a trait object or client
/// that connects to your actual storage backend.
#[derive(Default)]
pub struct MockSessionStore {
    files: RwLock<HashMap<PathBuf, Vec<u8>>>,
}

impl MockSessionStore {
    pub fn new() -> Self {
        let mut files = HashMap::new();
        // Initialize with some default directories
        files.insert(PathBuf::from("/"), Vec::new());
        files.insert(PathBuf::from("/tmp"), Vec::new());
        files.insert(PathBuf::from("/home"), Vec::new());
        files.insert(PathBuf::from("/home/user"), Vec::new());
        Self {
            files: RwLock::new(files),
        }
    }

    pub fn is_directory(&self, path: &Path) -> bool {
        let files = self.files.read().unwrap();
        // A path is a directory if it exists AND either:
        // - Has empty content (directory marker)
        // - Has children (other paths start with this path + /)
        if let Some(content) = files.get(path) {
            if content.is_empty() {
                return true;
            }
        }
        // Check for children
        let path_str = path.to_string_lossy();
        let prefix = if path_str.ends_with('/') {
            path_str.to_string()
        } else {
            format!("{}/", path_str)
        };
        files
            .keys()
            .any(|k| k.to_string_lossy().starts_with(&prefix) && k != path)
    }
}

/// A custom FileSystem adapter that bridges bashkit to a session store.
///
/// This pattern is useful when you want to:
/// - Connect bashkit to an external storage system
/// - Provide live visibility of files during bash execution
/// - Avoid pre/post sync of entire filesystem
pub struct SessionFileSystemAdapter {
    #[allow(dead_code)] // Would be used in real implementation for session-scoped operations
    session_id: String,
    store: Arc<MockSessionStore>,
}

impl SessionFileSystemAdapter {
    pub fn new(session_id: impl Into<String>, store: Arc<MockSessionStore>) -> Self {
        Self {
            session_id: session_id.into(),
            store,
        }
    }

    fn normalize_path(&self, path: &Path) -> PathBuf {
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

// The async_trait macro is re-exported from bashkit for convenience
#[async_trait]
impl FileSystem for SessionFileSystemAdapter {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let path = self.normalize_path(path);
        let files = self.store.files.read().unwrap();

        if let Some(content) = files.get(&path) {
            if content.is_empty() && self.store.is_directory(&path) {
                return Err(Error::Io(std::io::Error::other("is a directory")));
            }
            Ok(content.clone())
        } else {
            Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("file not found: {}", path.display()),
            )))
        }
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = self.normalize_path(path);

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            if !self.exists(parent).await? {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("parent directory not found: {}", parent.display()),
                )));
            }
        }

        let mut files = self.store.files.write().unwrap();
        files.insert(path, content.to_vec());
        Ok(())
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = self.normalize_path(path);
        let mut files = self.store.files.write().unwrap();

        if let Some(existing) = files.get_mut(&path) {
            existing.extend_from_slice(content);
            Ok(())
        } else {
            // Create new file
            files.insert(path, content.to_vec());
            Ok(())
        }
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = self.normalize_path(path);

        if recursive {
            let mut current = PathBuf::new();
            for component in path.components() {
                current.push(component);
                let mut files = self.store.files.write().unwrap();
                if !files.contains_key(&current) {
                    files.insert(current.clone(), Vec::new());
                }
            }
        } else {
            if let Some(parent) = path.parent() {
                if !self.exists(parent).await? {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("parent directory not found: {}", parent.display()),
                    )));
                }
            }
            let mut files = self.store.files.write().unwrap();
            if files.contains_key(&path) {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("directory already exists: {}", path.display()),
                )));
            }
            files.insert(path, Vec::new());
        }
        Ok(())
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = self.normalize_path(path);
        let mut files = self.store.files.write().unwrap();

        if !files.contains_key(&path) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("path not found: {}", path.display()),
            )));
        }

        let is_dir = self.store.is_directory(&path);
        if is_dir && !recursive {
            // Check if directory is empty
            let path_str = path.to_string_lossy();
            let prefix = format!("{}/", path_str);
            let has_children = files
                .keys()
                .any(|k| k.to_string_lossy().starts_with(&prefix));
            if has_children {
                return Err(Error::Io(std::io::Error::other("directory not empty")));
            }
        }

        if recursive {
            let path_str = path.to_string_lossy();
            let prefix = format!("{}/", path_str);
            let to_remove: Vec<_> = files
                .keys()
                .filter(|k| k.to_string_lossy().starts_with(&prefix) || *k == &path)
                .cloned()
                .collect();
            for key in to_remove {
                files.remove(&key);
            }
        } else {
            files.remove(&path);
        }
        Ok(())
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        let path = self.normalize_path(path);
        let files = self.store.files.read().unwrap();

        if let Some(content) = files.get(&path) {
            let is_dir = content.is_empty() && self.store.is_directory(&path);
            Ok(Metadata {
                file_type: if is_dir {
                    FileType::Directory
                } else {
                    FileType::File
                },
                size: content.len() as u64,
                mode: if is_dir { 0o755 } else { 0o644 },
                modified: SystemTime::now(),
                created: SystemTime::now(),
            })
        } else {
            Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("path not found: {}", path.display()),
            )))
        }
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let path = self.normalize_path(path);
        let files = self.store.files.read().unwrap();

        if !files.contains_key(&path) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("directory not found: {}", path.display()),
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
                // Extract immediate child name
                let remainder = &file_str[prefix.len()..];
                if let Some(slash_idx) = remainder.find('/') {
                    let name = &remainder[..slash_idx];
                    if !name.is_empty() && seen.insert(name.to_string()) {
                        entries.push(DirEntry {
                            name: name.to_string(),
                            metadata: Metadata {
                                file_type: FileType::Directory,
                                size: 0,
                                mode: 0o755,
                                modified: SystemTime::now(),
                                created: SystemTime::now(),
                            },
                        });
                    }
                } else if !remainder.is_empty() && seen.insert(remainder.to_string()) {
                    let is_dir = content.is_empty();
                    entries.push(DirEntry {
                        name: remainder.to_string(),
                        metadata: Metadata {
                            file_type: if is_dir {
                                FileType::Directory
                            } else {
                                FileType::File
                            },
                            size: content.len() as u64,
                            mode: if is_dir { 0o755 } else { 0o644 },
                            modified: SystemTime::now(),
                            created: SystemTime::now(),
                        },
                    });
                }
            }
        }

        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        let path = self.normalize_path(path);
        let files = self.store.files.read().unwrap();
        Ok(files.contains_key(&path))
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let from = self.normalize_path(from);
        let to = self.normalize_path(to);

        let mut files = self.store.files.write().unwrap();

        if let Some(content) = files.remove(&from) {
            files.insert(to, content);
            Ok(())
        } else {
            Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("source not found: {}", from.display()),
            )))
        }
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        let from = self.normalize_path(from);
        let to = self.normalize_path(to);

        let files = self.store.files.read().unwrap();

        if let Some(content) = files.get(&from) {
            let content = content.clone();
            drop(files);

            let mut files = self.store.files.write().unwrap();
            files.insert(to, content);
            Ok(())
        } else {
            Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("source not found: {}", from.display()),
            )))
        }
    }

    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        // Simple implementation: store target path as file content with special marker
        let link = self.normalize_path(link);
        let mut files = self.store.files.write().unwrap();

        // Store symlink target as content (simplified - real impl would track separately)
        let target_bytes = format!("SYMLINK:{}", target.display()).into_bytes();
        files.insert(link, target_bytes);
        Ok(())
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        let path = self.normalize_path(path);
        let files = self.store.files.read().unwrap();

        if let Some(content) = files.get(&path) {
            let content_str = String::from_utf8_lossy(content);
            if let Some(target) = content_str.strip_prefix("SYMLINK:") {
                return Ok(PathBuf::from(target));
            }
        }
        Err(Error::Io(std::io::Error::other("not a symbolic link")))
    }

    async fn chmod(&self, path: &Path, _mode: u32) -> Result<()> {
        // Simplified: just verify the file exists
        let path = self.normalize_path(path);
        let files = self.store.files.read().unwrap();

        if files.contains_key(&path) {
            // In a real implementation, store and track permissions
            Ok(())
        } else {
            Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("path not found: {}", path.display()),
            )))
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Custom FileSystem Implementation Example ===\n");
    println!("This demonstrates implementing the FileSystem trait");
    println!("for a custom storage backend.\n");

    // Create a shared session store (simulating external storage)
    let store = Arc::new(MockSessionStore::new());

    // Pre-populate some files in the store (simulating existing session data)
    {
        let mut files = store.files.write().unwrap();
        files.insert(PathBuf::from("/config"), Vec::new());
        files.insert(
            PathBuf::from("/config/settings.json"),
            br#"{"debug": true, "port": 8080}"#.to_vec(),
        );
    }

    // Create the adapter
    let session_fs = Arc::new(SessionFileSystemAdapter::new("session-123", store.clone()));

    // Create Bash with our custom filesystem
    let mut bash = Bash::builder().fs(session_fs.clone()).build();

    // Demonstrate 1: Read pre-existing files
    println!("1. Reading pre-populated config:");
    let result = bash.exec("cat /config/settings.json").await?;
    println!("   {}", result.stdout.trim());

    // Demonstrate 2: Write files during execution
    println!("\n2. Creating files during bash execution:");
    bash.exec("echo 'Hello from bash!' > /tmp/greeting.txt")
        .await?;
    let result = bash.exec("cat /tmp/greeting.txt").await?;
    println!("   {}", result.stdout.trim());

    // Demonstrate 3: Live visibility - files written externally are immediately visible
    println!("\n3. Live visibility (external writes visible to bash):");
    {
        // Simulate another tool writing to the session store
        let mut files = store.files.write().unwrap();
        files.insert(
            PathBuf::from("/tmp/external.txt"),
            b"Written by external tool".to_vec(),
        );
    }
    let result = bash.exec("cat /tmp/external.txt").await?;
    println!("   {}", result.stdout.trim());

    // Demonstrate 4: Directory operations
    println!("\n4. Directory operations:");
    bash.exec("mkdir -p /data/nested/dir").await?;
    bash.exec("echo 'file1' > /data/file1.txt").await?;
    bash.exec("echo 'file2' > /data/file2.txt").await?;
    // Note: 'ls' is not a builtin, but we can list directories via the API
    println!("   Created /data with nested dir and files");

    // Demonstrate 5: Direct filesystem API access (listing directories)
    println!("\n5. Direct filesystem API access (directory listing):");
    let entries = session_fs.read_dir(Path::new("/data")).await?;
    for entry in entries {
        println!(
            "   - {} ({:?}, {} bytes)",
            entry.name, entry.metadata.file_type, entry.metadata.size
        );
    }

    // Demonstrate 6: File metadata
    println!("\n6. File metadata:");
    let stat = session_fs.stat(Path::new("/data/file1.txt")).await?;
    println!("   file_type: {:?}", stat.file_type);
    println!("   size: {} bytes", stat.size);
    println!("   mode: {:o}", stat.mode);

    println!("\n=== Example Complete ===");
    println!("\nKey benefits of custom FileSystem adapters:");
    println!("  - Live visibility: External changes immediately visible to bash");
    println!("  - No sync overhead: No need to pre/post sync entire filesystem");
    println!("  - Memory efficient: Files read on-demand");
    println!("  - Single source of truth: One storage backend for all access");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_custom_fs_basic_operations() {
        let store = Arc::new(MockSessionStore::new());
        let fs = SessionFileSystemAdapter::new("test", store);

        // Write and read
        fs.write_file(Path::new("/tmp/test.txt"), b"hello")
            .await
            .unwrap();
        let content = fs.read_file(Path::new("/tmp/test.txt")).await.unwrap();
        assert_eq!(content, b"hello");

        // Exists
        assert!(fs.exists(Path::new("/tmp/test.txt")).await.unwrap());
        assert!(!fs.exists(Path::new("/tmp/nonexistent.txt")).await.unwrap());

        // Stat
        let stat = fs.stat(Path::new("/tmp/test.txt")).await.unwrap();
        assert!(stat.file_type.is_file());
        assert_eq!(stat.size, 5);
    }

    #[tokio::test]
    async fn test_custom_fs_with_bash() {
        let store = Arc::new(MockSessionStore::new());
        let fs = Arc::new(SessionFileSystemAdapter::new("test", store));
        let mut bash = Bash::builder().fs(fs).build();

        let result = bash
            .exec("echo hello > /tmp/test.txt && cat /tmp/test.txt")
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_custom_fs_directory_operations() {
        let store = Arc::new(MockSessionStore::new());
        let fs = SessionFileSystemAdapter::new("test", store);

        // Create nested directories
        fs.mkdir(Path::new("/data/nested/dir"), true).await.unwrap();
        assert!(fs.exists(Path::new("/data")).await.unwrap());
        assert!(fs.exists(Path::new("/data/nested")).await.unwrap());
        assert!(fs.exists(Path::new("/data/nested/dir")).await.unwrap());

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
}
