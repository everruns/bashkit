//! URL allowlist for network access control.
//!
//! Provides a whitelist-based security model for network access.
//!
//! # Security Mitigations
//!
//! This module mitigates the following threats (see `knowledge/security/threat-model.md`):
//!
//! - **TM-NET-001**: DNS spoofing → literal host matching, no DNS resolution
//! - **TM-NET-002**: DNS rebinding → allowlist uses literal strings; the
//!   reqwest client also installs `PrivateIpFilteringResolver` so private
//!   IPs are refused at connect time even if DNS rebinds between checks.
//! - **TM-NET-004**: IP-based bypass → IPs must be explicitly allowed
//! - **TM-NET-005**: Port scanning → port must match allowlist entry
//! - **TM-NET-006**: Protocol downgrade → scheme must match exactly
//! - **TM-NET-007**: Subdomain bypass → exact host match required
//! - **TM-INF-010**: Data exfiltration → default-deny blocks unauthorized destinations

use std::collections::HashSet;
use std::net::IpAddr;
use url::Url;

/// Check if an IP address is in a private/reserved range.
///
/// Blocks every range that is not globally reachable unicast:
/// - v4: 0.0.0.0/8, 10.0.0.0/8, 100.64.0.0/10, 127.0.0.0/8, 169.254.0.0/16,
///   172.16.0.0/12, 192.0.0.0/24, 192.168.0.0/16, 198.18.0.0/15,
///   224.0.0.0/4 (multicast), 240.0.0.0/4 (reserved + broadcast)
/// - v6: ::, ::1, 100::/64, 2001::/32 (Teredo), fc00::/7, fe80::/10,
///   fec0::/10, ff00::/8, 64:ff9b:1::/48 (local-use NAT64)
///
/// Documentation ranges (192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24,
/// 2001:db8::/32) are not blocked: they are unroutable, and tests use them
/// as stand-in public addresses.
///
/// v6 forms that carry a v4 address are classified by that v4 address:
/// IPv4-mapped (`::ffff:0:0/96`), IPv4-compatible (`::a.b.c.d`), NAT64
/// (`64:ff9b::/96`, RFC 6052) and 6to4 (`2002::/16`, RFC 3056). This is
/// critical: without it, an attacker who controls DNS for an allowlisted
/// hostname can return an AAAA record such as `::ffff:127.0.0.1`,
/// `64:ff9b::a9fe:a9fe` or `2002:7f00:1::` and reach loopback or cloud
/// metadata past the v4 checks.
///
/// # Security (TM-NET-002, TM-NET-004, TM-NET-008, TM-NET-029)
///
/// Used to prevent SSRF attacks where an allowed hostname resolves
/// to an internal/cloud metadata IP address.
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_ipv4(v4),
        IpAddr::V6(v6) => {
            // SECURITY (TM-NET-008, #1569): IPv4-mapped IPv6 addresses
            // (`::ffff:a.b.c.d`) and IPv4-compatible addresses
            // (`::a.b.c.d`, deprecated but still resolvable) re-enter the
            // address space as v4 at the kernel/socket layer. Reject them
            // by their v4 classification before applying v6-only rules.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_ipv4(&v4);
            }
            let seg = v6.segments();
            if let Some(v4) = v6.to_ipv4()
                && seg[..6].iter().all(|s| *s == 0)
                && seg[6] != 0
            {
                // ::a.b.c.d (IPv4-compatible) — apply v4 rules.
                return is_private_ipv4(&v4);
            }
            // SECURITY (TM-NET-029): NAT64 and 6to4 gateways forward to the
            // embedded v4 address, so classify by it.
            if seg[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                let o = v6.octets();
                return is_private_ipv4(&std::net::Ipv4Addr::new(o[12], o[13], o[14], o[15]));
            }
            if seg[0] == 0x2002 {
                let o = v6.octets();
                return is_private_ipv4(&std::net::Ipv4Addr::new(o[2], o[3], o[4], o[5]));
            }
            v6.is_loopback()                    // ::1
                || v6.is_unspecified()          // ::
                || seg[..4] == [0x100, 0, 0, 0] // 100::/64 (discard-only)
                || seg[..2] == [0x2001, 0]      // 2001::/32 (Teredo, embeds obfuscated v4)
                || seg[..3] == [0x64, 0xff9b, 1] // 64:ff9b:1::/48 (local-use NAT64)
                || (seg[0] & 0xfe00) == 0xfc00  // fc00::/7 (unique local)
                || (seg[0] & 0xffc0) == 0xfe80  // fe80::/10 (link-local)
                || (seg[0] & 0xffc0) == 0xfec0  // fec0::/10 (deprecated site-local)
                || (seg[0] & 0xff00) == 0xff00 // ff00::/8 (multicast)
        }
    }
}

fn is_private_ipv4(v4: &std::net::Ipv4Addr) -> bool {
    let o = v4.octets();
    o[0] == 0                           // 0.0.0.0/8 ("this network")
        || v4.is_loopback()             // 127.0.0.0/8
        || v4.is_private()              // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
        || v4.is_link_local()           // 169.254.0.0/16
        || o[0] == 100 && (o[1] & 0xC0) == 64 // 100.64.0.0/10 (CGNAT)
        || o[0] == 192 && o[1] == 0 && o[2] == 0 // 192.0.0.0/24 (IETF protocol assignments)
        || o[0] == 198 && (o[1] & 0xFE) == 18 // 198.18.0.0/15 (benchmarking)
        || o[0] >= 224 // 224.0.0.0/4 multicast, 240.0.0.0/4 reserved, broadcast
}

/// Redact credentials from a URL for safe inclusion in error messages.
/// Replaces `user:pass@` in the authority with `***@`.
fn redact_url(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut parsed) => {
            if !parsed.username().is_empty() || parsed.password().is_some() {
                let _ = parsed.set_username("***");
                let _ = parsed.set_password(None);
            }
            parsed.to_string()
        }
        Err(_) => "[invalid URL]".to_string(),
    }
}

/// Network allowlist configuration for controlling HTTP access.
///
/// URLs must match an entry in the allowlist to be accessed.
/// An empty allowlist means all URLs are blocked (secure by default).
///
/// # Examples
///
/// ```rust
/// use bashkit::NetworkAllowlist;
///
/// // Create allowlist for specific APIs
/// let allowlist = NetworkAllowlist::new()
///     .allow("https://api.example.com")        // Allow entire host
///     .allow("https://cdn.example.com/assets/"); // Allow path prefix
///
/// // Check URLs
/// assert!(allowlist.is_allowed("https://api.example.com/v1/users"));
/// assert!(allowlist.is_allowed("https://cdn.example.com/assets/logo.png"));
/// assert!(!allowlist.is_allowed("https://evil.com"));
/// ```
///
/// # Pattern Matching
///
/// - **Scheme**: Must match exactly (https vs http)
/// - **Host**: Must match exactly (no wildcards)
/// - **Port**: Must match (defaults apply: 443 for https, 80 for http)
/// - **Path**: Pattern path is treated as a prefix
#[derive(Debug, Clone)]
pub struct NetworkAllowlist {
    /// URL patterns that are allowed
    /// Format: "scheme://host[:port][/path]"
    /// Examples: `https://api.example.com`, `https://example.com/api`
    patterns: HashSet<String>,

    /// If true, allow all URLs (dangerous - use only for testing)
    allow_all: bool,

    /// THREAT[TM-NET-002/004]: Block requests to private/reserved IP ranges.
    /// Default: true. Prevents SSRF via DNS rebinding.
    block_private_ips: bool,
}

impl Default for NetworkAllowlist {
    fn default() -> Self {
        Self {
            patterns: HashSet::new(),
            allow_all: false,
            block_private_ips: true,
        }
    }
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
            block_private_ips: true,
        }
    }

    /// Block requests to private/reserved IP ranges (default: true).
    ///
    /// When enabled, requests to hostnames that resolve to private IPs
    /// (127.0.0.0/8, 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16,
    /// 169.254.0.0/16, ::1, fd00::/8, fe80::/10) are blocked.
    ///
    /// # Security (TM-NET-002, TM-NET-004)
    ///
    /// Prevents SSRF via DNS rebinding where an allowed hostname
    /// resolves to an internal IP address.
    pub fn block_private_ips(mut self, block: bool) -> Self {
        self.block_private_ips = block;
        self
    }

    /// Returns whether private IP blocking is enabled.
    pub fn is_blocking_private_ips(&self) -> bool {
        self.block_private_ips
    }

    /// Add a URL pattern to the allowlist.
    ///
    /// # Pattern Format
    ///
    /// Patterns can be:
    /// - Full URLs: `https://api.example.com/v1`
    /// - Host only: `https://example.com`
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
                };
            }
        };

        // THREAT[TM-NET-002]: Block requests to private IPs in the hostname.
        // If the hostname is a literal IP address, check it immediately.
        if self.block_private_ips
            && let Some(host) = parsed.host_str()
            && let Ok(ip) = host.parse::<IpAddr>()
            && is_private_ip(&ip)
        {
            return UrlMatch::Blocked {
                reason: format!(
                    "request to private/reserved IP {} blocked (SSRF protection)",
                    ip
                ),
            };
        }

        // Check against each pattern
        for pattern in &self.patterns {
            if self.matches_pattern(&parsed, pattern) {
                return UrlMatch::Allowed;
            }
        }

        UrlMatch::Blocked {
            reason: format!("URL not in allowlist: {}", redact_url(url)),
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
        // Use byte indexing consistently since url_path.len() and pattern_path.len()
        // are both byte counts, and starts_with already confirmed the prefix matches.
        if !pattern_path.ends_with('/') && url_path.len() > pattern_path.len() {
            let next_char = url_path
                .as_bytes()
                .get(pattern_path.len())
                .map(|&b| b as char);
            if next_char != Some('/') && next_char != Some('?') && next_char != Some('#') {
                return false;
            }
        }

        true
    }

    /// Check if a URL is allowed (convenience method).
    ///
    /// Returns `true` if the URL is allowed, `false` otherwise.
    /// This is equivalent to checking if `check(url)` returns `UrlMatch::Allowed`.
    pub fn is_allowed(&self, url: &str) -> bool {
        matches!(self.check(url), UrlMatch::Allowed)
    }

    /// Check if network access is enabled (has any patterns or allow_all)
    pub fn is_enabled(&self) -> bool {
        self.allow_all || !self.patterns.is_empty()
    }
}

#[cfg(test)]
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

    #[test]
    fn test_redact_url_strips_credentials() {
        let redacted = redact_url("https://user:secret@example.com/path");
        assert!(
            !redacted.contains("secret"),
            "password leaked: {}",
            redacted
        );
        assert!(!redacted.contains("user"), "username leaked: {}", redacted);
        assert!(redacted.contains("example.com/path"));
    }

    #[test]
    fn test_redact_url_preserves_clean_url() {
        let clean = "https://example.com/path?q=1";
        assert_eq!(redact_url(clean), clean);
    }

    #[test]
    fn test_blocked_message_no_credentials() {
        let allowlist = NetworkAllowlist::new().allow("https://allowed.com");
        let result = allowlist.check("https://user:pass@blocked.com/api");
        match result {
            UrlMatch::Blocked { reason } => {
                assert!(!reason.contains("pass"), "credentials leaked: {}", reason);
            }
            _ => panic!("expected Blocked"),
        }
    }

    #[test]
    fn test_path_boundary_check_byte_safe() {
        // Ensure path boundary check uses byte-safe indexing
        let allowlist = NetworkAllowlist::new().allow("https://example.com/api");
        // /api/v1 should be allowed (starts with /api and next char is /)
        assert!(matches!(
            allowlist.check("https://example.com/api/v1"),
            UrlMatch::Allowed
        ));
        // /apix should be blocked (not at path boundary)
        assert!(matches!(
            allowlist.check("https://example.com/apix"),
            UrlMatch::Blocked { .. }
        ));
    }

    // === Private IP tests (TM-NET-002/004) ===

    #[test]
    fn test_is_private_ip_loopback() {
        assert!(is_private_ip(&"127.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"127.0.0.2".parse().unwrap()));
        assert!(is_private_ip(&"::1".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_rfc1918() {
        assert!(is_private_ip(&"10.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"10.255.255.255".parse().unwrap()));
        assert!(is_private_ip(&"172.16.0.1".parse().unwrap()));
        assert!(is_private_ip(&"172.31.255.255".parse().unwrap()));
        assert!(is_private_ip(&"192.168.0.1".parse().unwrap()));
        assert!(is_private_ip(&"192.168.255.255".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_link_local() {
        assert!(is_private_ip(&"169.254.0.1".parse().unwrap()));
        assert!(is_private_ip(&"169.254.169.254".parse().unwrap())); // AWS metadata
    }

    #[test]
    fn test_is_private_ip_public() {
        assert!(!is_private_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip(&"1.1.1.1".parse().unwrap()));
        assert!(!is_private_ip(&"203.0.113.1".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_v6() {
        assert!(is_private_ip(&"::1".parse().unwrap()));
        assert!(is_private_ip(&"fd00::1".parse().unwrap()));
        assert!(is_private_ip(&"fe80::1".parse().unwrap()));
        assert!(!is_private_ip(
            &"2001:db8::1".parse::<std::net::IpAddr>().unwrap()
        ));
    }

    #[test]
    fn test_is_private_ip_nat64_embeds_v4() {
        // NAT64 (RFC 6052 well-known prefix 64:ff9b::/96) routes to the
        // embedded v4 address: 64:ff9b::7f00:1 reaches 127.0.0.1 on a
        // NAT64 network. Classify by the embedded address.
        assert!(is_private_ip(&"64:ff9b::7f00:1".parse().unwrap()));
        assert!(is_private_ip(&"64:ff9b::a9fe:a9fe".parse().unwrap())); // 169.254.169.254
        assert!(is_private_ip(&"64:ff9b::a00:1".parse().unwrap())); // 10.0.0.1
        assert!(!is_private_ip(&"64:ff9b::808:808".parse().unwrap())); // 8.8.8.8
        // RFC 8215 local-use NAT64 prefix: translator is site-specific.
        assert!(is_private_ip(&"64:ff9b:1::808:808".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_6to4_embeds_v4() {
        // 6to4 (2002::/16) carries a v4 address in bits 16..48.
        assert!(is_private_ip(&"2002:7f00:1::1".parse().unwrap())); // 127.0.0.1
        assert!(is_private_ip(&"2002:a9fe:a9fe::".parse().unwrap())); // 169.254.169.254
        assert!(is_private_ip(&"2002:c0a8:101::1".parse().unwrap())); // 192.168.1.1
        assert!(!is_private_ip(&"2002:808:808::1".parse().unwrap())); // 8.8.8.8
    }

    #[test]
    fn test_is_private_ip_reserved_v4_ranges() {
        assert!(is_private_ip(&"0.1.2.3".parse().unwrap())); // 0.0.0.0/8 "this network"
        assert!(is_private_ip(&"192.0.0.8".parse().unwrap())); // 192.0.0.0/24 IETF assignments
        assert!(is_private_ip(&"198.18.0.1".parse().unwrap())); // 198.18.0.0/15 benchmarking
        assert!(is_private_ip(&"198.19.255.255".parse().unwrap()));
        assert!(is_private_ip(&"224.0.0.1".parse().unwrap())); // multicast
        assert!(is_private_ip(&"239.255.255.250".parse().unwrap()));
        assert!(is_private_ip(&"240.0.0.1".parse().unwrap())); // reserved
        assert!(is_private_ip(&"255.255.255.255".parse().unwrap())); // broadcast
        assert!(!is_private_ip(&"198.20.0.1".parse().unwrap()));
        assert!(!is_private_ip(&"223.255.255.255".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_reserved_v6_ranges() {
        assert!(is_private_ip(&"ff02::1".parse().unwrap())); // multicast
        assert!(is_private_ip(&"fec0::1".parse().unwrap())); // deprecated site-local
        assert!(is_private_ip(&"fc00::1".parse().unwrap())); // ULA, fc00::/7 lower half
        assert!(is_private_ip(&"100::1".parse().unwrap())); // discard-only 100::/64
        assert!(is_private_ip(&"2001::1".parse().unwrap())); // Teredo 2001::/32
        assert!(!is_private_ip(&"2001:4860:4860::8888".parse().unwrap()));
        assert!(!is_private_ip(&"2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_v4_mapped_v6_loopback() {
        // Regression for #1569 (TM-NET-008): IPv4-mapped IPv6 addresses
        // must be normalized and classified by the embedded v4 address.
        // Without normalization, an AAAA record returning `::ffff:127.0.0.1`
        // would bypass the v4 loopback check and reach localhost.
        assert!(is_private_ip(&"::ffff:127.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"::ffff:127.255.255.255".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_v4_mapped_v6_rfc1918() {
        // Regression for #1569: AAAA → ::ffff:10.0.0.1 / ::ffff:192.168.x.y
        // must trip the v4 private-range check.
        assert!(is_private_ip(&"::ffff:10.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"::ffff:172.16.0.1".parse().unwrap()));
        assert!(is_private_ip(&"::ffff:192.168.1.1".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_v4_mapped_v6_metadata() {
        // Regression for #1569: ::ffff:169.254.169.254 (AWS metadata) is
        // the highest-impact bypass and must classify as private.
        assert!(is_private_ip(&"::ffff:169.254.169.254".parse().unwrap()));
        assert!(is_private_ip(&"::ffff:169.254.0.1".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_v4_mapped_v6_cgnat() {
        // CGNAT range coverage carries through normalization too.
        assert!(is_private_ip(&"::ffff:100.64.0.1".parse().unwrap()));
        assert!(is_private_ip(&"::ffff:100.127.255.255".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_v4_mapped_v6_unspecified() {
        assert!(is_private_ip(&"::ffff:0.0.0.0".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_v4_mapped_v6_public_still_public() {
        // Public IPv4 wrapped as v4-mapped v6 must still be public.
        assert!(!is_private_ip(&"::ffff:8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip(&"::ffff:1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_v4_compatible_v6_loopback() {
        // IPv4-compatible IPv6 (`::a.b.c.d`, deprecated but still parsable)
        // also needs v4 classification.
        assert!(is_private_ip(&"::127.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"::169.254.169.254".parse().unwrap()));
    }

    #[test]
    fn test_block_private_ips_default_true() {
        let al = NetworkAllowlist::new();
        assert!(al.is_blocking_private_ips());
    }

    #[test]
    fn test_block_private_ips_disabled() {
        let al = NetworkAllowlist::new().block_private_ips(false);
        assert!(!al.is_blocking_private_ips());
    }
}
