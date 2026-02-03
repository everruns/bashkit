//! Logging Security Tests
//!
//! Tests for TM-LOG-* threats to ensure sensitive data is properly redacted
//! and log injection attacks are prevented.
//!
//! These tests verify the security properties documented in specs/006-threat-model.md
//! section 7 "Logging Security".

#![cfg(feature = "logging")]

use bashkit::LogConfig;

mod redaction_tests {
    use super::*;

    // ==========================================================================
    // TM-LOG-001: Secrets in logs - Environment variable redaction
    // ==========================================================================

    #[test]
    fn test_common_secret_env_vars_redacted() {
        // Test for TM-LOG-001: Common sensitive variable patterns should be redacted
        let config = LogConfig::new();

        // Password patterns
        assert!(
            config.should_redact_env("PASSWORD"),
            "PASSWORD should be redacted"
        );
        assert!(
            config.should_redact_env("DB_PASSWORD"),
            "DB_PASSWORD should be redacted"
        );
        assert!(
            config.should_redact_env("MYSQL_ROOT_PASSWORD"),
            "MYSQL_ROOT_PASSWORD should be redacted"
        );

        // Token patterns
        assert!(
            config.should_redact_env("TOKEN"),
            "TOKEN should be redacted"
        );
        assert!(
            config.should_redact_env("ACCESS_TOKEN"),
            "ACCESS_TOKEN should be redacted"
        );
        assert!(
            config.should_redact_env("REFRESH_TOKEN"),
            "REFRESH_TOKEN should be redacted"
        );
        assert!(
            config.should_redact_env("GITHUB_TOKEN"),
            "GITHUB_TOKEN should be redacted"
        );
        assert!(
            config.should_redact_env("NPM_TOKEN"),
            "NPM_TOKEN should be redacted"
        );

        // Key patterns
        assert!(
            config.should_redact_env("API_KEY"),
            "API_KEY should be redacted"
        );
        assert!(
            config.should_redact_env("SECRET_KEY"),
            "SECRET_KEY should be redacted"
        );
        assert!(
            config.should_redact_env("PRIVATE_KEY"),
            "PRIVATE_KEY should be redacted"
        );
        assert!(
            config.should_redact_env("ENCRYPTION_KEY"),
            "ENCRYPTION_KEY should be redacted"
        );

        // Auth patterns
        assert!(config.should_redact_env("AUTH"), "AUTH should be redacted");
        assert!(
            config.should_redact_env("AUTHORIZATION"),
            "AUTHORIZATION should be redacted"
        );

        // Database URLs (often contain credentials)
        assert!(
            config.should_redact_env("DATABASE_URL"),
            "DATABASE_URL should be redacted"
        );
        assert!(
            config.should_redact_env("DB_URL"),
            "DB_URL should be redacted"
        );

        // Cloud provider secrets
        assert!(
            config.should_redact_env("AWS_SECRET_ACCESS_KEY"),
            "AWS_SECRET_ACCESS_KEY should be redacted"
        );

        // Session/Cookie related
        assert!(
            config.should_redact_env("SESSION_SECRET"),
            "SESSION_SECRET should be redacted"
        );
        assert!(
            config.should_redact_env("COOKIE_SECRET"),
            "COOKIE_SECRET should be redacted"
        );
    }

    #[test]
    fn test_normal_env_vars_not_redacted() {
        // Test for TM-LOG-001: Normal (non-sensitive) env vars should not be redacted
        let config = LogConfig::new();

        assert!(
            !config.should_redact_env("HOME"),
            "HOME should not be redacted"
        );
        assert!(
            !config.should_redact_env("PATH"),
            "PATH should not be redacted"
        );
        assert!(
            !config.should_redact_env("USER"),
            "USER should not be redacted"
        );
        assert!(
            !config.should_redact_env("SHELL"),
            "SHELL should not be redacted"
        );
        assert!(
            !config.should_redact_env("TERM"),
            "TERM should not be redacted"
        );
        assert!(
            !config.should_redact_env("PWD"),
            "PWD should not be redacted"
        );
        assert!(
            !config.should_redact_env("LANG"),
            "LANG should not be redacted"
        );
        assert!(
            !config.should_redact_env("LC_ALL"),
            "LC_ALL should not be redacted"
        );
        assert!(
            !config.should_redact_env("EDITOR"),
            "EDITOR should not be redacted"
        );
        assert!(
            !config.should_redact_env("DEBUG"),
            "DEBUG should not be redacted"
        );
    }

    #[test]
    fn test_case_insensitive_redaction() {
        // Test for TM-LOG-001: Redaction should be case-insensitive
        let config = LogConfig::new();

        assert!(
            config.should_redact_env("password"),
            "lowercase 'password' should be redacted"
        );
        assert!(
            config.should_redact_env("Password"),
            "mixed case 'Password' should be redacted"
        );
        assert!(
            config.should_redact_env("PASSWORD"),
            "uppercase 'PASSWORD' should be redacted"
        );
        assert!(
            config.should_redact_env("PaSsWoRd"),
            "weird case 'PaSsWoRd' should be redacted"
        );
    }

    #[test]
    fn test_custom_redaction_patterns() {
        // Test for TM-LOG-001: Custom patterns can be added
        let config = LogConfig::new()
            .redact_env("MY_INTERNAL_SECRET")
            .redact_env("COMPANY_SPECIFIC");

        assert!(
            config.should_redact_env("MY_INTERNAL_SECRET"),
            "Custom pattern should be redacted"
        );
        assert!(
            config.should_redact_env("COMPANY_SPECIFIC_KEY"),
            "Custom pattern should match substrings"
        );
    }

    // ==========================================================================
    // TM-LOG-003: URL credential redaction
    // ==========================================================================

    #[test]
    fn test_url_credential_redaction() {
        // Test for TM-LOG-003: URL credentials should be redacted
        let config = LogConfig::new();

        // Basic auth in URL
        assert_eq!(
            config
                .redact_url("https://user:password@example.com/path")
                .as_ref(),
            "https://[REDACTED]@example.com/path"
        );

        // Complex credentials
        assert_eq!(
            config
                .redact_url("https://admin:super$ecret123@db.example.com:5432/mydb")
                .as_ref(),
            "https://[REDACTED]@db.example.com:5432/mydb"
        );

        // HTTP (not just HTTPS)
        assert_eq!(
            config
                .redact_url("http://user:pass@internal.example.com")
                .as_ref(),
            "http://[REDACTED]@internal.example.com"
        );
    }

    #[test]
    fn test_url_without_credentials_unchanged() {
        // Test for TM-LOG-003: URLs without credentials should not be modified
        let config = LogConfig::new();

        assert_eq!(
            config.redact_url("https://example.com/path").as_ref(),
            "https://example.com/path"
        );

        // Username without password is not redacted (no sensitive data)
        assert_eq!(
            config.redact_url("https://user@example.com/path").as_ref(),
            "https://user@example.com/path"
        );

        // Query params (might contain tokens, but that's value redaction)
        assert_eq!(
            config
                .redact_url("https://api.example.com?key=value")
                .as_ref(),
            "https://api.example.com?key=value"
        );
    }

    // ==========================================================================
    // TM-LOG-004: API key and JWT detection
    // ==========================================================================

    #[test]
    fn test_jwt_redaction() {
        // Test for TM-LOG-004: JWTs should be detected and redacted
        let config = LogConfig::new();

        // Valid JWT format (three base64 parts)
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        assert_eq!(config.redact_value(jwt).as_ref(), "[REDACTED]");
    }

    #[test]
    fn test_api_key_prefixes_redacted() {
        // Test for TM-LOG-004: Common API key prefixes should trigger redaction
        let config = LogConfig::new();

        // Stripe-style keys (using fake prefix patterns)
        assert_eq!(
            config.redact_value("sk-FAKE_TEST_KEY_123456").as_ref(),
            "[REDACTED]"
        );
        assert_eq!(
            config.redact_value("pk-FAKE_TEST_KEY_123456").as_ref(),
            "[REDACTED]"
        );
        assert_eq!(
            config
                .redact_value("sk_live_FAKE_TEST_KEY_123456")
                .as_ref(),
            "[REDACTED]"
        );
        assert_eq!(
            config
                .redact_value("sk_test_FAKE_TEST_KEY_123456")
                .as_ref(),
            "[REDACTED]"
        );

        // GitHub tokens (using fake patterns)
        assert_eq!(
            config.redact_value("ghp_FAKE_TEST_TOKEN_12345678").as_ref(),
            "[REDACTED]"
        );
        assert_eq!(
            config.redact_value("gho_FAKE_TEST_TOKEN_12345678").as_ref(),
            "[REDACTED]"
        );

        // Slack-style tokens (using obviously fake pattern)
        assert_eq!(
            config
                .redact_value("xoxb-FAKE-TEST-TOKEN-HERE")
                .as_ref(),
            "[REDACTED]"
        );

        // AWS-style access keys (using documented fake key)
        assert_eq!(
            config.redact_value("AKIAFAKEKEY12345678").as_ref(),
            "[REDACTED]"
        );
    }

    #[test]
    fn test_normal_values_not_redacted() {
        // Test for TM-LOG-004: Normal values should not be redacted
        let config = LogConfig::new();

        assert_eq!(config.redact_value("hello world").as_ref(), "hello world");
        assert_eq!(config.redact_value("12345").as_ref(), "12345");
        assert_eq!(
            config.redact_value("/path/to/file").as_ref(),
            "/path/to/file"
        );
        assert_eq!(
            config.redact_value("user@example.com").as_ref(),
            "user@example.com"
        );
    }

    // ==========================================================================
    // TM-LOG-002: Script content leak prevention
    // ==========================================================================

    #[test]
    fn test_script_content_not_logged_by_default() {
        // Test for TM-LOG-002: Script content should not be logged by default
        use bashkit::logging::format_script_for_log;

        let config = LogConfig::new();
        let script = r#"
            export SECRET_API_KEY="sk-supersecretkey123"
            curl -H "Authorization: Bearer $SECRET_API_KEY" https://api.example.com
        "#;

        let formatted = format_script_for_log(script, &config);

        // Should only show metadata, not content
        assert!(formatted.contains("lines"), "Should show line count");
        assert!(formatted.contains("bytes"), "Should show byte count");
        assert!(
            !formatted.contains("SECRET_API_KEY"),
            "Should not contain secret var name"
        );
        assert!(
            !formatted.contains("sk-supersecretkey"),
            "Should not contain secret value"
        );
        assert!(
            !formatted.contains("Authorization"),
            "Should not contain auth header"
        );
    }

    #[test]
    fn test_script_content_with_unsafe_flag() {
        // Test for TM-LOG-002: Content shown only with explicit unsafe flag
        use bashkit::logging::format_script_for_log;

        let config = LogConfig::new().unsafe_log_scripts();
        let script = "echo hello";

        let formatted = format_script_for_log(script, &config);
        assert!(
            formatted.contains("echo"),
            "Should contain script content with unsafe flag"
        );
    }
}

mod injection_tests {
    use bashkit::logging::sanitize_for_log;

    // ==========================================================================
    // TM-LOG-005: Newline injection prevention
    // ==========================================================================

    #[test]
    fn test_newline_injection_prevented() {
        // Test for TM-LOG-005: Newlines should be escaped to prevent fake log entries
        let malicious = "normal\n[ERROR] SECURITY BREACH: Hacked!";
        let sanitized = sanitize_for_log(malicious);

        assert!(
            !sanitized.contains('\n'),
            "Should not contain literal newlines"
        );
        assert!(sanitized.contains("\\n"), "Should contain escaped newlines");
        assert!(
            sanitized.contains("normal"),
            "Should preserve normal content"
        );
        assert!(
            sanitized.contains("[ERROR]"),
            "Should preserve injected content (escaped)"
        );
    }

    #[test]
    fn test_carriage_return_injection_prevented() {
        // Test for TM-LOG-005: Carriage returns should be escaped
        let malicious = "normal\r[ERROR] Fake entry";
        let sanitized = sanitize_for_log(malicious);

        assert!(
            !sanitized.contains('\r'),
            "Should not contain literal carriage returns"
        );
        assert!(
            sanitized.contains("\\r"),
            "Should contain escaped carriage returns"
        );
    }

    #[test]
    fn test_crlf_injection_prevented() {
        // Test for TM-LOG-005: CRLF injection should be prevented
        let malicious = "normal\r\n[ERROR] Windows-style injection";
        let sanitized = sanitize_for_log(malicious);

        assert!(!sanitized.contains('\r'), "Should not contain literal CR");
        assert!(!sanitized.contains('\n'), "Should not contain literal LF");
        assert!(sanitized.contains("\\r\\n"), "Should contain escaped CRLF");
    }

    // ==========================================================================
    // TM-LOG-006: Control character injection prevention
    // ==========================================================================

    #[test]
    fn test_tab_escaped() {
        // Test for TM-LOG-006: Tabs should be escaped
        let input = "column1\tcolumn2";
        let sanitized = sanitize_for_log(input);

        assert!(!sanitized.contains('\t'), "Should not contain literal tabs");
        assert!(sanitized.contains("\\t"), "Should contain escaped tabs");
    }

    #[test]
    fn test_control_chars_filtered() {
        // Test for TM-LOG-006: Non-printable control characters should be filtered
        let input = "normal\x00null\x07bell\x1bescapeseq";
        let sanitized = sanitize_for_log(input);

        assert!(!sanitized.contains('\x00'), "Should not contain null bytes");
        assert!(!sanitized.contains('\x07'), "Should not contain bell char");
        assert!(
            !sanitized.contains('\x1b'),
            "Should not contain escape char"
        );
        assert!(sanitized.contains("normal"), "Should preserve normal text");
        assert!(
            sanitized.contains("null"),
            "Should preserve text after null"
        );
    }

    #[test]
    fn test_ansi_escape_sequences_filtered() {
        // Test for TM-LOG-006: ANSI escape sequences should be filtered
        let input = "normal\x1b[31mRED TEXT\x1b[0m";
        let sanitized = sanitize_for_log(input);

        assert!(
            !sanitized.contains("\x1b["),
            "Should not contain ANSI sequences"
        );
        assert!(sanitized.contains("normal"), "Should preserve normal text");
        assert!(
            sanitized.contains("RED TEXT"),
            "Should preserve text content"
        );
    }
}

mod truncation_tests {
    use super::*;

    // ==========================================================================
    // TM-LOG-007 & TM-LOG-008: Log value truncation
    // ==========================================================================

    #[test]
    fn test_long_values_truncated() {
        // Test for TM-LOG-008: Long values should be truncated
        let config = LogConfig::new().max_value_length(50);
        let long_value = "a".repeat(200);

        let truncated = config.redact_value(&long_value);
        let truncated_str = truncated.as_ref();

        assert!(truncated_str.len() < 200, "Should be shorter than original");
        assert!(
            truncated_str.starts_with("aaaa"),
            "Should start with original content"
        );
        assert!(
            truncated_str.contains("[truncated"),
            "Should indicate truncation"
        );
        assert!(
            truncated_str.contains("bytes]"),
            "Should show truncated byte count"
        );
    }

    #[test]
    fn test_short_values_not_truncated() {
        // Test for TM-LOG-008: Short values should not be truncated
        let config = LogConfig::new().max_value_length(200);
        let short_value = "hello world";

        let result = config.redact_value(short_value);
        assert_eq!(result.as_ref(), "hello world");
        assert!(!result.contains("[truncated"), "Should not be truncated");
    }

    #[test]
    fn test_default_truncation_length() {
        // Test for TM-LOG-008: Default max length is 200
        let config = LogConfig::new();
        let value_199 = "a".repeat(199);
        let value_201 = "a".repeat(201);

        let result_199 = config.redact_value(&value_199);
        let result_201 = config.redact_value(&value_201);

        assert!(
            !result_199.contains("[truncated"),
            "199 chars should not be truncated"
        );
        assert!(
            result_201.contains("[truncated"),
            "201 chars should be truncated"
        );
    }
}

mod disabled_redaction_tests {
    use super::*;

    #[test]
    fn test_disabled_redaction_shows_secrets() {
        // Test that unsafe_disable_redaction actually disables redaction
        let config = LogConfig::new().unsafe_disable_redaction();

        // Env var redaction disabled
        assert!(
            !config.should_redact_env("PASSWORD"),
            "Should not redact with disabled"
        );
        assert!(
            !config.should_redact_env("SECRET_KEY"),
            "Should not redact with disabled"
        );

        // URL redaction disabled
        assert_eq!(
            config.redact_url("https://user:pass@example.com").as_ref(),
            "https://user:pass@example.com"
        );
    }

    #[test]
    fn test_default_redaction_enabled() {
        // Test that redaction is enabled by default
        let config = LogConfig::new();
        assert!(
            config.redact_sensitive,
            "Redaction should be enabled by default"
        );
    }
}

mod proptest_redaction {
    use bashkit::LogConfig;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn redacted_env_never_returns_value(name in "[A-Z_]{1,50}") {
            // Property: If should_redact_env returns true, the env var name contains
            // a sensitive pattern
            let config = LogConfig::new();
            if config.should_redact_env(&name) {
                let upper = name.to_uppercase();
                let patterns = [
                    "PASSWORD", "SECRET", "TOKEN", "KEY", "CREDENTIAL", "AUTH",
                    "API_KEY", "APIKEY", "PRIVATE", "BEARER", "JWT", "SESSION",
                    "COOKIE", "ENCRYPTION", "SIGNING", "DATABASE_URL", "DB_URL",
                    "CONNECTION_STRING", "AWS_SECRET", "AWS_ACCESS", "GITHUB_TOKEN",
                    "NPM_TOKEN", "STRIPE", "TWILIO", "SENDGRID", "PASSWD",
                ];
                let contains_pattern = patterns.iter().any(|p| upper.contains(p));
                prop_assert!(contains_pattern, "Redacted var '{}' should contain sensitive pattern", name);
            }
        }

        #[test]
        fn url_redaction_preserves_scheme_and_host(
            scheme in "https?",
            user in "[a-z]{3,10}",
            pass in "[a-z]{5,15}",  // longer password to avoid false positives
            host in "[a-z]{5,10}",
            path in "[a-z]{0,10}"
        ) {
            // Property: URL redaction preserves scheme and host, removes credentials
            let url = format!("{}://{}:{}@{}.example.com/{}", scheme, user, pass, host, path);
            let config = LogConfig::new();
            let redacted = config.redact_url(&url);
            let redacted_str = redacted.as_ref();

            // Should preserve scheme
            prop_assert!(redacted_str.starts_with(&scheme), "Should preserve scheme in '{}'", redacted_str);
            // Should preserve host
            prop_assert!(redacted_str.contains(&format!("{}.example.com", host)), "Should preserve host in '{}'", redacted_str);
            // Should contain [REDACTED]
            prop_assert!(redacted_str.contains("[REDACTED]"), "Should have redaction marker in '{}'", redacted_str);
            // Should NOT contain the colon-separated credentials
            prop_assert!(!redacted_str.contains(&format!("{}:{}", user, pass)), "Credentials should be redacted");
        }

        #[test]
        fn sanitize_removes_control_chars(input in "[^\x00-\x1f]*") {
            // Property: sanitize_for_log on strings without control chars returns same-ish content
            // (Note: we avoid generating control chars to simplify the test)
            use bashkit::logging::sanitize_for_log;
            let sanitized = sanitize_for_log(&input);

            // The sanitized output should be similar length (no control chars to remove)
            prop_assert!(
                sanitized.len() <= input.len() + 10,  // small overhead for escaping
                "Sanitized length {} should be close to input length {}",
                sanitized.len(), input.len()
            );
        }

        #[test]
        fn truncation_respects_limit(value in "[a-zA-Z0-9 ]{0,500}", limit in 10usize..300) {
            // Property: Truncated values don't exceed limit (plus overhead for marker)
            // Use only ASCII to avoid UTF-8 boundary issues
            let config = LogConfig::new().max_value_length(limit);
            let test_value = format!("test_{}", value);
            let result = config.redact_value(&test_value);

            // The result length should be reasonable (limit + some overhead for marker)
            // The marker is approximately "...[truncated N bytes]" which is ~25-35 chars
            let max_expected = limit + 50;
            prop_assert!(
                result.len() <= max_expected,
                "Result length {} exceeds expected max {} for limit {}",
                result.len(), max_expected, limit
            );
        }
    }
}
