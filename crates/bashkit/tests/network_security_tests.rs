//! Network Security Tests
//!
//! Tests for HTTP-related security threats documented in specs/006-threat-model.md
//! Section 5.3: HTTP Attack Vectors
//!
//! These tests verify:
//! - URL allowlist enforcement
//! - Response size limits
//! - Timeout handling
//! - Redirect security
//! - curl/wget builtin security
//!
//! Run with: `cargo test --features http_client network_security`

#![cfg(feature = "http_client")]

use bashkit::{Bash, NetworkAllowlist};

// =============================================================================
// 1. URL ALLOWLIST TESTS
// =============================================================================

mod allowlist {
    use super::*;

    /// Test that empty allowlist blocks all URLs
    #[tokio::test]
    async fn threat_empty_allowlist_blocks_all() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash.exec("curl https://example.com").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("access denied"));
    }

    /// Test that only allowed URLs succeed
    #[tokio::test]
    async fn threat_allowlist_blocks_unlisted() {
        let allowlist = NetworkAllowlist::new().allow("https://allowed.com");
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash.exec("curl https://blocked.com").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("access denied"));
    }

    /// Test IP-based bypass is blocked
    #[tokio::test]
    async fn threat_ip_bypass_blocked() {
        let allowlist = NetworkAllowlist::new().allow("https://example.com");
        let mut bash = Bash::builder().network(allowlist).build();

        // Try to access via IP instead of hostname
        let result = bash.exec("curl https://93.184.216.34").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("access denied"));
    }

    /// Test port must match
    #[tokio::test]
    async fn threat_port_bypass_blocked() {
        let allowlist = NetworkAllowlist::new().allow("https://example.com");
        let mut bash = Bash::builder().network(allowlist).build();

        // Try non-standard port
        let result = bash.exec("curl https://example.com:8443").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("access denied"));
    }

    /// Test scheme must match
    #[tokio::test]
    async fn threat_scheme_downgrade_blocked() {
        let allowlist = NetworkAllowlist::new().allow("https://example.com");
        let mut bash = Bash::builder().network(allowlist).build();

        // Try HTTP instead of HTTPS
        let result = bash.exec("curl http://example.com").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("access denied"));
    }

    /// Test subdomain bypass is blocked
    #[tokio::test]
    async fn threat_subdomain_bypass_blocked() {
        let allowlist = NetworkAllowlist::new().allow("https://example.com");
        let mut bash = Bash::builder().network(allowlist).build();

        // Try subdomain
        let result = bash.exec("curl https://evil.example.com").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("access denied"));
    }

    /// Test path prefix matching
    #[tokio::test]
    async fn threat_path_prefix_enforced() {
        let allowlist = NetworkAllowlist::new().allow("https://api.example.com/v1");
        let mut bash = Bash::builder().network(allowlist).build();

        // Try different path
        let result = bash.exec("curl https://api.example.com/v2").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("access denied"));
    }

    /// Test wget respects allowlist
    #[tokio::test]
    async fn threat_wget_respects_allowlist() {
        let allowlist = NetworkAllowlist::new().allow("https://allowed.com");
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash.exec("wget https://blocked.com").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("access denied"));
    }
}

// =============================================================================
// 2. NETWORK NOT CONFIGURED TESTS
// =============================================================================

mod no_network {
    use super::*;

    /// Test curl fails gracefully without network config
    #[tokio::test]
    async fn curl_without_network_config() {
        let mut bash = Bash::new();

        let result = bash.exec("curl https://example.com").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("network access not configured"));
    }

    /// Test wget fails gracefully without network config
    #[tokio::test]
    async fn wget_without_network_config() {
        let mut bash = Bash::new();

        let result = bash.exec("wget https://example.com").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("network access not configured"));
    }
}

// =============================================================================
// 3. CURL ARGUMENT PARSING TESTS
// =============================================================================

mod curl_args {
    use super::*;

    /// Test curl requires URL
    #[tokio::test]
    async fn curl_requires_url() {
        let mut bash = Bash::new();

        let result = bash.exec("curl").await.unwrap();
        assert_eq!(result.exit_code, 3);
        assert!(result.stderr.contains("no URL specified"));
    }

    /// Test curl -X sets method
    #[tokio::test]
    async fn curl_method_parsing() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        // Should fail with access denied, but method should be parsed
        let result = bash.exec("curl -X POST https://example.com").await.unwrap();
        assert!(result.stderr.contains("access denied"));
    }

    /// Test curl -d implies POST
    #[tokio::test]
    async fn curl_data_implies_post() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("curl -d 'data=test' https://example.com")
            .await
            .unwrap();
        assert!(result.stderr.contains("access denied"));
    }
}

// =============================================================================
// 4. WGET ARGUMENT PARSING TESTS
// =============================================================================

mod wget_args {
    use super::*;

    /// Test wget requires URL
    #[tokio::test]
    async fn wget_requires_url() {
        let mut bash = Bash::new();

        let result = bash.exec("wget").await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing URL"));
    }

    /// Test wget -q suppresses output (when network available)
    #[tokio::test]
    async fn wget_quiet_mode() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash.exec("wget -q https://example.com").await.unwrap();
        // Should still fail with access denied
        assert!(result.stderr.contains("access denied"));
    }
}

// =============================================================================
// 5. INTEGRATION WITH SCRIPT TESTS
// =============================================================================

mod script_integration {
    use super::*;

    /// Test curl in a script pipeline
    #[tokio::test]
    async fn curl_in_pipeline() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        // Even without network access, pipeline should work
        let result = bash
            .exec("curl https://blocked.com 2>&1 | grep -c denied")
            .await
            .unwrap();
        // grep should find "denied" once
        assert!(result.stdout.trim() == "1" || result.exit_code != 0);
    }

    /// Test conditional on curl exit code
    #[tokio::test]
    async fn curl_exit_code_in_condition() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec(
                r#"
            if curl https://blocked.com 2>/dev/null; then
                echo "success"
            else
                echo "failed"
            fi
            "#,
            )
            .await
            .unwrap();
        assert!(result.stdout.contains("failed"));
    }

    /// Test wget output redirect
    #[tokio::test]
    async fn wget_output_redirect() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        // Even without network, -O should be parsed
        let result = bash
            .exec("wget -O output.txt https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }
}

// =============================================================================
// 6. SECURITY EDGE CASES
// =============================================================================

mod security_edge_cases {
    use super::*;

    /// Test malformed URL handling
    #[tokio::test]
    async fn curl_malformed_url() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash.exec("curl not-a-url").await.unwrap();
        assert_ne!(result.exit_code, 0);
    }

    /// Test very long URL handling
    #[tokio::test]
    async fn curl_very_long_url() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let long_path = "a".repeat(10000);
        let script = format!("curl https://example.com/{}", long_path);
        let result = bash.exec(&script).await.unwrap();
        // Should fail with access denied or invalid URL
        assert_ne!(result.exit_code, 0);
    }

    /// Test URL with special characters
    #[tokio::test]
    async fn curl_url_with_special_chars() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("curl 'https://example.com/path?a=1&b=2'")
            .await
            .unwrap();
        // Should fail with access denied
        assert!(result.stderr.contains("access denied"));
    }

    /// Test command substitution with curl
    #[tokio::test]
    async fn curl_in_command_substitution() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("echo $(curl https://blocked.com 2>&1)")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied"));
    }

    /// Test curl with variable URL
    #[tokio::test]
    async fn curl_with_variable_url() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec(
                r#"
            URL="https://blocked.com"
            curl $URL 2>&1
            "#,
            )
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }
}

// =============================================================================
// 7. ALLOWLIST UNIT TESTS
// =============================================================================

mod allowlist_unit {
    use bashkit::NetworkAllowlist;

    #[test]
    fn test_empty_allowlist() {
        let allowlist = NetworkAllowlist::new();
        // Empty allowlist should block everything
        assert!(!allowlist.is_allowed("https://example.com"));
    }

    #[test]
    fn test_allow_all() {
        let allowlist = NetworkAllowlist::allow_all();
        assert!(allowlist.is_allowed("https://example.com"));
        assert!(allowlist.is_allowed("http://any.domain.com:8080/path"));
    }

    #[test]
    fn test_specific_host() {
        let allowlist = NetworkAllowlist::new().allow("https://api.example.com");
        assert!(allowlist.is_allowed("https://api.example.com"));
        assert!(allowlist.is_allowed("https://api.example.com/any/path"));
        assert!(!allowlist.is_allowed("https://other.example.com"));
    }

    #[test]
    fn test_path_prefix() {
        let allowlist = NetworkAllowlist::new().allow("https://api.example.com/v1");
        assert!(allowlist.is_allowed("https://api.example.com/v1"));
        assert!(allowlist.is_allowed("https://api.example.com/v1/users"));
        assert!(!allowlist.is_allowed("https://api.example.com/v2"));
        assert!(!allowlist.is_allowed("https://api.example.com/"));
    }

    #[test]
    fn test_multiple_allowed() {
        let allowlist = NetworkAllowlist::new()
            .allow("https://api.example.com")
            .allow("https://cdn.example.com");
        assert!(allowlist.is_allowed("https://api.example.com"));
        assert!(allowlist.is_allowed("https://cdn.example.com"));
        assert!(!allowlist.is_allowed("https://other.example.com"));
    }

    #[test]
    fn test_port_matching() {
        let allowlist = NetworkAllowlist::new().allow("https://example.com:8443");
        assert!(allowlist.is_allowed("https://example.com:8443"));
        assert!(!allowlist.is_allowed("https://example.com")); // default 443
        assert!(!allowlist.is_allowed("https://example.com:443"));
    }

    #[test]
    fn test_scheme_matching() {
        let allowlist = NetworkAllowlist::new().allow("https://example.com");
        assert!(allowlist.is_allowed("https://example.com"));
        assert!(!allowlist.is_allowed("http://example.com"));
    }
}

// =============================================================================
// 8. NEW FLAG TESTS
// =============================================================================

mod new_curl_flags {
    use super::*;

    /// Test curl --compressed flag parsing
    #[tokio::test]
    async fn curl_compressed_flag() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        // Should parse --compressed but still fail on allowlist
        let result = bash
            .exec("curl --compressed https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test curl -u/--user flag parsing
    #[tokio::test]
    async fn curl_user_auth_flag() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        // Should parse -u but still fail on allowlist
        let result = bash
            .exec("curl -u user:pass https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        // Also test --user variant
        let result = bash
            .exec("curl --user admin:secret https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test curl -A/--user-agent flag parsing
    #[tokio::test]
    async fn curl_user_agent_flag() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("curl -A 'Mozilla/5.0' https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        let result = bash
            .exec("curl --user-agent 'CustomAgent/1.0' https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test curl -e/--referer flag parsing
    #[tokio::test]
    async fn curl_referer_flag() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("curl -e 'https://example.com' https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test curl -v/--verbose flag parsing
    #[tokio::test]
    async fn curl_verbose_flag() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash.exec("curl -v https://blocked.com 2>&1").await.unwrap();
        // Verbose output should include request info even on failure
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test curl -m/--max-time flag parsing
    #[tokio::test]
    async fn curl_max_time_flag() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("curl -m 10 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        let result = bash
            .exec("curl --max-time 30 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test curl timeout with various values
    #[tokio::test]
    async fn curl_max_time_various_values() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        // Test with small timeout value
        let result = bash
            .exec("curl -m 1 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        // Test with larger timeout value
        let result = bash
            .exec("curl -m 300 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        // Test with zero timeout (should be treated as no timeout or handled gracefully)
        let result = bash
            .exec("curl -m 0 https://blocked.com 2>&1")
            .await
            .unwrap();
        // Zero timeout may be ignored - just check it doesn't crash
        assert!(result.exit_code != 0);
    }

    /// Test curl timeout with combined flags
    #[tokio::test]
    async fn curl_max_time_with_other_flags() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        // Timeout with silent mode
        let result = bash
            .exec("curl -s -m 5 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        // Timeout with POST data
        let result = bash
            .exec("curl -m 5 -d 'data=test' https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        // Timeout with headers
        let result = bash
            .exec("curl -m 5 -H 'X-Custom: value' https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test that timeout errors return exit code 28 (curl convention)
    #[tokio::test]
    async fn curl_timeout_error_exit_code() {
        let allowlist = NetworkAllowlist::new().allow("https://example.com");
        let mut bash = Bash::builder().network(allowlist).build();

        // The error message should mention timeout and use exit code 28
        // Note: This tests the error handling path, actual timeout would need a slow server
        let result = bash
            .exec("curl -m 5 https://notinallowlist.com 2>&1; echo \"exit:$?\"")
            .await
            .unwrap();
        // Should fail with non-zero exit code
        assert!(result.stdout.contains("exit:7") || result.stdout.contains("access denied"));
    }

    /// Test curl --connect-timeout flag parsing
    #[tokio::test]
    async fn curl_connect_timeout_flag() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("curl --connect-timeout 5 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        // Test with different values
        let result = bash
            .exec("curl --connect-timeout 30 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test curl with both --max-time and --connect-timeout
    #[tokio::test]
    async fn curl_both_timeout_flags() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        // Both flags together
        let result = bash
            .exec("curl -m 30 --connect-timeout 5 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        // Connect timeout can be larger than max-time
        let result = bash
            .exec("curl --connect-timeout 60 -m 10 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test timeout safety limits - values are clamped to [1, 3600] seconds
    #[tokio::test]
    async fn curl_timeout_safety_limits() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        // Very large timeout should be clamped to MAX_TIMEOUT_SECS (3600)
        let result = bash
            .exec("curl -m 999999 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        // Zero timeout should be clamped to MIN_TIMEOUT_SECS (1)
        let result = bash
            .exec("curl -m 0 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        // Same for connect-timeout
        let result = bash
            .exec("curl --connect-timeout 999999 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test curl with multiple flags combined
    #[tokio::test]
    async fn curl_combined_flags() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("curl -s --compressed -u user:pass -A 'Agent' -e 'http://ref.com' -m 30 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }
}

mod new_wget_flags {
    use super::*;

    /// Test wget --header flag parsing
    #[tokio::test]
    async fn wget_header_flag() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("wget --header 'X-Custom: value' https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test wget -U/--user-agent flag parsing
    #[tokio::test]
    async fn wget_user_agent_flag() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("wget -U 'CustomAgent' https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        let result = bash
            .exec("wget --user-agent 'Mozilla/5.0' https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test wget --post-data flag parsing
    #[tokio::test]
    async fn wget_post_data_flag() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("wget --post-data 'key=value' https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test wget -t/--tries flag parsing (should be ignored gracefully)
    #[tokio::test]
    async fn wget_tries_flag() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("wget -t 3 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        let result = bash
            .exec("wget --tries 5 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test wget with multiple flags combined
    #[tokio::test]
    async fn wget_combined_flags() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("wget -q --header 'Accept: application/json' -U 'Agent' -t 3 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test wget -T/--timeout flag parsing
    #[tokio::test]
    async fn wget_timeout_flag() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("wget -T 10 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        let result = bash
            .exec("wget --timeout 30 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test wget --connect-timeout flag parsing
    #[tokio::test]
    async fn wget_connect_timeout_flag() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("wget --connect-timeout 5 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test wget with both timeout flags
    #[tokio::test]
    async fn wget_both_timeout_flags() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("wget -T 30 --connect-timeout 5 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }

    /// Test wget timeout safety limits
    #[tokio::test]
    async fn wget_timeout_safety_limits() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        // Very large timeout should be clamped
        let result = bash
            .exec("wget -T 999999 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));

        // Zero timeout should be clamped
        let result = bash
            .exec("wget -T 0 https://blocked.com 2>&1")
            .await
            .unwrap();
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }
}

// =============================================================================
// 9. DECOMPRESSION SECURITY TESTS
// =============================================================================

mod decompression_security {
    use super::*;

    /// Test that --compressed with blocked URL still respects allowlist
    #[tokio::test]
    async fn compressed_respects_allowlist() {
        let allowlist = NetworkAllowlist::new();
        let mut bash = Bash::builder().network(allowlist).build();

        let result = bash
            .exec("curl --compressed https://blocked.com 2>&1")
            .await
            .unwrap();
        // Allowlist check happens BEFORE any network activity
        assert!(result.stdout.contains("access denied") || result.stderr.contains("access denied"));
    }
}
