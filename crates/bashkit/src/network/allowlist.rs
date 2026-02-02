//! URL allowlist for network access control
//!
//! Provides a whitelist-based security model for network access.

use std::collections::HashSet;
use url::Url;

/// Network allowlist configuration.
///
/// URLs must match an entry in the allowlist to be accessed.
/// An empty allowlist means all URLs are blocked.
#[derive(Debug, Clone, Default)]
pub struct NetworkAllowlist {
    /// URL patterns that are allowed
    /// Format: "scheme://host[:port][/path]"
    /// Examples: "https://api.example.com", "https://example.com/api"
    patterns: HashSet<String>,

    /// If true, allow all URLs (dangerous - use only for testing)
    allow_all: bool,
}

/// Result of matching a URL against the allowlist
#[derive(Debug, Clone, PartialEq)]
pub enum UrlMatch {
    /// URL is allowed
    Allowed,
    /// URL is blocked (not in allowlist)
    Blocked { reason: String },
    /// URL is invalid
    Invalid { reason: String },
}

impl NetworkAllowlist {
    /// Create a new empty allowlist (blocks all URLs)
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an allowlist that allows all URLs.
    ///
    /// # Warning
    ///
    /// This is dangerous and should only be used for testing or
    /// when the script is fully trusted.
    pub fn allow_all() -> Self {
        Self {
            patterns: HashSet::new(),
            allow_all: true,
        }
    }

    /// Add a URL pattern to the allowlist.
    ///
    /// # Pattern Format
    ///
    /// Patterns can be:
    /// - Full URLs: "https://api.example.com/v1"
    /// - Host only: "https://example.com"
    /// - With port: "http://localhost:8080"
    ///
    /// A pattern matches if the requested URL's scheme, host, and port match,
    /// and the requested path starts with the pattern's path (if specified).
    pub fn allow(mut self, pattern: impl Into<String>) -> Self {
        self.patterns.insert(pattern.into());
        self
    }

    /// Add multiple URL patterns to the allowlist.
    pub fn allow_many(mut self, patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for pattern in patterns {
            self.patterns.insert(pattern.into());
        }
        self
    }

    /// Check if a URL is allowed.
    pub fn check(&self, url: &str) -> UrlMatch {
        // Allow all if configured
        if self.allow_all {
            return UrlMatch::Allowed;
        }

        // Empty allowlist blocks everything
        if self.patterns.is_empty() {
            return UrlMatch::Blocked {
                reason: "no URLs are allowed (empty allowlist)".to_string(),
            };
        }

        // Parse the URL
        let parsed = match Url::parse(url) {
            Ok(u) => u,
            Err(e) => {
                return UrlMatch::Invalid {
                    reason: format!("invalid URL: {}", e),
                }
            }
        };

        // Check against each pattern
        for pattern in &self.patterns {
            if self.matches_pattern(&parsed, pattern) {
                return UrlMatch::Allowed;
            }
        }

        UrlMatch::Blocked {
            reason: format!("URL not in allowlist: {}", url),
        }
    }

    /// Check if a parsed URL matches a pattern.
    fn matches_pattern(&self, url: &Url, pattern: &str) -> bool {
        // Parse the pattern as a URL
        let pattern_url = match Url::parse(pattern) {
            Ok(u) => u,
            Err(_) => return false,
        };

        // Check scheme
        if url.scheme() != pattern_url.scheme() {
            return false;
        }

        // Check host
        match (url.host_str(), pattern_url.host_str()) {
            (Some(url_host), Some(pattern_host)) => {
                if url_host != pattern_host {
                    return false;
                }
            }
            _ => return false,
        }

        // Check port (use default ports if not specified)
        let url_port = url.port_or_known_default();
        let pattern_port = pattern_url.port_or_known_default();
        if url_port != pattern_port {
            return false;
        }

        // Check path prefix (pattern path must be prefix of URL path)
        let pattern_path = pattern_url.path();
        let url_path = url.path();

        // If pattern path is "/" or empty, match any path
        if pattern_path == "/" || pattern_path.is_empty() {
            return true;
        }

        // URL path must start with pattern path
        if !url_path.starts_with(pattern_path) {
            return false;
        }

        // If pattern path doesn't end with /, ensure we're at a path boundary
        if !pattern_path.ends_with('/') && url_path.len() > pattern_path.len() {
            let next_char = url_path.chars().nth(pattern_path.len());
            if next_char != Some('/') && next_char != Some('?') && next_char != Some('#') {
                return false;
            }
        }

        true
    }

    /// Check if network access is enabled (has any patterns or allow_all)
    pub fn is_enabled(&self) -> bool {
        self.allow_all || !self.patterns.is_empty()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_allowlist_blocks_all() {
        let allowlist = NetworkAllowlist::new();
        assert!(matches!(
            allowlist.check("https://example.com"),
            UrlMatch::Blocked { .. }
        ));
    }

    #[test]
    fn test_allow_all() {
        let allowlist = NetworkAllowlist::allow_all();
        assert_eq!(allowlist.check("https://example.com"), UrlMatch::Allowed);
        assert_eq!(
            allowlist.check("http://localhost:8080/anything"),
            UrlMatch::Allowed
        );
    }

    #[test]
    fn test_exact_host_match() {
        let allowlist = NetworkAllowlist::new().allow("https://api.example.com");

        assert_eq!(
            allowlist.check("https://api.example.com"),
            UrlMatch::Allowed
        );
        assert_eq!(
            allowlist.check("https://api.example.com/"),
            UrlMatch::Allowed
        );
        assert_eq!(
            allowlist.check("https://api.example.com/v1/users"),
            UrlMatch::Allowed
        );

        // Different scheme
        assert!(matches!(
            allowlist.check("http://api.example.com"),
            UrlMatch::Blocked { .. }
        ));

        // Different host
        assert!(matches!(
            allowlist.check("https://other.example.com"),
            UrlMatch::Blocked { .. }
        ));
    }

    #[test]
    fn test_path_prefix_match() {
        let allowlist = NetworkAllowlist::new().allow("https://api.example.com/v1");

        // Matches path prefix
        assert_eq!(
            allowlist.check("https://api.example.com/v1"),
            UrlMatch::Allowed
        );
        assert_eq!(
            allowlist.check("https://api.example.com/v1/"),
            UrlMatch::Allowed
        );
        assert_eq!(
            allowlist.check("https://api.example.com/v1/users"),
            UrlMatch::Allowed
        );

        // Does not match different path
        assert!(matches!(
            allowlist.check("https://api.example.com/v2"),
            UrlMatch::Blocked { .. }
        ));

        // Does not match partial path component
        assert!(matches!(
            allowlist.check("https://api.example.com/v10"),
            UrlMatch::Blocked { .. }
        ));
    }

    #[test]
    fn test_port_matching() {
        let allowlist = NetworkAllowlist::new().allow("http://localhost:8080");

        assert_eq!(
            allowlist.check("http://localhost:8080/api"),
            UrlMatch::Allowed
        );

        // Different port
        assert!(matches!(
            allowlist.check("http://localhost:3000"),
            UrlMatch::Blocked { .. }
        ));

        // Default HTTP port
        assert!(matches!(
            allowlist.check("http://localhost"),
            UrlMatch::Blocked { .. }
        ));
    }

    #[test]
    fn test_multiple_patterns() {
        let allowlist = NetworkAllowlist::new()
            .allow("https://api.example.com")
            .allow("https://cdn.example.com")
            .allow("http://localhost:3000");

        assert_eq!(
            allowlist.check("https://api.example.com/v1"),
            UrlMatch::Allowed
        );
        assert_eq!(
            allowlist.check("https://cdn.example.com/assets/logo.png"),
            UrlMatch::Allowed
        );
        assert_eq!(
            allowlist.check("http://localhost:3000/health"),
            UrlMatch::Allowed
        );

        assert!(matches!(
            allowlist.check("https://evil.com"),
            UrlMatch::Blocked { .. }
        ));
    }

    #[test]
    fn test_invalid_url() {
        let allowlist = NetworkAllowlist::new().allow("https://example.com");

        assert!(matches!(
            allowlist.check("not a url"),
            UrlMatch::Invalid { .. }
        ));
    }

    #[test]
    fn test_is_enabled() {
        let empty = NetworkAllowlist::new();
        assert!(!empty.is_enabled());

        let with_pattern = NetworkAllowlist::new().allow("https://example.com");
        assert!(with_pattern.is_enabled());

        let allow_all = NetworkAllowlist::allow_all();
        assert!(allow_all.is_enabled());
    }
}
