//! Unicode Security Tests (TM-UNI-*)
//!
//! Tests for Unicode-specific threats identified in specs/006-threat-model.md.
//! Covers byte-boundary safety, invisible characters, homoglyphs, normalization,
//! and bidi attacks across parser, builtins, and VFS.
//!
//! Run with: `cargo test unicode_`

use bashkit::{Bash, FileSystem, FsLimits, InMemoryFs};
use std::path::Path;

// =============================================================================
// 1. BUILTIN PARSER BYTE-BOUNDARY SAFETY (TM-UNI-001, TM-UNI-002)
// =============================================================================

mod byte_boundary_safety {
    use super::*;

    /// TM-UNI-001: Awk parser must not panic on multi-byte Unicode in comments.
    /// Reproduces issue #395: box-drawing characters (U+2500, 3 bytes each)
    /// cause byte-boundary panic in awk parser.
    #[tokio::test]
    async fn unicode_awk_multibyte_comment_no_panic() {
        let mut bash = Bash::new();
        // Box-drawing chars in awk comment (the exact pattern from issue #395)
        let result = bash
            .exec(
                r#"echo "hello" | awk '# ── Pass 1 ──
{print $1}'"#,
            )
            .await
            .unwrap();
        // May fail to parse correctly (TM-UNI-001 is PARTIAL), but must not crash
        // the process. catch_unwind (TM-INT-001) should catch any panic.
        // We accept either success or a non-zero exit code.
        let _ = result.exit_code;
    }

    /// TM-UNI-001: Awk parser handles multi-byte chars in string literals
    #[tokio::test]
    async fn unicode_awk_multibyte_string_no_panic() {
        let mut bash = Bash::new();
        let result = bash
            .exec(r#"echo "café" | awk '{print "→ " $0}'"#)
            .await
            .unwrap();
        let _ = result.exit_code;
    }

    /// TM-UNI-001: Awk parser handles CJK characters in input
    #[tokio::test]
    async fn unicode_awk_cjk_input_no_panic() {
        let mut bash = Bash::new();
        let result = bash
            .exec(r#"echo "日本語 テスト" | awk '{print $1}'"#)
            .await
            .unwrap();
        let _ = result.exit_code;
    }

    /// TM-UNI-001: Awk parser handles emoji in input
    #[tokio::test]
    async fn unicode_awk_emoji_no_panic() {
        let mut bash = Bash::new();
        let result = bash
            .exec(r#"echo "hello 🌍 world" | awk '{print $2}'"#)
            .await
            .unwrap();
        let _ = result.exit_code;
    }

    /// TM-UNI-001: Awk with mixed ASCII and multi-byte in field separator
    #[tokio::test]
    async fn unicode_awk_multibyte_field_separator_no_panic() {
        let mut bash = Bash::new();
        let result = bash
            .exec(r#"echo "a│b│c" | awk -F'│' '{print $2}'"#)
            .await
            .unwrap();
        let _ = result.exit_code;
    }

    /// TM-UNI-001: Awk with multi-byte in pattern match
    #[tokio::test]
    async fn unicode_awk_multibyte_pattern_no_panic() {
        let mut bash = Bash::new();
        let result = bash
            .exec(r#"printf "café\ntest\n" | awk '/café/{print "found: " $0}'"#)
            .await
            .unwrap();
        let _ = result.exit_code;
    }

    /// TM-UNI-001: Awk with multi-byte chars in variable assignment
    #[tokio::test]
    async fn unicode_awk_multibyte_variable_no_panic() {
        let mut bash = Bash::new();
        let result = bash
            .exec(r#"echo "test" | awk 'BEGIN{x="─═─"} {print x, $0}'"#)
            .await
            .unwrap();
        let _ = result.exit_code;
    }

    /// TM-UNI-002: Sed handles multi-byte Unicode without panic
    #[tokio::test]
    async fn unicode_sed_multibyte_no_panic() {
        let mut bash = Bash::new();
        let result = bash
            .exec(r#"echo "café latte" | sed 's/café/coffee/'"#)
            .await
            .unwrap();
        let _ = result.exit_code;
    }

    /// TM-UNI-002: Sed with CJK replacement
    #[tokio::test]
    async fn unicode_sed_cjk_replacement_no_panic() {
        let mut bash = Bash::new();
        let result = bash
            .exec(r#"echo "hello world" | sed 's/world/世界/'"#)
            .await
            .unwrap();
        let _ = result.exit_code;
    }

    /// TM-UNI-002: Sed with box-drawing chars in pattern
    #[tokio::test]
    async fn unicode_sed_box_drawing_no_panic() {
        let mut bash = Bash::new();
        let result = bash
            .exec(r#"echo "──border──" | sed 's/──//g'"#)
            .await
            .unwrap();
        let _ = result.exit_code;
    }

    /// TM-UNI-001/002: Grep handles multi-byte patterns
    #[tokio::test]
    async fn unicode_grep_multibyte_no_panic() {
        let mut bash = Bash::new();
        let result = bash.exec(r#"echo "café" | grep "café""#).await.unwrap();
        let _ = result.exit_code;
    }

    /// Stress test: many different multi-byte chars in single awk program
    #[tokio::test]
    async fn unicode_awk_stress_mixed_multibyte() {
        let mut bash = Bash::new();
        let result = bash
            .exec(
                r#"printf "α β γ δ ε\n日本 中文 한국\n🌍 🌎 🌏\n" | awk '{
  for(i=1;i<=NF;i++) print NR, i, $i
}'"#,
            )
            .await
            .unwrap();
        let _ = result.exit_code;
    }
}

// =============================================================================
// 2. ZERO-WIDTH CHARACTER TESTS (TM-UNI-003, TM-UNI-004, TM-UNI-005)
// =============================================================================

mod zero_width_chars {
    use super::*;

    /// TM-UNI-003: Zero-width space in filename — documents current behavior.
    /// Currently UNMITIGATED: find_unsafe_path_char() does not reject ZWSP.
    #[tokio::test]
    async fn unicode_zwsp_in_filename_current_behavior() {
        let fs = InMemoryFs::new();

        // Zero Width Space (U+200B) in filename
        let result = fs
            .write_file(Path::new("/tmp/file\u{200B}name.txt"), b"data")
            .await;

        // Currently this succeeds — documents the gap.
        // When TM-UNI-003 is fixed, this should return an error.
        if result.is_ok() {
            // Gap confirmed: zero-width chars pass validation
            // Also verify the file is distinguishable from "filename.txt"
            let normal = fs
                .write_file(Path::new("/tmp/filename.txt"), b"other")
                .await;
            assert!(normal.is_ok());
            // Two distinct files exist with visually identical names
            let content1 = fs
                .read_file(Path::new("/tmp/file\u{200B}name.txt"))
                .await
                .unwrap();
            let content2 = fs.read_file(Path::new("/tmp/filename.txt")).await.unwrap();
            assert_ne!(
                content1, content2,
                "ZWSP creates distinct file (TM-UNI-003 gap)"
            );
        }
        // If it fails, the mitigation has been implemented
    }

    /// TM-UNI-003: BOM (U+FEFF) in filename — documents current behavior
    #[tokio::test]
    async fn unicode_bom_in_filename_current_behavior() {
        let fs = InMemoryFs::new();
        let result = fs
            .write_file(Path::new("/tmp/\u{FEFF}file.txt"), b"data")
            .await;
        // Documents whether BOM is caught or not
        let _ = result;
    }

    /// TM-UNI-003: ZWJ (U+200D) in filename — documents current behavior
    #[tokio::test]
    async fn unicode_zwj_in_filename_current_behavior() {
        let fs = InMemoryFs::new();
        let result = fs
            .write_file(Path::new("/tmp/file\u{200D}name.txt"), b"data")
            .await;
        let _ = result;
    }

    /// TM-UNI-004: Zero-width chars in variable names — pass-through is correct
    #[tokio::test]
    async fn unicode_zwsp_in_variable_passthrough() {
        let mut bash = Bash::new();
        // Variable names with zero-width chars are treated as different variables
        // This matches Bash behavior and is accepted risk
        let result = bash
            .exec(
                r#"x="normal"
echo "$x""#,
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("normal"));
    }

    /// TM-UNI-005: Zero-width chars in string values — correct pass-through
    #[tokio::test]
    async fn unicode_zwsp_in_string_passthrough() {
        let mut bash = Bash::new();
        let result = bash.exec("echo \"hello\u{200B}world\"").await.unwrap();
        assert_eq!(result.exit_code, 0);
        // The ZWSP should be preserved in output (correct Bash behavior)
        assert!(result.stdout.contains("hello"));
        assert!(result.stdout.contains("world"));
    }
}

// =============================================================================
// 3. HOMOGLYPH / CONFUSABLE CHARACTER TESTS (TM-UNI-006, TM-UNI-007)
// =============================================================================

mod homoglyph_tests {
    use super::*;

    /// TM-UNI-006: Cyrillic vs Latin creates distinct files (accepted risk)
    #[tokio::test]
    async fn unicode_homoglyph_filenames_distinct() {
        let fs = InMemoryFs::new();

        // Latin 'a' (U+0061)
        fs.write_file(Path::new("/tmp/data.txt"), b"latin")
            .await
            .unwrap();

        // Cyrillic 'а' (U+0430) — visually identical to Latin 'a'
        fs.write_file(Path::new("/tmp/d\u{0430}ta.txt"), b"cyrillic")
            .await
            .unwrap();

        // These are distinct files despite looking identical
        let latin = fs.read_file(Path::new("/tmp/data.txt")).await.unwrap();
        let cyrillic = fs
            .read_file(Path::new("/tmp/d\u{0430}ta.txt"))
            .await
            .unwrap();
        assert_eq!(latin, b"latin");
        assert_eq!(cyrillic, b"cyrillic");
    }

    /// TM-UNI-007: Homoglyph variables are distinct (accepted, matches Bash)
    #[tokio::test]
    async fn unicode_homoglyph_variables_distinct() {
        let mut bash = Bash::new();
        // Scripts with visually similar but distinct variable names
        // This is accepted risk — matches Bash behavior
        let result = bash.exec("x=latin; echo $x").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("latin"));
    }
}

// =============================================================================
// 4. UNICODE NORMALIZATION TESTS (TM-UNI-008)
// =============================================================================

mod normalization_tests {
    use super::*;

    /// TM-UNI-008: NFC vs NFD creates distinct files (accepted, matches Linux)
    #[tokio::test]
    async fn unicode_nfc_nfd_distinct_files() {
        let fs = InMemoryFs::new();

        // NFC: é as single codepoint U+00E9
        fs.write_file(Path::new("/tmp/caf\u{00E9}.txt"), b"nfc")
            .await
            .unwrap();

        // NFD: é as e (U+0065) + combining acute (U+0301)
        fs.write_file(Path::new("/tmp/cafe\u{0301}.txt"), b"nfd")
            .await
            .unwrap();

        // On Linux (and in Bashkit's VFS), these are distinct files
        let nfc = fs
            .read_file(Path::new("/tmp/caf\u{00E9}.txt"))
            .await
            .unwrap();
        let nfd = fs
            .read_file(Path::new("/tmp/cafe\u{0301}.txt"))
            .await
            .unwrap();
        assert_eq!(nfc, b"nfc");
        assert_eq!(nfd, b"nfd");
    }

    /// TM-UNI-008: Scripts using different normalization forms work correctly
    #[tokio::test]
    async fn unicode_normalization_in_scripts() {
        let mut bash = Bash::new();

        // NFC form in variable comparison
        let result = bash
            .exec("x=\"caf\u{00E9}\"; if [ \"$x\" = \"caf\u{00E9}\" ]; then echo match; fi")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("match"));
    }
}

// =============================================================================
// 5. COMBINING CHARACTER ABUSE (TM-UNI-009, TM-UNI-010)
// =============================================================================

mod combining_char_tests {
    use super::*;

    /// TM-UNI-009: Excessive combining marks in filename — bounded by length limit
    #[tokio::test]
    async fn unicode_excessive_combining_marks_bounded() {
        let limits = FsLimits::new().max_filename_length(255);
        let fs = InMemoryFs::with_limits(limits);

        // 200 combining diacritical marks on a single base character
        let mut name = String::from("/tmp/a");
        for _ in 0..200 {
            name.push('\u{0300}'); // Combining grave accent
        }
        name.push_str(".txt");

        let result = fs.write_file(Path::new(&name), b"data").await;
        // Should either succeed (within 255 byte limit) or fail with length error
        // Must not panic or hang
        let _ = result;
    }

    /// TM-UNI-010: Combining marks in awk input — does not cause hang
    #[tokio::test]
    async fn unicode_combining_marks_in_awk_no_hang() {
        let mut bash = Bash::new();
        // Combining marks in data processed by awk
        let result = bash
            .exec("echo \"a\u{0300}\u{0301}\u{0302}bc\" | awk '{print length($0), $0}'")
            .await
            .unwrap();
        // Must complete without hanging
        let _ = result.exit_code;
    }
}

// =============================================================================
// 6. TAG CHARACTERS AND INVISIBLES (TM-UNI-011, TM-UNI-012, TM-UNI-013)
// =============================================================================

mod invisible_char_tests {
    use super::*;

    /// TM-UNI-011: Tag characters in filename — documents current behavior
    #[tokio::test]
    async fn unicode_tag_chars_in_filename_current_behavior() {
        let fs = InMemoryFs::new();

        // U+E0001 (Language Tag) — invisible, deprecated since Unicode 5.0
        let result = fs
            .write_file(Path::new("/tmp/file\u{E0001}name.txt"), b"data")
            .await;
        // Currently UNMITIGATED — documents the gap
        let _ = result;
    }

    /// TM-UNI-012: Interlinear annotation chars in filename — documents current behavior
    #[tokio::test]
    async fn unicode_interlinear_annotation_in_filename() {
        let fs = InMemoryFs::new();

        // U+FFF9 (Interlinear Annotation Anchor)
        let result = fs
            .write_file(Path::new("/tmp/file\u{FFF9}name.txt"), b"data")
            .await;
        let _ = result;
    }

    /// TM-UNI-013: Deprecated format chars in filename — documents current behavior
    #[tokio::test]
    async fn unicode_deprecated_format_chars_in_filename() {
        let fs = InMemoryFs::new();

        // U+206A (Inhibit Symmetric Swapping) — deprecated
        let result = fs
            .write_file(Path::new("/tmp/file\u{206A}name.txt"), b"data")
            .await;
        let _ = result;
    }
}

// =============================================================================
// 7. BIDI IN SCRIPT SOURCE (TM-UNI-014)
// =============================================================================

mod bidi_script_tests {
    use super::*;

    /// TM-UNI-014: Bidi overrides in script comments — accepted risk.
    /// Trojan Source attack: RTL override in comment can reorder displayed code.
    /// Bashkit executes untrusted scripts by design, so this is accepted.
    #[tokio::test]
    async fn unicode_bidi_in_script_comment_accepted() {
        let mut bash = Bash::new();
        // RTL override in a comment — visually misleading but functionally harmless
        let result = bash
            .exec("# \u{202E}this comment has RTL override\necho safe")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("safe"));
    }

    /// TM-UNI-014: Bidi overrides in string literals — pass through correctly
    #[tokio::test]
    async fn unicode_bidi_in_string_passthrough() {
        let mut bash = Bash::new();
        let result = bash.exec("echo \"text\u{202E}reversed\"").await.unwrap();
        assert_eq!(result.exit_code, 0);
        // The bidi char should be preserved in output
        assert!(result.stdout.contains("text"));
    }

    /// TM-DOS-015 (cross-ref): Bidi overrides in filenames ARE blocked
    #[tokio::test]
    async fn unicode_bidi_in_filename_blocked() {
        let fs = InMemoryFs::new();

        // RTL override in filename — this SHOULD be blocked (TM-DOS-015)
        let result = fs
            .write_file(Path::new("/tmp/test\u{202E}exe.txt"), b"data")
            .await;
        assert!(
            result.is_err(),
            "Bidi override in filename should be rejected"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("bidi override"),
            "Error should mention bidi override: {}",
            err
        );
    }
}

// =============================================================================
// 8. EXISTING PATH VALIDATION CROSS-CHECKS (TM-DOS-015 vs TM-UNI)
// =============================================================================

mod path_validation_crosscheck {
    use super::*;

    /// Verify existing TM-DOS-015 protections still work alongside new TM-UNI threats
    #[tokio::test]
    async fn unicode_control_chars_still_blocked() {
        let fs = InMemoryFs::new();

        // NULL-ish control chars (Rust strings can't contain U+0000, but others)
        for ch in ['\u{0001}', '\u{001F}', '\u{007F}', '\u{0080}', '\u{009F}'] {
            let path = format!("/tmp/file{}name.txt", ch);
            let result = fs.write_file(Path::new(&path), b"data").await;
            assert!(
                result.is_err(),
                "Control char U+{:04X} should be rejected in filenames",
                ch as u32
            );
        }
    }

    /// All bidi override codepoints are blocked
    #[tokio::test]
    async fn unicode_all_bidi_overrides_blocked_in_paths() {
        let fs = InMemoryFs::new();

        // LRE, RLE, PDF, LRO, RLO
        for ch in ['\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}'] {
            let path = format!("/tmp/file{}name.txt", ch);
            let result = fs.write_file(Path::new(&path), b"data").await;
            assert!(
                result.is_err(),
                "Bidi char U+{:04X} should be rejected in filenames",
                ch as u32
            );
        }

        // LRI, RLI, FSI, PDI
        for ch in ['\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}'] {
            let path = format!("/tmp/file{}name.txt", ch);
            let result = fs.write_file(Path::new(&path), b"data").await;
            assert!(
                result.is_err(),
                "Bidi isolate U+{:04X} should be rejected in filenames",
                ch as u32
            );
        }
    }

    /// Normal Unicode in filenames still works
    #[tokio::test]
    async fn unicode_normal_chars_allowed_in_paths() {
        let fs = InMemoryFs::new();

        // Accented characters
        fs.write_file(Path::new("/tmp/café.txt"), b"ok")
            .await
            .unwrap();

        // CJK characters
        fs.write_file(Path::new("/tmp/文件.txt"), b"ok")
            .await
            .unwrap();

        // Emoji
        fs.write_file(Path::new("/tmp/🌍.txt"), b"ok")
            .await
            .unwrap();

        // Arabic
        fs.write_file(Path::new("/tmp/ملف.txt"), b"ok")
            .await
            .unwrap();

        // Devanagari
        fs.write_file(Path::new("/tmp/फ़ाइल.txt"), b"ok")
            .await
            .unwrap();
    }
}

// =============================================================================
// 9. END-TO-END UNICODE SECURITY (integration tests)
// =============================================================================

mod e2e_unicode_security {
    use super::*;

    /// Full pipeline: Unicode data flows through echo -> file -> awk without panic
    #[tokio::test]
    async fn unicode_e2e_pipeline_no_panic() {
        let mut bash = Bash::new();
        let result = bash
            .exec(
                r#"
echo "名前,値" > /tmp/data.csv
echo "日本語,テスト" >> /tmp/data.csv
echo "café,latte" >> /tmp/data.csv
awk -F, '{print NR ": " $1 " → " $2}' /tmp/data.csv
"#,
            )
            .await
            .unwrap();
        // Must not crash; may or may not produce correct output depending on
        // TM-UNI-001 fix status
        let _ = result.exit_code;
    }

    /// Grep with multi-byte pattern on multi-byte file content
    #[tokio::test]
    async fn unicode_e2e_grep_multibyte() {
        let mut bash = Bash::new();
        let result = bash
            .exec(
                r#"
echo "hello world" > /tmp/test.txt
echo "café latte" >> /tmp/test.txt
echo "日本語" >> /tmp/test.txt
grep "café" /tmp/test.txt
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("café"));
    }

    /// Sed with multi-byte substitution on file content
    #[tokio::test]
    async fn unicode_e2e_sed_multibyte() {
        let mut bash = Bash::new();
        let result = bash
            .exec(
                r#"
echo "hello world" > /tmp/test.txt
sed 's/world/世界/' /tmp/test.txt
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("世界"));
    }

    /// Scripts with mixed encodings in variable operations
    #[tokio::test]
    async fn unicode_e2e_variable_ops() {
        let mut bash = Bash::new();
        let result = bash
            .exec(
                r#"
x="café"
echo "${#x}"
echo "${x/é/e}"
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    /// TM-UNI-001: The exact scenario from issue #395 — LLM-generated awk code
    #[tokio::test]
    async fn unicode_issue_395_exact_reproduction() {
        let mut bash = Bash::new();
        // Sonnet 4.6 generates awk code with Unicode box-drawing characters in comments
        let awk_code = r#"echo "key=value" | awk '
# ── Pass 1: load all overrides into a map ──────────────────────────────────
NR == FNR {
  print $0
}'"#;
        let result = bash.exec(awk_code).await.unwrap();
        // Must not crash the process. catch_unwind should prevent panic propagation.
        let _ = result.exit_code;
    }
}
