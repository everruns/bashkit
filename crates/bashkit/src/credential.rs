// Decision: Two modes — injection (script unaware) and placeholder (opaque env var replaced on the wire).
// Decision: Header-only for v1 — no URL query param or body mutation.
// Decision: Overwrite semantics — injected headers replace existing headers with same name.
// Decision: Non-blocking — injection failures don't block the request.
// Decision: Built on before_http hooks — no new interception points.
// See specs/credential-injection.md

//! Generic credential injection for outbound HTTP requests.
//!
//! Provides transparent per-host credential injection so sandboxed scripts
//! can make authenticated API calls without ever seeing the real secrets.
//!
//! Two modes are supported:
//!
//! - **Injection**: Script has no knowledge of credentials. Headers are added
//!   automatically based on the request URL.
//! - **Placeholder**: Script sees an opaque placeholder in an env var. The
//!   placeholder is replaced with the real credential in outbound headers.
//!
//! See [`crate::credential_injection_guide`] for the full guide.

use crate::hooks::{HookAction, HttpRequestEvent, Interceptor};
use crate::network::NetworkAllowlist;

/// A credential to inject into outbound HTTP requests.
///
/// # Examples
///
/// ```rust
/// use bashkit::Credential;
///
/// // Bearer token
/// let cred = Credential::bearer("ghp_xxxx");
///
/// // Custom header
/// let cred = Credential::header("X-Api-Key", "secret123");
///
/// // Multiple headers
/// let cred = Credential::headers(vec![
///     ("X-Api-Key".into(), "key123".into()),
///     ("X-Api-Secret".into(), "secret456".into()),
/// ]);
/// ```
#[derive(Clone)]
pub enum Credential {
    /// Inject `Authorization: Bearer <token>`.
    Bearer(String),
    /// Inject a single custom header.
    Header {
        /// Header name.
        name: String,
        /// Header value (the secret).
        value: String,
    },
    /// Inject multiple headers.
    Headers(Vec<(String, String)>),
}

impl Credential {
    /// Create a Bearer token credential.
    pub fn bearer(token: impl Into<String>) -> Self {
        Self::Bearer(token.into())
    }

    /// Create a single custom header credential.
    pub fn header(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Header {
            name: name.into(),
            value: value.into(),
        }
    }

    /// Create a multi-header credential.
    pub fn headers(headers: Vec<(String, String)>) -> Self {
        Self::Headers(headers)
    }

    /// Return the headers this credential would inject.
    fn to_headers(&self) -> Vec<(String, String)> {
        match self {
            Self::Bearer(token) => {
                vec![("Authorization".to_string(), format!("Bearer {token}"))]
            }
            Self::Header { name, value } => vec![(name.clone(), value.clone())],
            Self::Headers(headers) => headers.clone(),
        }
    }

    /// Return the header names this credential injects (for overwrite).
    fn header_names(&self) -> Vec<String> {
        match self {
            Self::Bearer(_) => vec!["authorization".to_string()],
            Self::Header { name, .. } => vec![name.to_lowercase()],
            Self::Headers(headers) => headers.iter().map(|(n, _)| n.to_lowercase()).collect(),
        }
    }

    /// Return the raw secret values (for placeholder replacement matching).
    fn secret_values(&self) -> Vec<String> {
        match self {
            Self::Bearer(token) => vec![token.clone()],
            Self::Header { value, .. } => vec![value.clone()],
            Self::Headers(headers) => headers.iter().map(|(_, v)| v.clone()).collect(),
        }
    }
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bearer(_) => f.debug_tuple("Bearer").field(&"[REDACTED]").finish(),
            Self::Header { name, .. } => f
                .debug_struct("Header")
                .field("name", name)
                .field("value", &"[REDACTED]")
                .finish(),
            Self::Headers(headers) => {
                let redacted: Vec<_> = headers.iter().map(|(n, _)| (n, "[REDACTED]")).collect();
                f.debug_tuple("Headers").field(&redacted).finish()
            }
        }
    }
}

/// Placeholder prefix for generated placeholder tokens.
const PLACEHOLDER_PREFIX: &str = "bk_placeholder_";

/// Generate a random placeholder token using a CSPRNG.
///
/// Format: `bk_placeholder_<32 hex chars>` (128 bits of OS randomness).
///
/// # Security
///
/// The placeholder acts like a bearer capability inside scripts: if a script
/// sends the placeholder value to a URL matching a credential rule, Bashkit
/// injects the real credential-owned header. The token therefore must come
/// from a security-grade RNG so it cannot be predicted, brute-forced, or
/// reproduced from observed program state.
///
/// `getrandom::fill` reads from the OS CSPRNG (`getrandom(2)` / `arc4random_buf`
/// / `BCryptGenRandom`). If the OS RNG is genuinely unavailable the function
/// panics — there is no safe fallback for credential-shaped randomness, and
/// silently weakening it (e.g. `RandomState`) was the bug this code replaces.
fn generate_placeholder() -> String {
    let mut bytes = [0u8; 16]; // 128 bits
    getrandom::fill(&mut bytes).expect("OS CSPRNG must be available for credential placeholder");
    let mut hex = String::with_capacity(PLACEHOLDER_PREFIX.len() + 32);
    hex.push_str(PLACEHOLDER_PREFIX);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(hex, "{:02x}", byte);
    }
    hex
}

/// A rule mapping a URL pattern to a credential.
struct CredentialRule {
    /// URL pattern (same format as NetworkAllowlist patterns).
    pattern: String,
    /// The credential to inject.
    credential: Credential,
    /// For placeholder mode: the placeholder string visible to scripts.
    placeholder: Option<String>,
}

/// A compiled credential rule with a pre-built allowlist for URL matching.
struct CompiledRule {
    allowlist: NetworkAllowlist,
    credential: Credential,
    placeholder: Option<String>,
}

/// Collects credential rules and builds a `before_http` hook.
///
/// This is an internal type used by [`crate::BashBuilder`]. Users interact
/// with it via [`crate::BashBuilder::credential`] and
/// [`crate::BashBuilder::credential_placeholder`].
pub(crate) struct CredentialPolicy {
    rules: Vec<CredentialRule>,
}

impl CredentialPolicy {
    pub(crate) fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add an injection-mode rule.
    pub(crate) fn add_injection(&mut self, pattern: impl Into<String>, credential: Credential) {
        self.rules.push(CredentialRule {
            pattern: pattern.into(),
            credential,
            placeholder: None,
        });
    }

    /// Add a placeholder-mode rule. Returns `(env_name, placeholder_value)`
    /// so the caller can set the env var.
    pub(crate) fn add_placeholder(
        &mut self,
        pattern: impl Into<String>,
        credential: Credential,
    ) -> String {
        let placeholder = generate_placeholder();
        self.rules.push(CredentialRule {
            pattern: pattern.into(),
            credential,
            placeholder: Some(placeholder.clone()),
        });
        placeholder
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Convert this policy into a `before_http` interceptor hook.
    ///
    /// The hook:
    /// 1. Matches request URL against rule patterns
    /// 2. For injection rules: overwrites headers with credential headers
    /// 3. For placeholder rules: finds placeholder strings in header values
    ///    and replaces them with real credential values
    pub(crate) fn into_hook(self) -> Interceptor<HttpRequestEvent> {
        // Pre-build allowlists for each rule so we can use URL matching.
        let compiled: Vec<CompiledRule> = self
            .rules
            .into_iter()
            .map(|rule| {
                let allowlist = NetworkAllowlist::new().allow(&rule.pattern);
                CompiledRule {
                    allowlist,
                    credential: rule.credential,
                    placeholder: rule.placeholder,
                }
            })
            .collect();

        Box::new(move |mut event: HttpRequestEvent| {
            for rule in &compiled {
                if !rule.allowlist.is_allowed(&event.url) {
                    continue;
                }

                match &rule.placeholder {
                    None => {
                        // Injection mode: overwrite existing headers, then add credential headers.
                        let names_to_remove = rule.credential.header_names();
                        event
                            .headers
                            .retain(|(name, _)| !names_to_remove.contains(&name.to_lowercase()));
                        event.headers.extend(rule.credential.to_headers());
                    }
                    Some(placeholder) => {
                        // Placeholder mode: replace only in credential-owned header values.
                        let credential_header_secrets = rule
                            .credential
                            .header_names()
                            .into_iter()
                            .zip(rule.credential.secret_values())
                            .collect::<Vec<_>>();
                        let placeholder_str: &str = placeholder;
                        for (header_name, header_value) in &mut event.headers {
                            if let Some((_, secret_value)) = credential_header_secrets
                                .iter()
                                .find(|(name, _)| name.eq_ignore_ascii_case(header_name))
                            {
                                // Substitute only when the placeholder appears
                                // exactly once. A legitimate value carries one
                                // placeholder; multiple occurrences are a
                                // segmentation/fingerprinting attempt (a script
                                // splitting the value to read the secret back in
                                // chunks from a header-reflecting endpoint), so
                                // we refuse and fail closed — the request goes
                                // out with placeholders, never the real secret.
                                // (The single-echo risk where an approved host
                                // reflects the whole header remains a documented
                                // v1 limitation; see credential-injection.md.)
                                if header_value.matches(placeholder_str).count() == 1 {
                                    *header_value =
                                        header_value.replacen(placeholder_str, secret_value, 1);
                                }
                            }
                        }
                    }
                }
            }

            HookAction::Continue(event)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder_generation() {
        let p1 = generate_placeholder();
        let p2 = generate_placeholder();
        assert!(p1.starts_with(PLACEHOLDER_PREFIX));
        assert!(p2.starts_with(PLACEHOLDER_PREFIX));
        // Should be different (128 bits of CSPRNG randomness)
        assert_ne!(p1, p2);
        // Fixed length: prefix (15) + 32 hex chars = 47
        assert_eq!(p1.len(), 47);
    }

    #[test]
    fn test_placeholder_format_is_lowercase_hex() {
        let p = generate_placeholder();
        let hex_part = &p[PLACEHOLDER_PREFIX.len()..];
        assert_eq!(hex_part.len(), 32);
        for c in hex_part.chars() {
            assert!(
                c.is_ascii_hexdigit() && (!c.is_ascii_alphabetic() || c.is_ascii_lowercase()),
                "placeholder must be lowercase hex, got: {p}"
            );
        }
    }

    #[test]
    fn test_placeholder_uniqueness_across_many_generations() {
        // CSPRNG must produce 1024 distinct 128-bit tokens. RandomState's
        // hasher used to share initialization across hashers within a thread,
        // which made this regression surface; getrandom must not.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1024 {
            let p = generate_placeholder();
            assert!(seen.insert(p), "placeholder collision detected");
        }
    }

    #[test]
    fn test_placeholder_does_not_leak_raw_credential() {
        // Placeholder tokens must never embed any portion of a real secret.
        // Build one alongside an injected credential and assert the secret
        // bytes do not appear in the placeholder string.
        let secret = "hunter2-very-long-secret-token-payload-XYZ";
        let mut policy = CredentialPolicy::new();
        let cred = Credential::bearer(secret);
        let placeholder = policy.add_placeholder("https://api.example.com", cred);
        assert!(!placeholder.contains(secret));
        // Debug formatting of the placeholder must also not leak the secret.
        let debug = format!("{:?}", placeholder);
        assert!(!debug.contains(secret));
    }

    #[test]
    fn test_credential_debug_redacts() {
        let cred = Credential::bearer("super_secret");
        let debug = format!("{:?}", cred);
        assert!(!debug.contains("super_secret"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn test_credential_to_headers_bearer() {
        let cred = Credential::bearer("tok123");
        let headers = cred.to_headers();
        assert_eq!(
            headers,
            vec![("Authorization".to_string(), "Bearer tok123".to_string())]
        );
    }

    #[test]
    fn test_credential_to_headers_custom() {
        let cred = Credential::header("X-Api-Key", "key123");
        let headers = cred.to_headers();
        assert_eq!(
            headers,
            vec![("X-Api-Key".to_string(), "key123".to_string())]
        );
    }

    #[test]
    fn test_credential_to_headers_multi() {
        let cred = Credential::headers(vec![
            ("X-Key".into(), "k".into()),
            ("X-Secret".into(), "s".into()),
        ]);
        let headers = cred.to_headers();
        assert_eq!(headers.len(), 2);
    }

    #[test]
    fn test_injection_hook_adds_headers() {
        let mut policy = CredentialPolicy::new();
        policy.add_injection("https://api.example.com", Credential::bearer("tok"));

        let hook = policy.into_hook();
        let event = HttpRequestEvent {
            method: "GET".into(),
            url: "https://api.example.com/data".into(),
            headers: vec![],
        };

        match hook(event) {
            HookAction::Continue(e) => {
                assert_eq!(e.headers.len(), 1);
                assert_eq!(e.headers[0].0, "Authorization");
                assert_eq!(e.headers[0].1, "Bearer tok");
            }
            HookAction::Cancel(_) => panic!("should not cancel"),
        }
    }

    #[test]
    fn test_injection_hook_overwrites_existing_header() {
        let mut policy = CredentialPolicy::new();
        policy.add_injection("https://api.example.com", Credential::bearer("real_tok"));

        let hook = policy.into_hook();
        let event = HttpRequestEvent {
            method: "GET".into(),
            url: "https://api.example.com/data".into(),
            headers: vec![("Authorization".into(), "Bearer fake_tok".into())],
        };

        match hook(event) {
            HookAction::Continue(e) => {
                assert_eq!(e.headers.len(), 1);
                assert_eq!(e.headers[0].1, "Bearer real_tok");
            }
            HookAction::Cancel(_) => panic!("should not cancel"),
        }
    }

    #[test]
    fn test_injection_hook_skips_non_matching_url() {
        let mut policy = CredentialPolicy::new();
        policy.add_injection("https://api.example.com", Credential::bearer("tok"));

        let hook = policy.into_hook();
        let event = HttpRequestEvent {
            method: "GET".into(),
            url: "https://other.example.com/data".into(),
            headers: vec![],
        };

        match hook(event) {
            HookAction::Continue(e) => {
                assert!(
                    e.headers.is_empty(),
                    "should not inject for non-matching URL"
                );
            }
            HookAction::Cancel(_) => panic!("should not cancel"),
        }
    }

    #[test]
    fn test_placeholder_hook_replaces_in_header() {
        let mut policy = CredentialPolicy::new();
        let placeholder =
            policy.add_placeholder("https://api.openai.com", Credential::bearer("sk-real-key"));

        let hook = policy.into_hook();
        let event = HttpRequestEvent {
            method: "POST".into(),
            url: "https://api.openai.com/v1/chat/completions".into(),
            headers: vec![("Authorization".into(), format!("Bearer {}", placeholder))],
        };

        match hook(event) {
            HookAction::Continue(e) => {
                assert_eq!(e.headers.len(), 1);
                assert_eq!(e.headers[0].1, "Bearer sk-real-key");
            }
            HookAction::Cancel(_) => panic!("should not cancel"),
        }
    }

    #[test]
    fn test_placeholder_multiple_occurrences_fail_closed() {
        // A script repeating the placeholder is attempting to segment the
        // secret across a header-reflecting endpoint. We must not substitute
        // anything: the request goes out with placeholders, never the secret.
        let mut policy = CredentialPolicy::new();
        let placeholder =
            policy.add_placeholder("https://api.openai.com", Credential::bearer("sk-real-key"));

        let hook = policy.into_hook();
        let event = HttpRequestEvent {
            method: "POST".into(),
            url: "https://api.openai.com/v1/chat/completions".into(),
            headers: vec![(
                "Authorization".into(),
                format!("Bearer {placeholder}.{placeholder}"),
            )],
        };

        match hook(event) {
            HookAction::Continue(e) => {
                assert!(
                    !e.headers[0].1.contains("sk-real-key"),
                    "secret must not be injected when placeholder is repeated"
                );
                assert!(e.headers[0].1.contains("bk_placeholder_"));
            }
            HookAction::Cancel(_) => panic!("should not cancel"),
        }
    }

    #[test]
    fn test_placeholder_not_replaced_for_wrong_host() {
        let mut policy = CredentialPolicy::new();
        let placeholder =
            policy.add_placeholder("https://api.openai.com", Credential::bearer("sk-real-key"));

        let hook = policy.into_hook();
        let event = HttpRequestEvent {
            method: "POST".into(),
            url: "https://evil.com/exfiltrate".into(),
            headers: vec![("Authorization".into(), format!("Bearer {}", placeholder))],
        };

        match hook(event) {
            HookAction::Continue(e) => {
                // Placeholder should NOT be replaced — wrong host
                assert!(e.headers[0].1.contains("bk_placeholder_"));
            }
            HookAction::Cancel(_) => panic!("should not cancel"),
        }
    }

    #[test]
    fn test_placeholder_only_replaced_in_credential_headers() {
        let mut policy = CredentialPolicy::new();
        let placeholder =
            policy.add_placeholder("https://api.openai.com", Credential::bearer("sk-real-key"));

        let hook = policy.into_hook();
        let event = HttpRequestEvent {
            method: "POST".into(),
            url: "https://api.openai.com/v1/chat/completions".into(),
            headers: vec![
                ("Authorization".into(), format!("Bearer {}", placeholder)),
                ("X-Debug".into(), format!("leak={}", placeholder)),
            ],
        };

        match hook(event) {
            HookAction::Continue(e) => {
                assert_eq!(e.headers[0].1, "Bearer sk-real-key");
                assert!(e.headers[1].1.contains("bk_placeholder_"));
            }
            HookAction::Cancel(_) => panic!("should not cancel"),
        }
    }

    #[test]
    fn test_path_scoped_credential() {
        let mut policy = CredentialPolicy::new();
        policy.add_injection("https://api.example.com/v1/", Credential::bearer("v1_tok"));

        let hook = policy.into_hook();

        // Should match /v1/ prefix
        let event = HttpRequestEvent {
            method: "GET".into(),
            url: "https://api.example.com/v1/users".into(),
            headers: vec![],
        };
        match hook(event) {
            HookAction::Continue(e) => assert_eq!(e.headers.len(), 1),
            HookAction::Cancel(_) => panic!("should not cancel"),
        }

        // Should NOT match /v2/
        let event = HttpRequestEvent {
            method: "GET".into(),
            url: "https://api.example.com/v2/users".into(),
            headers: vec![],
        };
        match hook(event) {
            HookAction::Continue(e) => assert!(e.headers.is_empty()),
            HookAction::Cancel(_) => panic!("should not cancel"),
        }
    }

    #[test]
    fn test_multiple_rules() {
        let mut policy = CredentialPolicy::new();
        policy.add_injection("https://github.com", Credential::bearer("gh_tok"));
        policy.add_injection(
            "https://api.openai.com",
            Credential::header("X-Api-Key", "openai_key"),
        );

        let hook = policy.into_hook();

        // GitHub request
        let event = HttpRequestEvent {
            method: "GET".into(),
            url: "https://github.com/api/repos".into(),
            headers: vec![],
        };
        match hook(event) {
            HookAction::Continue(e) => {
                assert_eq!(e.headers.len(), 1);
                assert_eq!(e.headers[0].1, "Bearer gh_tok");
            }
            HookAction::Cancel(_) => panic!("should not cancel"),
        }

        // OpenAI request
        let event = HttpRequestEvent {
            method: "POST".into(),
            url: "https://api.openai.com/v1/chat".into(),
            headers: vec![],
        };
        match hook(event) {
            HookAction::Continue(e) => {
                assert_eq!(e.headers.len(), 1);
                assert_eq!(e.headers[0].0, "X-Api-Key");
            }
            HookAction::Cancel(_) => panic!("should not cancel"),
        }
    }

    #[test]
    fn test_header_name_case_insensitive_overwrite() {
        let mut policy = CredentialPolicy::new();
        policy.add_injection("https://api.example.com", Credential::bearer("real"));

        let hook = policy.into_hook();
        let event = HttpRequestEvent {
            method: "GET".into(),
            url: "https://api.example.com/data".into(),
            headers: vec![("authorization".into(), "Bearer fake".into())],
        };

        match hook(event) {
            HookAction::Continue(e) => {
                assert_eq!(e.headers.len(), 1);
                assert_eq!(e.headers[0].1, "Bearer real");
            }
            HookAction::Cancel(_) => panic!("should not cancel"),
        }
    }
}
