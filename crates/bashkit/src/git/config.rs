//! Git configuration for BashKit.
//!
//! # Security Mitigations
//!
//! This module mitigates the following threats (see `specs/006-threat-model.md`):
//!
//! - **TM-GIT-001**: Unauthorized clone → remote URL allowlist (Phase 2)
//! - **TM-GIT-002**: Host identity leak → configurable sandbox identity
//! - **TM-GIT-003**: Host git config access → no host filesystem access
//! - **TM-GIT-010**: Push to unauthorized remote → remote URL allowlist (Phase 2)

use std::collections::HashSet;

/// Default author name for commits in sandboxed environment.
pub const DEFAULT_AUTHOR_NAME: &str = "sandbox";

/// Default author email for commits in sandboxed environment.
pub const DEFAULT_AUTHOR_EMAIL: &str = "sandbox@bashkit.local";

/// Git configuration for BashKit.
///
/// Controls git behavior including author identity and remote access.
///
/// # Example
///
/// ```rust
/// use bashkit::GitConfig;
///
/// let config = GitConfig::new()
///     .author("Deploy Bot", "deploy@example.com");
/// ```
///
/// # Security
///
/// - Author identity is sandboxed (never reads from host)
/// - Remote URLs require explicit allowlist (Phase 2)
/// - All operations confined to virtual filesystem
#[derive(Debug, Clone)]
pub struct GitConfig {
    /// Author name for commits
    pub(crate) author_name: String,
    /// Author email for commits
    pub(crate) author_email: String,
    /// Remote URL patterns that are allowed (Phase 2)
    pub(crate) remote_allowlist: HashSet<String>,
    /// Allow all remote URLs (dangerous - testing only)
    pub(crate) allow_all_remotes: bool,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            author_name: DEFAULT_AUTHOR_NAME.to_string(),
            author_email: DEFAULT_AUTHOR_EMAIL.to_string(),
            remote_allowlist: HashSet::new(),
            allow_all_remotes: false,
        }
    }
}

impl GitConfig {
    /// Create a new git configuration with default sandbox identity.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::GitConfig;
    ///
    /// let config = GitConfig::new();
    /// // Uses default author: "sandbox <sandbox@bashkit.local>"
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the author name and email for commits.
    ///
    /// # Security (TM-GIT-002)
    ///
    /// This is the only way to set author identity. The git builtin will
    /// never read from host `~/.gitconfig` or environment variables.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::GitConfig;
    ///
    /// let config = GitConfig::new()
    ///     .author("CI Bot", "ci@example.com");
    /// ```
    pub fn author(mut self, name: impl Into<String>, email: impl Into<String>) -> Self {
        self.author_name = name.into();
        self.author_email = email.into();
        self
    }

    /// Add a remote URL pattern to the allowlist (Phase 2).
    ///
    /// Remote operations (clone, push, pull, fetch) require URLs to be
    /// in the allowlist. This method will be used in Phase 2.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::GitConfig;
    ///
    /// let config = GitConfig::new()
    ///     .allow_remote("https://github.com/myorg/");
    /// ```
    pub fn allow_remote(mut self, pattern: impl Into<String>) -> Self {
        self.remote_allowlist.insert(pattern.into());
        self
    }

    /// Add multiple remote URL patterns to the allowlist (Phase 2).
    pub fn allow_remotes(mut self, patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for pattern in patterns {
            self.remote_allowlist.insert(pattern.into());
        }
        self
    }

    /// Allow all remote URLs.
    ///
    /// # Warning
    ///
    /// This is dangerous and should only be used for testing or
    /// when the script is fully trusted.
    pub fn allow_all_remotes(mut self) -> Self {
        self.allow_all_remotes = true;
        self
    }

    /// Get the configured author name.
    pub fn author_name(&self) -> &str {
        &self.author_name
    }

    /// Get the configured author email.
    pub fn author_email(&self) -> &str {
        &self.author_email
    }

    /// Check if remote access is configured.
    #[allow(dead_code)]
    pub(crate) fn has_remote_access(&self) -> bool {
        self.allow_all_remotes || !self.remote_allowlist.is_empty()
    }

    /// Check if a remote URL is allowed.
    ///
    /// # Security (TM-GIT-010, TM-GIT-011)
    ///
    /// Returns true only if:
    /// - allow_all_remotes is true, or
    /// - URL starts with one of the allowed patterns
    ///
    /// # Security (TM-GIT-012, TM-GIT-013)
    ///
    /// Also validates that the URL uses HTTPS (not SSH or git://).
    #[cfg(feature = "git")]
    pub(crate) fn is_url_allowed(&self, url: &str) -> Result<(), String> {
        // TM-GIT-012, TM-GIT-013: Only allow HTTPS
        if !url.starts_with("https://") {
            return Err(format!(
                "error: only HTTPS URLs are allowed (got '{}')\n\
                 hint: SSH and git:// protocols are disabled for security",
                url
            ));
        }

        // Check allowlist
        if self.allow_all_remotes {
            return Ok(());
        }

        if self.remote_allowlist.is_empty() {
            return Err("error: no remote URLs are allowed\n\
                 hint: configure allowed remotes with GitConfig::allow_remote()"
                .to_string());
        }

        // Check if URL starts with any allowed pattern
        for pattern in &self.remote_allowlist {
            if url.starts_with(pattern) {
                return Ok(());
            }
        }

        Err(format!(
            "error: remote URL '{}' is not in allowlist\n\
             hint: configure allowed remotes with GitConfig::allow_remote()",
            url
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = GitConfig::new();
        assert_eq!(config.author_name(), DEFAULT_AUTHOR_NAME);
        assert_eq!(config.author_email(), DEFAULT_AUTHOR_EMAIL);
        assert!(!config.has_remote_access());
    }

    #[test]
    fn test_custom_author() {
        let config = GitConfig::new().author("Test User", "test@example.com");
        assert_eq!(config.author_name(), "Test User");
        assert_eq!(config.author_email(), "test@example.com");
    }

    #[test]
    fn test_remote_allowlist() {
        let config = GitConfig::new()
            .allow_remote("https://github.com/org1/")
            .allow_remote("https://github.com/org2/");

        assert!(config.has_remote_access());
        assert_eq!(config.remote_allowlist.len(), 2);
    }

    #[test]
    fn test_allow_all_remotes() {
        let config = GitConfig::new().allow_all_remotes();
        assert!(config.has_remote_access());
    }

    #[test]
    #[cfg(feature = "git")]
    fn test_url_validation_https_allowed() {
        let config = GitConfig::new().allow_remote("https://github.com/org/");

        // Allowed URL
        assert!(config
            .is_url_allowed("https://github.com/org/repo.git")
            .is_ok());

        // Different org - not allowed
        assert!(config
            .is_url_allowed("https://github.com/other/repo.git")
            .is_err());
    }

    #[test]
    #[cfg(feature = "git")]
    fn test_url_validation_ssh_blocked() {
        let config = GitConfig::new().allow_all_remotes();

        // SSH URLs should be blocked
        assert!(config
            .is_url_allowed("git@github.com:org/repo.git")
            .is_err());
    }

    #[test]
    #[cfg(feature = "git")]
    fn test_url_validation_git_protocol_blocked() {
        let config = GitConfig::new().allow_all_remotes();

        // git:// protocol should be blocked
        assert!(config
            .is_url_allowed("git://github.com/org/repo.git")
            .is_err());
    }

    #[test]
    #[cfg(feature = "git")]
    fn test_url_validation_empty_allowlist() {
        let config = GitConfig::new();

        // Empty allowlist should block all
        assert!(config
            .is_url_allowed("https://github.com/org/repo.git")
            .is_err());
    }

    #[test]
    #[cfg(feature = "git")]
    fn test_url_validation_allow_all() {
        let config = GitConfig::new().allow_all_remotes();

        // All HTTPS URLs should be allowed
        assert!(config
            .is_url_allowed("https://github.com/any/repo.git")
            .is_ok());
        assert!(config
            .is_url_allowed("https://gitlab.com/any/repo.git")
            .is_ok());
    }
}
