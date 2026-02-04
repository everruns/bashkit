//! Git client implementation for BashKit.
//!
//! Provides sandboxed git operations on the virtual filesystem.
//!
//! # Implementation Notes
//!
//! This implementation stores git state in a simplified format within the VFS.
//! For Phase 1, we implement local operations without full git object format
//! compatibility, focusing on the user-facing behavior and security model.
//!
//! Future phases may integrate more deeply with gitoxide for full compatibility.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::fs::FileSystem;

use super::config::GitConfig;

/// Git client for sandboxed operations.
///
/// All operations work on the virtual filesystem and never access
/// the host's git installation or configuration.
#[derive(Debug, Clone)]
pub struct GitClient {
    config: GitConfig,
}

/// Result of git status command.
#[derive(Debug, Clone, Default)]
pub struct GitStatus {
    /// Current branch name
    pub branch: String,
    /// Staged files (ready to commit)
    pub staged: Vec<String>,
    /// Modified files (not staged)
    pub modified: Vec<String>,
    /// Untracked files
    pub untracked: Vec<String>,
}

/// A git commit entry.
#[derive(Debug, Clone)]
pub struct GitLogEntry {
    /// Commit hash (shortened)
    pub hash: String,
    /// Author name
    pub author: String,
    /// Commit timestamp (Unix time)
    pub timestamp: i64,
    /// Commit message
    pub message: String,
}

/// A remote entry.
#[derive(Debug, Clone)]
pub struct Remote {
    /// Remote name
    pub name: String,
    /// Remote URL
    pub url: String,
}

/// A branch entry.
#[derive(Debug, Clone)]
pub struct Branch {
    /// Branch name
    pub name: String,
    /// Whether this is the current branch
    pub current: bool,
}

impl GitClient {
    /// Create a new git client with the given configuration.
    pub fn new(config: GitConfig) -> Self {
        Self { config }
    }

    /// Get the git configuration.
    pub fn config(&self) -> &GitConfig {
        &self.config
    }

    /// Initialize a new git repository.
    ///
    /// Creates the `.git` directory structure at the specified path.
    ///
    /// # Security (TM-GIT-005)
    ///
    /// The path must be within the virtual filesystem. Path traversal
    /// attacks are blocked by the VFS layer.
    pub async fn init(&self, fs: &Arc<dyn FileSystem>, repo_path: &Path) -> Result<String> {
        let git_dir = repo_path.join(".git");

        // Check if already initialized
        if fs.exists(&git_dir).await? {
            return Ok(format!(
                "Reinitialized existing Git repository in {}/.git/\n",
                repo_path.display()
            ));
        }

        // Create .git directory structure
        fs.mkdir(&git_dir, true).await?;
        fs.mkdir(&git_dir.join("objects"), true).await?;
        fs.mkdir(&git_dir.join("refs"), true).await?;
        fs.mkdir(&git_dir.join("refs/heads"), true).await?;
        fs.mkdir(&git_dir.join("refs/tags"), true).await?;

        // Create HEAD pointing to master
        fs.write_file(&git_dir.join("HEAD"), b"ref: refs/heads/master\n")
            .await?;

        // Create config with author info
        let config_content = format!(
            "[core]\n\
             \trepositoryformatversion = 0\n\
             \tfilemode = true\n\
             \tbare = false\n\
             [user]\n\
             \tname = {}\n\
             \temail = {}\n",
            self.config.author_name, self.config.author_email
        );
        fs.write_file(&git_dir.join("config"), config_content.as_bytes())
            .await?;

        // Create empty index
        fs.write_file(&git_dir.join("index"), b"").await?;

        Ok(format!(
            "Initialized empty Git repository in {}/.git/\n",
            repo_path.display()
        ))
    }

    /// Get or set a git config value.
    ///
    /// # Security (TM-GIT-003)
    ///
    /// Only reads from the repository's `.git/config`. Never accesses
    /// host's `~/.gitconfig` or system git config.
    pub async fn config_get(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        key: &str,
    ) -> Result<Option<String>> {
        let config_path = repo_path.join(".git/config");

        if !fs.exists(&config_path).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        let content = fs.read_file(&config_path).await?;
        let content = String::from_utf8_lossy(&content);

        // Simple config parser - looks for key = value
        let key_lower = key.to_lowercase();
        let parts: Vec<&str> = key_lower.split('.').collect();

        if parts.len() != 2 {
            return Ok(None);
        }

        let (section, name) = (parts[0], parts[1]);
        let mut in_section = false;

        for line in content.lines() {
            let line = line.trim();

            // Check for section header
            if line.starts_with('[') && line.ends_with(']') {
                let sect = &line[1..line.len() - 1].to_lowercase();
                in_section = sect == section;
                continue;
            }

            // Check for key = value in the right section
            if in_section {
                if let Some((k, v)) = line.split_once('=') {
                    if k.trim().to_lowercase() == name {
                        return Ok(Some(v.trim().to_string()));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Set a git config value.
    pub async fn config_set(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        key: &str,
        value: &str,
    ) -> Result<()> {
        let config_path = repo_path.join(".git/config");

        if !fs.exists(&config_path).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        let content = fs.read_file(&config_path).await?;
        let content = String::from_utf8_lossy(&content);

        let key_lower = key.to_lowercase();
        let parts: Vec<&str> = key_lower.split('.').collect();

        if parts.len() != 2 {
            return Err(Error::Internal(format!("error: invalid key: {}", key)));
        }

        let (section, name) = (parts[0], parts[1]);

        // Rebuild config with updated value
        let mut new_content = String::new();
        let mut in_section = false;
        let mut found = false;
        let mut section_exists = false;

        for line in content.lines() {
            let trimmed = line.trim();

            // Check for section header
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                // If we were in the target section and didn't find the key, add it
                if in_section && !found {
                    new_content.push_str(&format!("\t{} = {}\n", name, value));
                    found = true;
                }

                let sect = &trimmed[1..trimmed.len() - 1].to_lowercase();
                in_section = sect == section;
                if in_section {
                    section_exists = true;
                }
                new_content.push_str(line);
                new_content.push('\n');
                continue;
            }

            // Check for key = value in the right section
            if in_section {
                if let Some((k, _)) = trimmed.split_once('=') {
                    if k.trim().to_lowercase() == name {
                        // Replace this line
                        new_content.push_str(&format!("\t{} = {}\n", name, value));
                        found = true;
                        continue;
                    }
                }
            }

            new_content.push_str(line);
            new_content.push('\n');
        }

        // If we were in the target section at the end and didn't find the key
        if in_section && !found {
            new_content.push_str(&format!("\t{} = {}\n", name, value));
        }

        // If section doesn't exist, add it
        if !section_exists {
            new_content.push_str(&format!("[{}]\n\t{} = {}\n", section, name, value));
        }

        fs.write_file(&config_path, new_content.as_bytes()).await?;
        Ok(())
    }

    /// Add files to the staging area.
    pub async fn add(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        paths: &[&str],
    ) -> Result<()> {
        let git_dir = repo_path.join(".git");
        let index_path = git_dir.join("index");

        if !fs.exists(&git_dir).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        // Load current index
        let mut staged: HashSet<String> = HashSet::new();
        if fs.exists(&index_path).await? {
            let content = fs.read_file(&index_path).await?;
            let content = String::from_utf8_lossy(&content);
            for line in content.lines() {
                if !line.is_empty() {
                    staged.insert(line.to_string());
                }
            }
        }

        // Add files
        for path_str in paths {
            let path = if Path::new(path_str).is_absolute() {
                PathBuf::from(path_str)
            } else {
                repo_path.join(path_str)
            };

            // Handle "." to add all files
            if *path_str == "." {
                self.add_directory_recursive(fs, repo_path, repo_path, &mut staged)
                    .await?;
                continue;
            }

            if fs.exists(&path).await? {
                // Get relative path from repo root
                let rel_path = path
                    .strip_prefix(repo_path)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();

                if !rel_path.starts_with(".git") {
                    staged.insert(rel_path);
                }
            }
        }

        // Write index
        let index_content: String = staged.into_iter().collect::<Vec<_>>().join("\n");
        fs.write_file(&index_path, index_content.as_bytes()).await?;

        Ok(())
    }

    /// Recursively add files from a directory.
    async fn add_directory_recursive(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        dir: &Path,
        staged: &mut HashSet<String>,
    ) -> Result<()> {
        let entries = fs.read_dir(dir).await?;

        for entry in entries {
            let name = &entry.name;

            // Skip .git directory
            if name == ".git" {
                continue;
            }

            let path = dir.join(name);
            let rel_path = path
                .strip_prefix(repo_path)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();

            if entry.metadata.file_type.is_dir() {
                Box::pin(self.add_directory_recursive(fs, repo_path, &path, staged)).await?;
            } else {
                staged.insert(rel_path);
            }
        }

        Ok(())
    }

    /// Create a commit.
    ///
    /// # Security (TM-GIT-002)
    ///
    /// Uses the configured author identity, never reads from host.
    pub async fn commit(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        message: &str,
    ) -> Result<String> {
        let git_dir = repo_path.join(".git");
        let index_path = git_dir.join("index");
        let commits_path = git_dir.join("commits");

        if !fs.exists(&git_dir).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        // Check if there are staged files
        if !fs.exists(&index_path).await? {
            return Err(Error::Internal(
                "nothing to commit, working tree clean".to_string(),
            ));
        }

        let index_content = fs.read_file(&index_path).await?;
        let index_content = String::from_utf8_lossy(&index_content);

        if index_content.trim().is_empty() {
            return Err(Error::Internal(
                "nothing to commit, working tree clean".to_string(),
            ));
        }

        // Generate a commit hash (simplified - just use timestamp + random)
        let timestamp = chrono::Utc::now().timestamp();
        let hash = format!("{:08x}", timestamp as u32 ^ 0xdeadbeef);

        // Load existing commits
        let mut commits = String::new();
        if fs.exists(&commits_path).await? {
            let content = fs.read_file(&commits_path).await?;
            commits = String::from_utf8_lossy(&content).to_string();
        }

        // Add new commit
        let commit_entry = format!(
            "{}|{}|{}|{}|{}\n",
            hash,
            self.config.author_name,
            self.config.author_email,
            timestamp,
            message.replace('|', "\\|").replace('\n', "\\n")
        );
        commits = commit_entry + &commits;

        // Ensure commits directory parent exists
        if !fs.exists(commits_path.parent().unwrap_or(&git_dir)).await? {
            fs.mkdir(&git_dir, true).await?;
        }

        fs.write_file(&commits_path, commits.as_bytes()).await?;

        // Track committed files by adding staged files to the tracked set
        let tracked_path = git_dir.join("tracked");
        let mut tracked: HashSet<String> = HashSet::new();
        if fs.exists(&tracked_path).await? {
            let content = fs.read_file(&tracked_path).await?;
            let content = String::from_utf8_lossy(&content);
            for line in content.lines() {
                if !line.is_empty() {
                    tracked.insert(line.to_string());
                }
            }
        }

        // Add staged files to tracked
        for line in index_content.lines() {
            if !line.is_empty() {
                tracked.insert(line.to_string());
            }
        }

        // Write tracked files
        let tracked_content: String = tracked.into_iter().collect::<Vec<_>>().join("\n");
        fs.write_file(&tracked_path, tracked_content.as_bytes())
            .await?;

        // Clear index after commit
        fs.write_file(&index_path, b"").await?;

        // Update HEAD
        let head_ref_path = git_dir.join("refs/heads/master");
        fs.write_file(&head_ref_path, hash.as_bytes()).await?;

        Ok(format!(
            "[master {}] {}\n",
            &hash[..7.min(hash.len())],
            message.lines().next().unwrap_or(message)
        ))
    }

    /// Get repository status.
    pub async fn status(&self, fs: &Arc<dyn FileSystem>, repo_path: &Path) -> Result<GitStatus> {
        let git_dir = repo_path.join(".git");
        let index_path = git_dir.join("index");

        if !fs.exists(&git_dir).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        let mut status = GitStatus {
            branch: "master".to_string(),
            ..Default::default()
        };

        // Read HEAD to get branch
        let head_path = git_dir.join("HEAD");
        if fs.exists(&head_path).await? {
            let content = fs.read_file(&head_path).await?;
            let content = String::from_utf8_lossy(&content);
            if let Some(branch) = content.strip_prefix("ref: refs/heads/") {
                status.branch = branch.trim().to_string();
            }
        }

        // Load staged files
        let mut staged_files: HashSet<String> = HashSet::new();
        if fs.exists(&index_path).await? {
            let content = fs.read_file(&index_path).await?;
            let content = String::from_utf8_lossy(&content);
            for line in content.lines() {
                if !line.is_empty() {
                    staged_files.insert(line.to_string());
                    status.staged.push(line.to_string());
                }
            }
        }

        // Load tracked (committed) files
        let tracked_path = git_dir.join("tracked");
        let mut tracked_files: HashSet<String> = HashSet::new();
        if fs.exists(&tracked_path).await? {
            let content = fs.read_file(&tracked_path).await?;
            let content = String::from_utf8_lossy(&content);
            for line in content.lines() {
                if !line.is_empty() {
                    tracked_files.insert(line.to_string());
                }
            }
        }

        // Find all files in repo (excluding .git)
        let all_files = self.list_files_recursive(fs, repo_path, repo_path).await?;

        // Files not in staging or tracked are untracked
        for file in all_files {
            if !staged_files.contains(&file) && !tracked_files.contains(&file) {
                status.untracked.push(file);
            }
        }

        status.staged.sort();
        status.modified.sort();
        status.untracked.sort();

        Ok(status)
    }

    /// List all files recursively.
    async fn list_files_recursive(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        dir: &Path,
    ) -> Result<Vec<String>> {
        let mut files = Vec::new();
        let entries = fs.read_dir(dir).await?;

        for entry in entries {
            let name = &entry.name;

            // Skip .git directory
            if name == ".git" {
                continue;
            }

            let path = dir.join(name);
            let rel_path = path
                .strip_prefix(repo_path)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();

            if entry.metadata.file_type.is_dir() {
                let sub_files = Box::pin(self.list_files_recursive(fs, repo_path, &path)).await?;
                files.extend(sub_files);
            } else {
                files.push(rel_path);
            }
        }

        Ok(files)
    }

    /// Get commit log.
    pub async fn log(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        limit: Option<usize>,
    ) -> Result<Vec<GitLogEntry>> {
        let git_dir = repo_path.join(".git");
        let commits_path = git_dir.join("commits");

        if !fs.exists(&git_dir).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        let mut entries = Vec::new();

        if fs.exists(&commits_path).await? {
            let content = fs.read_file(&commits_path).await?;
            let content = String::from_utf8_lossy(&content);

            for line in content.lines() {
                if line.is_empty() {
                    continue;
                }

                let parts: Vec<&str> = line.splitn(5, '|').collect();
                if parts.len() >= 5 {
                    entries.push(GitLogEntry {
                        hash: parts[0].to_string(),
                        author: format!("{} <{}>", parts[1], parts[2]),
                        timestamp: parts[3].parse().unwrap_or(0),
                        message: parts[4].replace("\\|", "|").replace("\\n", "\n"),
                    });
                }

                if let Some(limit) = limit {
                    if entries.len() >= limit {
                        break;
                    }
                }
            }
        }

        Ok(entries)
    }

    /// Format git log output.
    pub fn format_log(&self, entries: &[GitLogEntry]) -> String {
        let mut output = String::new();

        for entry in entries {
            output.push_str(&format!("commit {}\n", entry.hash));
            output.push_str(&format!("Author: {}\n", entry.author));

            // Format timestamp
            if let Some(dt) = chrono::DateTime::from_timestamp(entry.timestamp, 0) {
                output.push_str(&format!(
                    "Date:   {}\n",
                    dt.format("%a %b %d %H:%M:%S %Y %z")
                ));
            }

            output.push('\n');
            for line in entry.message.lines() {
                output.push_str(&format!("    {}\n", line));
            }
            output.push('\n');
        }

        output
    }

    /// Format git status output.
    pub fn format_status(&self, status: &GitStatus) -> String {
        let mut output = String::new();

        output.push_str(&format!("On branch {}\n", status.branch));

        if !status.staged.is_empty() {
            output.push_str("\nChanges to be committed:\n");
            output.push_str("  (use \"git restore --staged <file>...\" to unstage)\n");
            for file in &status.staged {
                output.push_str(&format!("\tnew file:   {}\n", file));
            }
        }

        if !status.modified.is_empty() {
            output.push_str("\nChanges not staged for commit:\n");
            output.push_str("  (use \"git add <file>...\" to update what will be committed)\n");
            for file in &status.modified {
                output.push_str(&format!("\tmodified:   {}\n", file));
            }
        }

        if !status.untracked.is_empty() {
            output.push_str("\nUntracked files:\n");
            output.push_str("  (use \"git add <file>...\" to include in what will be committed)\n");
            for file in &status.untracked {
                output.push_str(&format!("\t{}\n", file));
            }
        }

        if status.staged.is_empty() && status.modified.is_empty() && status.untracked.is_empty() {
            output.push_str("\nnothing to commit, working tree clean\n");
        }

        output
    }

    // ==================== Remote Operations (Phase 2) ====================

    /// Add a remote to the repository.
    pub async fn remote_add(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        name: &str,
        url: &str,
    ) -> Result<()> {
        let git_dir = repo_path.join(".git");

        if !fs.exists(&git_dir).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        // Validate URL against allowlist
        self.config.is_url_allowed(url).map_err(Error::Internal)?;

        // Store remote in .git/remotes file
        let remotes_path = git_dir.join("remotes");
        let mut remotes = String::new();
        if fs.exists(&remotes_path).await? {
            let content = fs.read_file(&remotes_path).await?;
            remotes = String::from_utf8_lossy(&content).to_string();

            // Check if remote already exists
            for line in remotes.lines() {
                if let Some((existing_name, _)) = line.split_once('|') {
                    if existing_name == name {
                        return Err(Error::Internal(format!(
                            "error: remote {} already exists",
                            name
                        )));
                    }
                }
            }
        }

        // Add new remote
        remotes.push_str(&format!("{}|{}\n", name, url));
        fs.write_file(&remotes_path, remotes.as_bytes()).await?;

        Ok(())
    }

    /// Remove a remote from the repository.
    pub async fn remote_remove(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        name: &str,
    ) -> Result<()> {
        let git_dir = repo_path.join(".git");
        let remotes_path = git_dir.join("remotes");

        if !fs.exists(&git_dir).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        if !fs.exists(&remotes_path).await? {
            return Err(Error::Internal(format!("error: no such remote: {}", name)));
        }

        let content = fs.read_file(&remotes_path).await?;
        let content = String::from_utf8_lossy(&content);

        let mut found = false;
        let mut new_content = String::new();
        for line in content.lines() {
            if let Some((existing_name, _)) = line.split_once('|') {
                if existing_name == name {
                    found = true;
                    continue;
                }
            }
            new_content.push_str(line);
            new_content.push('\n');
        }

        if !found {
            return Err(Error::Internal(format!("error: no such remote: {}", name)));
        }

        fs.write_file(&remotes_path, new_content.as_bytes()).await?;
        Ok(())
    }

    /// List all remotes.
    pub async fn remote_list(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
    ) -> Result<Vec<Remote>> {
        let git_dir = repo_path.join(".git");
        let remotes_path = git_dir.join("remotes");

        if !fs.exists(&git_dir).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        let mut remotes = Vec::new();

        if fs.exists(&remotes_path).await? {
            let content = fs.read_file(&remotes_path).await?;
            let content = String::from_utf8_lossy(&content);

            for line in content.lines() {
                if let Some((name, url)) = line.split_once('|') {
                    remotes.push(Remote {
                        name: name.to_string(),
                        url: url.to_string(),
                    });
                }
            }
        }

        Ok(remotes)
    }

    /// Clone a repository.
    ///
    /// # Note
    ///
    /// In sandbox mode, this validates the URL against the allowlist
    /// but actual network operations are not supported.
    pub async fn clone(
        &self,
        _fs: &Arc<dyn FileSystem>,
        url: &str,
        _dest: &Path,
    ) -> Result<String> {
        // Validate URL
        self.config.is_url_allowed(url).map_err(Error::Internal)?;

        // Return sandbox mode message
        Err(Error::Internal(format!(
            "git clone: network operations not supported in sandbox mode\n\
             hint: URL '{}' passed allowlist validation\n\
             hint: to clone, use a pre-populated virtual filesystem instead",
            url
        )))
    }

    /// Fetch from a remote.
    ///
    /// # Note
    ///
    /// In sandbox mode, this validates the URL against the allowlist
    /// but actual network operations are not supported.
    pub async fn fetch(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        remote: &str,
    ) -> Result<String> {
        // Get remote URL
        let remotes = self.remote_list(fs, repo_path).await?;
        let remote_entry = remotes
            .iter()
            .find(|r| r.name == remote)
            .ok_or_else(|| Error::Internal(format!("error: remote '{}' not found", remote)))?;

        // Validate URL
        self.config
            .is_url_allowed(&remote_entry.url)
            .map_err(Error::Internal)?;

        // Return sandbox mode message
        Err(Error::Internal(format!(
            "git fetch: network operations not supported in sandbox mode\n\
             hint: remote '{}' URL '{}' passed allowlist validation",
            remote, remote_entry.url
        )))
    }

    /// Push to a remote.
    ///
    /// # Note
    ///
    /// In sandbox mode, this validates the URL against the allowlist
    /// but actual network operations are not supported.
    pub async fn push(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        remote: &str,
    ) -> Result<String> {
        // Get remote URL
        let remotes = self.remote_list(fs, repo_path).await?;
        let remote_entry = remotes
            .iter()
            .find(|r| r.name == remote)
            .ok_or_else(|| Error::Internal(format!("error: remote '{}' not found", remote)))?;

        // Validate URL
        self.config
            .is_url_allowed(&remote_entry.url)
            .map_err(Error::Internal)?;

        // Return sandbox mode message
        Err(Error::Internal(format!(
            "git push: network operations not supported in sandbox mode\n\
             hint: remote '{}' URL '{}' passed allowlist validation",
            remote, remote_entry.url
        )))
    }

    /// Pull from a remote.
    ///
    /// # Note
    ///
    /// In sandbox mode, this validates the URL against the allowlist
    /// but actual network operations are not supported.
    pub async fn pull(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        remote: &str,
    ) -> Result<String> {
        // Get remote URL
        let remotes = self.remote_list(fs, repo_path).await?;
        let remote_entry = remotes
            .iter()
            .find(|r| r.name == remote)
            .ok_or_else(|| Error::Internal(format!("error: remote '{}' not found", remote)))?;

        // Validate URL
        self.config
            .is_url_allowed(&remote_entry.url)
            .map_err(Error::Internal)?;

        // Return sandbox mode message
        Err(Error::Internal(format!(
            "git pull: network operations not supported in sandbox mode\n\
             hint: remote '{}' URL '{}' passed allowlist validation",
            remote, remote_entry.url
        )))
    }

    // ==================== Advanced Operations (Phase 3) ====================

    /// List all branches.
    pub async fn branch_list(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
    ) -> Result<Vec<Branch>> {
        let git_dir = repo_path.join(".git");
        let refs_heads = git_dir.join("refs/heads");

        if !fs.exists(&git_dir).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        // Get current branch
        let current_branch = self.get_current_branch(fs, repo_path).await?;

        let mut branches = Vec::new();

        if fs.exists(&refs_heads).await? {
            let entries = fs.read_dir(&refs_heads).await?;
            for entry in entries {
                if !entry.metadata.file_type.is_dir() {
                    let name = entry.name.clone();
                    branches.push(Branch {
                        current: name == current_branch,
                        name,
                    });
                }
            }
        }

        // Sort branches, current first
        branches.sort_by(|a, b| {
            if a.current && !b.current {
                std::cmp::Ordering::Less
            } else if !a.current && b.current {
                std::cmp::Ordering::Greater
            } else {
                a.name.cmp(&b.name)
            }
        });

        Ok(branches)
    }

    /// Get the current branch name.
    async fn get_current_branch(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
    ) -> Result<String> {
        let git_dir = repo_path.join(".git");
        let head_path = git_dir.join("HEAD");

        if fs.exists(&head_path).await? {
            let content = fs.read_file(&head_path).await?;
            let content = String::from_utf8_lossy(&content);
            if let Some(branch) = content.trim().strip_prefix("ref: refs/heads/") {
                return Ok(branch.to_string());
            }
        }

        Ok("master".to_string())
    }

    /// Create a new branch.
    pub async fn branch_create(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        name: &str,
    ) -> Result<()> {
        let git_dir = repo_path.join(".git");
        let refs_heads = git_dir.join("refs/heads");
        let branch_path = refs_heads.join(name);

        if !fs.exists(&git_dir).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        if fs.exists(&branch_path).await? {
            return Err(Error::Internal(format!(
                "fatal: a branch named '{}' already exists",
                name
            )));
        }

        // Get current HEAD commit
        let head_ref = refs_heads.join(self.get_current_branch(fs, repo_path).await?);
        let commit_hash = if fs.exists(&head_ref).await? {
            let content = fs.read_file(&head_ref).await?;
            String::from_utf8_lossy(&content).trim().to_string()
        } else {
            // No commits yet
            return Err(Error::Internal(
                "fatal: not a valid object name: 'master'".to_string(),
            ));
        };

        // Create branch pointing to current commit
        if !fs.exists(&refs_heads).await? {
            fs.mkdir(&refs_heads, true).await?;
        }
        fs.write_file(&branch_path, commit_hash.as_bytes()).await?;

        Ok(())
    }

    /// Delete a branch.
    pub async fn branch_delete(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        name: &str,
    ) -> Result<()> {
        let git_dir = repo_path.join(".git");
        let branch_path = git_dir.join("refs/heads").join(name);

        if !fs.exists(&git_dir).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        // Can't delete current branch
        let current = self.get_current_branch(fs, repo_path).await?;
        if current == name {
            return Err(Error::Internal(format!(
                "error: cannot delete branch '{}' checked out at '{}'",
                name,
                repo_path.display()
            )));
        }

        if !fs.exists(&branch_path).await? {
            return Err(Error::Internal(format!(
                "error: branch '{}' not found",
                name
            )));
        }

        fs.remove(&branch_path, false).await?;
        Ok(())
    }

    /// Checkout a branch or commit.
    pub async fn checkout(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        target: &str,
    ) -> Result<String> {
        let git_dir = repo_path.join(".git");
        let head_path = git_dir.join("HEAD");
        let branch_path = git_dir.join("refs/heads").join(target);

        if !fs.exists(&git_dir).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        // Check if target is a branch
        if fs.exists(&branch_path).await? {
            // Update HEAD to point to branch
            let head_content = format!("ref: refs/heads/{}", target);
            fs.write_file(&head_path, head_content.as_bytes()).await?;
            Ok(format!("Switched to branch '{}'\n", target))
        } else {
            // Check if it's a commit hash (simplified - just check if it looks like a hash)
            if target.len() >= 7 && target.chars().all(|c| c.is_ascii_hexdigit()) {
                // Detached HEAD
                fs.write_file(&head_path, target.as_bytes()).await?;
                Ok(format!(
                    "Note: switching to '{}'.\n\n\
                     You are in 'detached HEAD' state.\n",
                    target
                ))
            } else {
                Err(Error::Internal(format!(
                    "error: pathspec '{}' did not match any file(s) known to git",
                    target
                )))
            }
        }
    }

    /// Show diff between working tree and HEAD (or between commits).
    pub async fn diff(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        _from: Option<&str>,
        _to: Option<&str>,
    ) -> Result<String> {
        let git_dir = repo_path.join(".git");

        if !fs.exists(&git_dir).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        // Simplified diff: just show a placeholder
        // Full diff implementation would require tracking file content hashes
        Ok("# Diff output (simplified in sandbox mode)\n".to_string())
    }

    /// Reset HEAD to a commit.
    pub async fn reset(
        &self,
        fs: &Arc<dyn FileSystem>,
        repo_path: &Path,
        mode: &str,
        target: Option<&str>,
    ) -> Result<String> {
        let git_dir = repo_path.join(".git");

        if !fs.exists(&git_dir).await? {
            return Err(Error::Internal(format!(
                "fatal: not a git repository: {}",
                repo_path.display()
            )));
        }

        let _target = target.unwrap_or("HEAD");

        match mode {
            "--soft" | "--mixed" | "--hard" => {
                // Clear staged files (index)
                let index_path = git_dir.join("index");
                fs.write_file(&index_path, b"").await?;

                Ok(match mode {
                    "--soft" => "".to_string(),
                    "--mixed" => "Unstaged changes after reset:\n".to_string(),
                    "--hard" => "HEAD is now at (reset complete)\n".to_string(),
                    _ => "".to_string(),
                })
            }
            _ => Err(Error::Internal(format!(
                "error: unknown switch `{}`",
                mode.trim_start_matches('-')
            ))),
        }
    }

    /// Format branch list output.
    pub fn format_branch_list(&self, branches: &[Branch]) -> String {
        let mut output = String::new();
        for branch in branches {
            if branch.current {
                output.push_str(&format!("* {}\n", branch.name));
            } else {
                output.push_str(&format!("  {}\n", branch.name));
            }
        }
        output
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;

    async fn create_test_fs() -> Arc<dyn FileSystem> {
        Arc::new(InMemoryFs::new())
    }

    #[tokio::test]
    async fn test_init_creates_git_directory() {
        let fs = create_test_fs().await;
        let client = GitClient::new(GitConfig::new());

        let result = client.init(&fs, Path::new("/repo")).await.unwrap();
        assert!(result.contains("Initialized empty Git repository"));

        assert!(fs.exists(Path::new("/repo/.git")).await.unwrap());
        assert!(fs.exists(Path::new("/repo/.git/HEAD")).await.unwrap());
        assert!(fs.exists(Path::new("/repo/.git/config")).await.unwrap());
        assert!(fs.exists(Path::new("/repo/.git/objects")).await.unwrap());
        assert!(fs.exists(Path::new("/repo/.git/refs")).await.unwrap());
    }

    #[tokio::test]
    async fn test_init_reinitialize() {
        let fs = create_test_fs().await;
        let client = GitClient::new(GitConfig::new());

        client.init(&fs, Path::new("/repo")).await.unwrap();
        let result = client.init(&fs, Path::new("/repo")).await.unwrap();
        assert!(result.contains("Reinitialized existing Git repository"));
    }

    #[tokio::test]
    async fn test_config_get_set() {
        let fs = create_test_fs().await;
        let client = GitClient::new(GitConfig::new());

        client.init(&fs, Path::new("/repo")).await.unwrap();

        // Get existing config
        let name = client
            .config_get(&fs, Path::new("/repo"), "user.name")
            .await
            .unwrap();
        assert_eq!(name, Some("sandbox".to_string()));

        // Set new config
        client
            .config_set(&fs, Path::new("/repo"), "user.name", "Test User")
            .await
            .unwrap();

        let name = client
            .config_get(&fs, Path::new("/repo"), "user.name")
            .await
            .unwrap();
        assert_eq!(name, Some("Test User".to_string()));
    }

    #[tokio::test]
    async fn test_add_and_commit() {
        let fs = create_test_fs().await;
        let client = GitClient::new(GitConfig::new());

        // Create repo and file
        fs.mkdir(Path::new("/repo"), true).await.unwrap();
        client.init(&fs, Path::new("/repo")).await.unwrap();
        fs.write_file(Path::new("/repo/test.txt"), b"hello")
            .await
            .unwrap();

        // Add file
        client
            .add(&fs, Path::new("/repo"), &["test.txt"])
            .await
            .unwrap();

        // Check status
        let status = client.status(&fs, Path::new("/repo")).await.unwrap();
        assert!(status.staged.contains(&"test.txt".to_string()));

        // Commit
        let result = client
            .commit(&fs, Path::new("/repo"), "Initial commit")
            .await
            .unwrap();
        assert!(result.contains("[master"));
        assert!(result.contains("Initial commit"));

        // Check log
        let log = client.log(&fs, Path::new("/repo"), None).await.unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].message, "Initial commit");
    }

    #[tokio::test]
    async fn test_status_untracked_files() {
        let fs = create_test_fs().await;
        let client = GitClient::new(GitConfig::new());

        fs.mkdir(Path::new("/repo"), true).await.unwrap();
        client.init(&fs, Path::new("/repo")).await.unwrap();
        fs.write_file(Path::new("/repo/file1.txt"), b"content1")
            .await
            .unwrap();
        fs.write_file(Path::new("/repo/file2.txt"), b"content2")
            .await
            .unwrap();

        let status = client.status(&fs, Path::new("/repo")).await.unwrap();
        assert!(status.untracked.contains(&"file1.txt".to_string()));
        assert!(status.untracked.contains(&"file2.txt".to_string()));
        assert!(status.staged.is_empty());
    }

    #[tokio::test]
    async fn test_author_from_config() {
        let fs = create_test_fs().await;
        let client = GitClient::new(GitConfig::new().author("Custom Author", "custom@example.com"));

        fs.mkdir(Path::new("/repo"), true).await.unwrap();
        client.init(&fs, Path::new("/repo")).await.unwrap();

        let name = client
            .config_get(&fs, Path::new("/repo"), "user.name")
            .await
            .unwrap();
        assert_eq!(name, Some("Custom Author".to_string()));

        let email = client
            .config_get(&fs, Path::new("/repo"), "user.email")
            .await
            .unwrap();
        assert_eq!(email, Some("custom@example.com".to_string()));
    }
}
