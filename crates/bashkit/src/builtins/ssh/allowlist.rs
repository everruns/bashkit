//! Host allowlist for SSH access control.
//!
//! Provides a whitelist-based security model for SSH connections.
//!
//! # Security Mitigations
//!
//! - **TM-SSH-001**: Unauthorized host access → host allowlist (default-deny)
//! - **TM-SSH-007**: Port scanning → port allowlist

use std::collections::HashSet;

/// SSH host allowlist configuration.
///
/// Hosts must match an entry in the allowlist to be connected to.
/// An empty allowlist means all hosts are blocked (secure by default).
///
/// # Examples
///
/// ```rust
/// use bashkit::SshAllowlist;
///
/// let allowlist = SshAllowlist::new()
///     .allow("db.abc123.supabase.co")
///     .allow("*.example.com");
///
/// assert!(allowlist.is_allowed("db.abc123.supabase.co", 22));
/// assert!(allowlist.is_allowed("staging.example.com", 22));
/// assert!(!allowlist.is_allowed("evil.com", 22));
/// ```
///
/// # Pattern Matching
///
/// - **Exact host**: `db.abc123.supabase.co`
/// - **Wildcard subdomain**: `*.supabase.co` matches `db.abc.supabase.co`
/// - **IP address**: `192.168.1.100`
/// - **Port check**: Host must be allowed AND port must be in allowed set
#[derive(Debug, Clone, Default)]
pub struct SshAllowlist {
    /// Host patterns that are allowed.
    /// Supports exact match and `*.domain.com` wildcard patterns.
    patterns: HashSet<String>,

    /// Allowed ports. Empty means default port 22 only.
    allowed_ports: HashSet<u16>,

    /// If true, allow all hosts (dangerous - testing only).
    allow_all: bool,
}

/// Result of matching a host against the allowlist.
#[derive(Debug, Clone, PartialEq)]
pub enum SshMatch {
    /// Host and port are allowed.
    Allowed,
    /// Host or port is blocked.
    Blocked { reason: String },
}

impl SshAllowlist {
    /// Create a new empty allowlist (blocks all hosts).
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an allowlist that allows all hosts.
    ///
    /// # Warning
    ///
    /// This is dangerous and should only be used for testing.
    pub fn allow_all() -> Self {
        Self {
            patterns: HashSet::new(),
            allowed_ports: HashSet::new(),
            allow_all: true,
        }
    }

    /// Add a host pattern to the allowlist.
    ///
    /// Patterns can be:
    /// - Exact host: `db.abc123.supabase.co`
    /// - Wildcard subdomain: `*.supabase.co`
    /// - IP address: `192.168.1.100`
    pub fn allow(mut self, pattern: impl Into<String>) -> Self {
        self.patterns.insert(pattern.into());
        self
    }

    /// Add multiple host patterns.
    pub fn allow_many(mut self, patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for p in patterns {
            self.patterns.insert(p.into());
        }
        self
    }

    /// Add an allowed port. If no ports are added, only port 22 is allowed.
    pub fn allow_port(mut self, port: u16) -> Self {
        self.allowed_ports.insert(port);
        self
    }

    /// Check if a host + port combination is allowed.
    pub fn check(&self, host: &str, port: u16) -> SshMatch {
        if self.allow_all {
            return SshMatch::Allowed;
        }

        // Check port first
        if !self.is_port_allowed(port) {
            return SshMatch::Blocked {
                reason: format!("SSH port {} is not allowed", port),
            };
        }

        // Empty allowlist blocks everything
        if self.patterns.is_empty() {
            return SshMatch::Blocked {
                reason: "no SSH hosts are allowed (empty allowlist)".to_string(),
            };
        }

        // Check host against patterns
        for pattern in &self.patterns {
            if Self::matches_pattern(host, pattern) {
                return SshMatch::Allowed;
            }
        }

        SshMatch::Blocked {
            reason: format!("SSH host '{}' is not in allowlist", host),
        }
    }

    /// Convenience method: is this host+port allowed?
    pub fn is_allowed(&self, host: &str, port: u16) -> bool {
        matches!(self.check(host, port), SshMatch::Allowed)
    }

    /// Check if network access is enabled.
    pub fn is_enabled(&self) -> bool {
        self.allow_all || !self.patterns.is_empty()
    }

    fn is_port_allowed(&self, port: u16) -> bool {
        if self.allow_all {
            return true;
        }
        if self.allowed_ports.is_empty() {
            // Default: only port 22
            return port == 22;
        }
        self.allowed_ports.contains(&port)
    }

    /// Match a hostname against a pattern.
    ///
    /// - Exact match: `host == pattern`
    /// - Wildcard: `*.domain.com` matches `any.domain.com` and `deep.any.domain.com`
    fn matches_pattern(host: &str, pattern: &str) -> bool {
        if host == pattern {
            return true;
        }

        // Wildcard pattern: *.domain.com
        if let Some(suffix) = pattern.strip_prefix("*.") {
            // Host must end with .suffix and have at least one char before the dot
            if let Some(prefix) = host.strip_suffix(suffix) {
                // prefix should end with '.' (e.g., "db." from "db.supabase.co")
                return prefix.ends_with('.');
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_allowlist_blocks_all() {
        let allowlist = SshAllowlist::new();
        assert!(matches!(
            allowlist.check("example.com", 22),
            SshMatch::Blocked { .. }
        ));
    }

    #[test]
    fn test_allow_all() {
        let allowlist = SshAllowlist::allow_all();
        assert_eq!(allowlist.check("anything.com", 22), SshMatch::Allowed);
        assert_eq!(allowlist.check("anything.com", 2222), SshMatch::Allowed);
    }

    #[test]
    fn test_exact_host_match() {
        let allowlist = SshAllowlist::new().allow("db.supabase.co");

        assert_eq!(allowlist.check("db.supabase.co", 22), SshMatch::Allowed);
        assert!(matches!(
            allowlist.check("other.supabase.co", 22),
            SshMatch::Blocked { .. }
        ));
        assert!(matches!(
            allowlist.check("evil.com", 22),
            SshMatch::Blocked { .. }
        ));
    }

    #[test]
    fn test_wildcard_pattern() {
        let allowlist = SshAllowlist::new().allow("*.supabase.co");

        assert_eq!(allowlist.check("db.supabase.co", 22), SshMatch::Allowed);
        assert_eq!(
            allowlist.check("staging.supabase.co", 22),
            SshMatch::Allowed
        );
        assert_eq!(
            allowlist.check("deep.nested.supabase.co", 22),
            SshMatch::Allowed
        );

        // Must have subdomain
        assert!(matches!(
            allowlist.check("supabase.co", 22),
            SshMatch::Blocked { .. }
        ));
        assert!(matches!(
            allowlist.check("evil.com", 22),
            SshMatch::Blocked { .. }
        ));
    }

    #[test]
    fn test_port_restriction_default() {
        let allowlist = SshAllowlist::new().allow("example.com");

        // Default: only port 22
        assert_eq!(allowlist.check("example.com", 22), SshMatch::Allowed);
        assert!(matches!(
            allowlist.check("example.com", 2222),
            SshMatch::Blocked { .. }
        ));
    }

    #[test]
    fn test_port_restriction_custom() {
        let allowlist = SshAllowlist::new()
            .allow("example.com")
            .allow_port(22)
            .allow_port(2222);

        assert_eq!(allowlist.check("example.com", 22), SshMatch::Allowed);
        assert_eq!(allowlist.check("example.com", 2222), SshMatch::Allowed);
        assert!(matches!(
            allowlist.check("example.com", 3333),
            SshMatch::Blocked { .. }
        ));
    }

    #[test]
    fn test_ip_address() {
        let allowlist = SshAllowlist::new().allow("192.168.1.100");
        assert_eq!(allowlist.check("192.168.1.100", 22), SshMatch::Allowed);
        assert!(matches!(
            allowlist.check("192.168.1.101", 22),
            SshMatch::Blocked { .. }
        ));
    }

    #[test]
    fn test_multiple_patterns() {
        let allowlist = SshAllowlist::new()
            .allow("*.supabase.co")
            .allow("bastion.example.com")
            .allow("10.0.0.1");

        assert_eq!(allowlist.check("db.supabase.co", 22), SshMatch::Allowed);
        assert_eq!(
            allowlist.check("bastion.example.com", 22),
            SshMatch::Allowed
        );
        assert_eq!(allowlist.check("10.0.0.1", 22), SshMatch::Allowed);
        assert!(matches!(
            allowlist.check("evil.com", 22),
            SshMatch::Blocked { .. }
        ));
    }

    #[test]
    fn test_is_enabled() {
        assert!(!SshAllowlist::new().is_enabled());
        assert!(SshAllowlist::new().allow("x.com").is_enabled());
        assert!(SshAllowlist::allow_all().is_enabled());
    }
}
