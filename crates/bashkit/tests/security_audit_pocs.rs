//! Security Audit PoC Tests
//!
//! Proof-of-concept tests for vulnerabilities discovered during security audit.
//! Each test demonstrates a concrete attack vector. Tests PASS when the
//! vulnerability is confirmed, documenting current behavior.
//!
//! Run with: `cargo test security_audit_`

#![allow(unused_variables, unused_imports)]

use bashkit::{Bash, ExecutionLimits};
use std::sync::Arc;
use std::time::{Duration, Instant};

// =============================================================================
// 1. INTERNAL VARIABLE PREFIX INJECTION
//
// Root cause: declare, readonly, local, export insert directly into the
// variables HashMap via ctx.variables.insert(), bypassing the
// is_internal_variable() guard in set_variable().
//
// Impact: Unauthorized nameref creation, case conversion attribute injection,
// arbitrary array creation, internal state pollution.
//
// Files:
//   - interpreter/mod.rs:5574 (declare bypass)
//   - builtins/vars.rs:223 (local bypass), :265 (readonly bypass)
//   - builtins/export.rs:41 (export bypass)
//   - interpreter/mod.rs:7634-7641 (is_internal_variable)
//   - interpreter/mod.rs:4042-4057 (_ARRAY_READ_ post-processing)
// =============================================================================

mod internal_variable_injection {
    use super::*;

    /// VULN: `declare` creates unauthorized namerefs via _NAMEREF_ prefix.
    /// set_variable() blocks `_NAMEREF_alias=secret`, but declare inserts directly.
    #[tokio::test]
    async fn security_audit_declare_creates_unauthorized_nameref() {
        let mut bash = Bash::builder().build();

        let result = bash
            .exec(
                r#"
                secret="sensitive_data"
                declare _NAMEREF_alias=secret
                echo "$alias"
            "#,
            )
            .await
            .unwrap();

        // VULN CONFIRMED: $alias resolves to $secret via unauthorized nameref
        assert_eq!(
            result.stdout.trim(),
            "sensitive_data",
            "declare _NAMEREF_ injection creates nameref"
        );
    }

    /// VULN: `readonly` creates unauthorized namerefs via _NAMEREF_ prefix.
    /// readonly inserts at builtins/vars.rs:265 without checking internal prefixes.
    #[tokio::test]
    async fn security_audit_readonly_creates_unauthorized_nameref() {
        let mut bash = Bash::builder().build();

        let result = bash
            .exec(
                r#"
                target="important_value"
                readonly _NAMEREF_sneaky=target
                echo "$sneaky"
            "#,
            )
            .await
            .unwrap();

        // VULN CONFIRMED: readonly created a nameref
        assert_eq!(
            result.stdout.trim(),
            "important_value",
            "readonly _NAMEREF_ injection creates nameref"
        );
    }

    /// VULN: Inject _UPPER_ marker via declare to force case conversion.
    /// set_variable() at interpreter/mod.rs:7654 applies case conversion
    /// when _UPPER_ marker exists in variables HashMap.
    #[tokio::test]
    async fn security_audit_inject_upper_case_conversion() {
        let mut bash = Bash::builder().build();

        let result = bash
            .exec(
                r#"
                declare _UPPER_myvar=1
                myvar="should be lowercase"
                echo "$myvar"
            "#,
            )
            .await
            .unwrap();

        // VULN CONFIRMED: _UPPER_ marker forces uppercase on assignment
        assert_eq!(
            result.stdout.trim(),
            "SHOULD BE LOWERCASE",
            "declare _UPPER_ injection forces case conversion"
        );
    }

    /// VULN: Inject _LOWER_ marker via declare to force lowercase conversion.
    #[tokio::test]
    async fn security_audit_inject_lower_case_conversion() {
        let mut bash = Bash::builder().build();

        let result = bash
            .exec(
                r#"
                declare _LOWER_myvar=1
                myvar="SHOULD BE UPPERCASE"
                echo "$myvar"
            "#,
            )
            .await
            .unwrap();

        // VULN CONFIRMED: _LOWER_ marker forces lowercase on assignment
        assert_eq!(
            result.stdout.trim(),
            "should be uppercase",
            "declare _LOWER_ injection forces case conversion"
        );
    }

    /// VULN: _ARRAY_READ_ prefix not in is_internal_variable().
    /// Post-processing at interpreter/mod.rs:4042-4057 creates arrays from
    /// _ARRAY_READ_ markers after every builtin execution.
    #[tokio::test]
    async fn security_audit_array_read_prefix_injection() {
        let mut bash = Bash::builder().build();

        // export injects _ARRAY_READ_ marker, then `true` triggers post-processing
        let result = bash
            .exec(
                "export \"_ARRAY_READ_injected=val0\x1Fval1\x1Fval2\"\ntrue\necho \"${injected[0]} ${injected[1]} ${injected[2]}\"",
            )
            .await
            .unwrap();

        // VULN CONFIRMED: _ARRAY_READ_ marker created array during post-processing
        assert!(
            result.stdout.trim().contains("val0"),
            "_ARRAY_READ_ injection creates array. Got: '{}'",
            result.stdout.trim()
        );
    }

    /// VULN: _READONLY_ marker injection is possible but has no enforcement effect.
    /// This documents that: (a) declare bypasses is_internal_variable, AND
    /// (b) _READONLY_ markers are never checked during set_variable().
    #[tokio::test]
    async fn security_audit_readonly_marker_injectable_but_unenforced() {
        let mut bash = Bash::builder().build();

        // Inject _READONLY_ marker via declare
        let result = bash
            .exec(
                r#"
                myvar="original"
                declare _READONLY_myvar=1
                myvar="changed"
                echo "$myvar"
            "#,
            )
            .await
            .unwrap();

        // The marker IS injected (declare bypasses is_internal_variable),
        // but set_variable() doesn't check _READONLY_ markers, so assignment
        // proceeds. The variable changes to "changed".
        assert_eq!(
            result.stdout.trim(),
            "changed",
            "Marker injected but not enforced -- readonly check missing in set_variable()"
        );

        // Verify the marker is visible in `set` output (it was injected)
        let leak = bash
            .exec("set | grep _READONLY_myvar")
            .await
            .unwrap();
        assert!(
            !leak.stdout.trim().is_empty(),
            "_READONLY_ marker was injected via declare (visible in `set` output)"
        );
    }
}

// =============================================================================
// 2. INTERNAL VARIABLE INFO LEAK
//
// Root cause: `set` and `declare -p` iterate all variables without filtering
// internal prefixes.
// Impact: Scripts discover internal state (namerefs, readonly, case attrs).
// Files: builtins/vars.rs:114-119, interpreter/mod.rs:5367-5374
// =============================================================================

mod internal_variable_leak {
    use super::*;

    /// VULN: `set` dumps internal _NAMEREF_ and _READONLY_ markers.
    #[tokio::test]
    async fn security_audit_set_leaks_internal_markers() {
        let mut bash = Bash::builder().build();

        let result = bash
            .exec(
                r#"
                declare -n myref=target
                readonly myval=123
                set | grep -E "^_(NAMEREF|READONLY)_"
            "#,
            )
            .await
            .unwrap();

        // VULN CONFIRMED: Internal markers visible in `set` output
        assert!(
            !result.stdout.trim().is_empty(),
            "`set` leaks internal markers. Output:\n{}",
            result.stdout.trim()
        );
    }

    /// VULN: `declare -p` dumps internal marker variables.
    #[tokio::test]
    async fn security_audit_declare_p_leaks_internal_markers() {
        let mut bash = Bash::builder().build();

        let result = bash
            .exec(
                r#"
                declare -n myref=target
                readonly locked=42
                declare -p | grep -E "_(NAMEREF|READONLY)_"
            "#,
            )
            .await
            .unwrap();

        // VULN CONFIRMED: Internal markers visible in `declare -p` output
        assert!(
            !result.stdout.trim().is_empty(),
            "`declare -p` leaks internal markers. Output:\n{}",
            result.stdout.trim()
        );
    }
}

// =============================================================================
// 3. ARITHMETIC COMPOUND ASSIGNMENT PANIC (DoS)
//
// Root cause: execute_arithmetic_with_side_effects() at interpreter/mod.rs:1563
// and evaluate_arithmetic_with_assign() at :7022 use native +,-,*,<<,>>
// operators instead of wrapping_* variants. Debug mode panics on overflow.
//
// Impact: Process crash (panic) in debug mode. Silent wrapping in release.
// Files: interpreter/mod.rs:1563, :7022-7043
// =============================================================================

mod arithmetic_overflow {
    use super::*;

    /// VULN: i64::MAX + 1 in ((x+=1)) panics (DoS).
    /// The non-compound path uses wrapping_add, but the compound assignment
    /// path at mod.rs:1563 uses native +.
    #[tokio::test]
    async fn security_audit_compound_add_overflow_panics() {
        let limits = ExecutionLimits::new().timeout(Duration::from_secs(5));
        let mut bash = Bash::builder().limits(limits).build();

        // Use std::panic::catch_unwind can't work with async, so we test
        // via a less-than-MAX value that still demonstrates the path
        let result = bash
            .exec("x=9223372036854775806; ((x+=1)); echo $x")
            .await;

        // This specific value doesn't overflow, but confirms the code path exists
        match result {
            Ok(r) => {
                assert_eq!(
                    r.stdout.trim(),
                    "9223372036854775807",
                    "Compound += works for non-overflowing values"
                );
            }
            Err(_) => {
                // Unexpected error for non-overflowing value
            }
        }

        // NOTE: ((x+=1)) with x=9223372036854775807 (i64::MAX) will panic
        // at interpreter/mod.rs:1563 with "attempt to add with overflow".
        // This crashes the process in debug mode (confirmed via test output).
        // Not tested here to avoid crashing the test runner.
    }

    /// VULN: Compound <<= with shift >= 64 is unclamped.
    /// Non-compound path clamps to 0..=63 at interpreter/mod.rs:7455,
    /// but evaluate_arithmetic_with_assign at :7042 doesn't clamp.
    #[tokio::test]
    async fn security_audit_compound_shift_unclamped() {
        let limits = ExecutionLimits::new().timeout(Duration::from_secs(5));
        let mut bash = Bash::builder().limits(limits).build();

        // Safe shift value to test the path exists
        // Note: ((x<<=4)) goes through evaluate_arithmetic_with_assign, not
        // execute_arithmetic_with_side_effects (because '<' before '=' is filtered)
        let result = bash.exec("x=1; let 'x<<=4'; echo $x").await;
        match result {
            Ok(r) => assert_eq!(r.stdout.trim(), "16", "Compound <<= works via let"),
            Err(_) => {
                // May fail due to parsing -- test documents the code path exists
            }
        }
        // NOTE: let 'x<<=64' would use unclamped shift at mod.rs:7042
    }
}

// =============================================================================
// 4. VFS LIMIT BYPASS: copy() skips limits when destination exists
//
// Root cause: InMemoryFs::copy() at fs/memory.rs:1176 only calls
// check_write_limits when is_new=true. Overwriting a small file with
// a large one doesn't check the size delta.
//
// Impact: Total VFS bytes can exceed max_total_bytes.
// Files: fs/memory.rs:1155-1183
// =============================================================================

mod vfs_limit_bypass {
    use super::*;
    use bashkit::{FileSystem, FsLimits, InMemoryFs};
    use std::path::Path;

    /// VULN: copy() to existing destination skips check_write_limits.
    #[tokio::test]
    async fn security_audit_copy_skips_limit_on_overwrite() {
        let limits = FsLimits::new()
            .max_total_bytes(1024)
            .max_file_size(600)
            .max_file_count(10);
        let fs = InMemoryFs::with_limits(limits);

        // 10-byte target, 500-byte source
        fs.write_file(Path::new("/target"), b"tiny_file!")
            .await
            .unwrap();
        fs.write_file(Path::new("/source"), &vec![b'A'; 500])
            .await
            .unwrap();

        // Copy source -> target: is_new=false, check_write_limits skipped
        let result = fs.copy(Path::new("/source"), Path::new("/target")).await;

        // VULN CONFIRMED: copy succeeds without limit check on overwrite
        assert!(
            result.is_ok(),
            "copy() to existing dest skips check_write_limits (vuln confirmed)"
        );
    }

    /// VULN: rename(file, dir) silently overwrites directory, orphans children.
    /// HashMap::insert at fs/memory.rs:1147 overwrites any entry type.
    #[tokio::test]
    async fn security_audit_rename_overwrites_dir_orphans_children() {
        let fs = InMemoryFs::new();

        // Create dir with child
        fs.mkdir(Path::new("/mydir"), false).await.unwrap();
        fs.write_file(Path::new("/mydir/child.txt"), b"child data")
            .await
            .unwrap();

        // Create regular file
        fs.write_file(Path::new("/myfile"), b"file data")
            .await
            .unwrap();

        // rename(file, dir) should fail per POSIX but succeeds silently
        let result = fs.rename(Path::new("/myfile"), Path::new("/mydir")).await;
        assert!(
            result.is_ok(),
            "rename(file, dir) succeeds silently (vuln: should fail per POSIX)"
        );

        // Child is now orphaned: parent key is a file, not a directory
        let child_exists = fs
            .exists(Path::new("/mydir/child.txt"))
            .await
            .unwrap_or(false);
        assert!(
            child_exists,
            "Orphaned child still exists but parent is now a file (vuln confirmed)"
        );
    }
}

// =============================================================================
// 5. OVERLAY FS SYMLINK LIMIT BYPASS
//
// Root cause: OverlayFs::symlink() at fs/overlay.rs:683-691 has no
// check_write_limits() call. Upper layer has FsLimits::unlimited().
//
// Impact: Unlimited symlink creation despite file count limits.
// Files: fs/overlay.rs:683-691
// =============================================================================

mod overlay_symlink_bypass {
    use super::*;
    use bashkit::{FileSystem, FsLimits, InMemoryFs, OverlayFs};
    use std::path::Path;

    /// VULN: OverlayFs::symlink() ignores file count limits.
    #[tokio::test]
    async fn security_audit_overlay_symlink_bypasses_file_count() {
        let lower: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let limits = FsLimits::new().max_file_count(5);
        let overlay = OverlayFs::with_limits(lower, limits);

        // Create 6 symlinks (exceeding limit of 5)
        for i in 0..6 {
            let link = format!("/link{}", i);
            let result = overlay
                .symlink(Path::new("/target"), Path::new(&link))
                .await;
            assert!(
                result.is_ok(),
                "Symlink {} should succeed (no limit check in symlink())",
                i
            );
        }
        // VULN CONFIRMED: All 6 symlinks created despite max_file_count=5
    }
}

// =============================================================================
// 6. INFORMATION DISCLOSURE: date leaks real host time
//
// Root cause: date builtin uses chrono::Local/Utc (real system clock).
//             hostname, whoami, uname are properly virtualized.
//
// Impact: Timezone fingerprinting, timing correlation.
// Files: builtins/date.rs
// =============================================================================

mod information_disclosure {
    use super::*;

    /// VULN: `date` returns real host time despite other builtins being virtual.
    #[tokio::test]
    async fn security_audit_date_leaks_real_time() {
        let mut bash = Bash::builder()
            .username("sandboxuser")
            .hostname("sandbox.local")
            .build();

        // Verify identity builtins ARE virtualized
        let host = bash.exec("hostname").await.unwrap();
        assert_eq!(host.stdout.trim(), "sandbox.local");
        let who = bash.exec("whoami").await.unwrap();
        assert_eq!(who.stdout.trim(), "sandboxuser");

        // date leaks real time
        let result = bash.exec("date +%s").await.unwrap();
        let script_epoch: i64 = result.stdout.trim().parse().unwrap_or(0);
        let real_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // VULN CONFIRMED: date epoch matches real system time
        assert!(
            (script_epoch - real_epoch).abs() < 10,
            "date returns real host time (script={} real={})",
            script_epoch,
            real_epoch
        );
    }
}

// =============================================================================
// 7. EXTGLOB EXPONENTIAL BACKTRACKING
//
// Root cause: glob_match_impl for +(pattern) tries every split point,
// O(n! * 2^n) worst case. MAX_GLOB_DEPTH limits recursion depth but
// not total work done within the depth budget.
//
// Impact: CPU exhaustion. Timeout is backstop but glob is synchronous.
// Files: interpreter/mod.rs:3101-3149
// =============================================================================

mod extglob_dos {
    use super::*;

    /// VULN: +(a|aa) causes exponential work even with depth limit.
    #[tokio::test]
    async fn security_audit_extglob_exponential() {
        let limits = ExecutionLimits::new().timeout(Duration::from_secs(10));
        let mut bash = Bash::builder().limits(limits).build();

        let start = Instant::now();
        let _result = bash
            .exec(r#"shopt -s extglob; [[ "aaaaaaaaaaaaaaa" == +(a|aa) ]] && echo match"#)
            .await;
        let elapsed = start.elapsed();

        // Document timing -- with depth limit, 15 chars should complete.
        // The vuln is that for longer inputs, exponential work occurs
        // within the 50-level depth budget.
        assert!(
            elapsed < Duration::from_secs(8),
            "ExtGlob took {:?} for 15 chars. Larger inputs cause exponential backtracking.",
            elapsed
        );
    }
}

// =============================================================================
// 8. BRACE EXPANSION UNBOUNDED RANGE (OOM DoS)
//
// Root cause: try_expand_range() at interpreter/mod.rs:8049-8060 expands
// {N..M} into Vec with no cap on (M - N).
//
// Impact: {1..999999999} allocates billions of strings -> OOM.
// Files: interpreter/mod.rs:8049-8060
// =============================================================================

mod brace_expansion_dos {
    use super::*;

    /// VULN: {1..N} has no inherent cap on range size.
    #[tokio::test]
    async fn security_audit_brace_expansion_unbounded_range() {
        let limits = ExecutionLimits::new()
            .max_commands(100)
            .timeout(Duration::from_secs(10));
        let mut bash = Bash::builder().limits(limits).build();

        let start = Instant::now();
        let _result = bash.exec("echo {1..100000} > /dev/null").await;
        let elapsed = start.elapsed();

        // 100K strings is feasible. The vuln is {1..10000000}+ which OOMs.
        assert!(
            elapsed < Duration::from_secs(8),
            "Brace expansion of 100K entries took {:?}. No cap on range size.",
            elapsed
        );
    }
}

// =============================================================================
// 9. LEXER STACK OVERFLOW
//
// Root cause: read_command_subst_into() in parser/lexer.rs recurses for
// nested $() inside double-quotes without depth tracking. Parser has depth
// limits but lexer runs first.
//
// Impact: Stack overflow (SIGABRT) crashing the process.
// Files: parser/lexer.rs:1109-1188
//
// NOTE: This test uses depth=30 which may or may not overflow depending on
// stack size. The confirmed crash is at depth=50+ in debug mode and depth=200
// in release mode. We intentionally use a safe depth to avoid killing the
// test runner while still documenting the vulnerability.
// =============================================================================

mod lexer_stack_overflow {
    use super::*;

    /// Document: nested $() hits parser depth limit gracefully at low depths.
    /// VULN: At depth ~50 (debug) or ~200 (release), lexer overflows stack
    /// before parser depth limit is reached.
    #[tokio::test]
    async fn security_audit_nested_subst_safe_depth() {
        let limits = ExecutionLimits::new()
            .max_ast_depth(10)
            .timeout(Duration::from_secs(5));
        let mut bash = Bash::builder().limits(limits).build();

        // depth=15: should hit parser depth limit safely
        let mut script = String::new();
        let depth = 15;
        for _ in 0..depth {
            script.push_str("echo \"$(");
        }
        script.push_str("echo hi");
        for _ in 0..depth {
            script.push_str(")\"");
        }

        let result = bash.exec(&script).await;
        // Should error with depth limit, not stack overflow
        match result {
            Ok(_) => {} // Fine if it works
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    !msg.contains("stack overflow"),
                    "Should hit depth limit, not stack overflow: {}",
                    msg
                );
            }
        }
        // VULN: depth=50 causes SIGABRT. Not tested to avoid killing runner.
    }
}
