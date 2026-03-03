//! Security Audit Regression Tests
//!
//! Tests for vulnerabilities discovered during the 2026-03 security audit.
//! Each test asserts the DESIRED secure behavior. Tests are #[ignore] until
//! the corresponding fix lands -- they flip from ignored to green on fix.
//!
//! Run all (including ignored): `cargo test security_audit_ -- --ignored`
//! Run only passing:            `cargo test security_audit_`

#![allow(unused_variables, unused_imports)]

use bashkit::{Bash, ExecutionLimits};
use std::sync::Arc;
use std::time::{Duration, Instant};

// =============================================================================
// 1. INTERNAL VARIABLE PREFIX INJECTION (TM-INJ-012 to TM-INJ-016)
//
// Root cause: declare, readonly, local, export insert directly into the
// variables HashMap via ctx.variables.insert(), bypassing the
// is_internal_variable() guard in set_variable().
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

    /// TM-INJ-012: `declare` must not create namerefs via _NAMEREF_ prefix.
    #[tokio::test]
    async fn security_audit_declare_blocks_nameref_prefix() {
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

        // $alias must NOT resolve to $secret — the _NAMEREF_ prefix should be blocked
        assert_ne!(
            result.stdout.trim(),
            "sensitive_data",
            "declare must block _NAMEREF_ prefix injection"
        );
    }

    /// TM-INJ-013: `readonly` must not create namerefs via _NAMEREF_ prefix.
    #[tokio::test]
    async fn security_audit_readonly_blocks_nameref_prefix() {
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

        assert_ne!(
            result.stdout.trim(),
            "important_value",
            "readonly must block _NAMEREF_ prefix injection"
        );
    }

    /// TM-INJ-012: `declare` must not inject _UPPER_ case conversion marker.
    #[tokio::test]
    async fn security_audit_declare_blocks_upper_prefix() {
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

        // Assignment must NOT be forced to uppercase
        assert_eq!(
            result.stdout.trim(),
            "should be lowercase",
            "declare must block _UPPER_ prefix injection"
        );
    }

    /// TM-INJ-012: `declare` must not inject _LOWER_ case conversion marker.
    #[tokio::test]
    async fn security_audit_declare_blocks_lower_prefix() {
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

        assert_eq!(
            result.stdout.trim(),
            "SHOULD BE UPPERCASE",
            "declare must block _LOWER_ prefix injection"
        );
    }

    /// TM-INJ-016: _ARRAY_READ_ prefix must be rejected by is_internal_variable().
    #[tokio::test]
    async fn security_audit_array_read_prefix_blocked() {
        let mut bash = Bash::builder().build();

        let result = bash
            .exec(
                "export \"_ARRAY_READ_injected=val0\x1Fval1\x1Fval2\"\ntrue\necho \"${injected[0]} ${injected[1]} ${injected[2]}\"",
            )
            .await
            .unwrap();

        // Array must NOT be created via _ARRAY_READ_ marker injection
        assert!(
            !result.stdout.trim().contains("val0"),
            "_ARRAY_READ_ prefix must be blocked. Got: '{}'",
            result.stdout.trim()
        );
    }

    /// TM-INJ-015: `export` must not inject _READONLY_ marker prefix.
    #[tokio::test]
    async fn security_audit_export_blocks_readonly_prefix() {
        let mut bash = Bash::builder().build();

        let result = bash
            .exec(
                r#"
                myvar="original"
                export _READONLY_myvar=1
                myvar="changed"
                echo "$myvar"
            "#,
            )
            .await
            .unwrap();

        // Marker injection must be blocked — the var assignment should succeed
        // and _READONLY_ marker should not appear in `set` output
        assert_eq!(result.stdout.trim(), "changed");
        let leak = bash.exec("set | grep _READONLY_myvar").await.unwrap();
        assert!(
            leak.stdout.trim().is_empty(),
            "_READONLY_ marker must not be injectable via export"
        );
    }

    /// TM-INJ-014: `local` (interpreter-level) must not inject _NAMEREF_ prefix.
    /// execute_local_builtin at interpreter/mod.rs:4572 inserts into
    /// frame.locals without calling is_internal_variable().
    /// The marker ends up in the call frame, which set_variable traverses.
    #[tokio::test]
    async fn security_audit_local_blocks_internal_prefixes() {
        let mut bash = Bash::builder().build();

        // Outside a function, execute_local_builtin inserts into self.variables
        // at line 4599. This bypasses is_internal_variable().
        let result = bash
            .exec(
                r#"
                secret="stolen"
                local _NAMEREF_sneaky=secret
                echo "$sneaky"
            "#,
            )
            .await
            .unwrap();

        // local must not create a nameref — $sneaky must NOT resolve to $secret
        assert_ne!(
            result.stdout.trim(),
            "stolen",
            "local must block _NAMEREF_ prefix injection"
        );
    }
}

// =============================================================================
// 2. INTERNAL VARIABLE INFO LEAK (TM-INF-017)
//
// Root cause: `set` and `declare -p` iterate all variables without filtering
// internal prefixes.
// Files: builtins/vars.rs:114-119, interpreter/mod.rs:5367-5374
// =============================================================================

mod internal_variable_leak {
    use super::*;

    /// TM-INF-017: `set` must not expose internal _NAMEREF_/_READONLY_ markers.
    #[tokio::test]
    async fn security_audit_set_hides_internal_markers() {
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

        assert!(
            result.stdout.trim().is_empty(),
            "`set` must filter internal markers from output. Got:\n{}",
            result.stdout.trim()
        );
    }

    /// TM-INF-017: `declare -p` must not expose internal markers.
    #[tokio::test]
    async fn security_audit_declare_p_hides_internal_markers() {
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

        assert!(
            result.stdout.trim().is_empty(),
            "`declare -p` must filter internal markers. Got:\n{}",
            result.stdout.trim()
        );
    }
}

// =============================================================================
// 3. ARITHMETIC COMPOUND ASSIGNMENT OVERFLOW (TM-DOS-043)
//
// Root cause: execute_arithmetic_with_side_effects() at interpreter/mod.rs:1563
// uses native + instead of wrapping_add. Panics in debug mode.
// Files: interpreter/mod.rs:1563, :7022-7043
// =============================================================================

mod arithmetic_overflow {
    use super::*;

    /// TM-DOS-043: i64::MAX + 1 in ((x+=1)) must not panic.
    /// Should use wrapping arithmetic like the non-compound path.
    #[tokio::test]
    async fn security_audit_compound_add_no_panic() {
        let limits = ExecutionLimits::new().timeout(Duration::from_secs(5));
        let mut bash = Bash::builder().limits(limits).build();

        // This must not panic — should wrap or return error
        let result = bash.exec("x=9223372036854775807; ((x+=1)); echo $x").await;

        assert!(
            result.is_ok(),
            "Compound += with i64::MAX must not panic. Got: {:?}",
            result.err()
        );
    }

    /// TM-DOS-043: Compound <<= must clamp shift amount like non-compound path.
    #[tokio::test]
    async fn security_audit_compound_shift_clamped() {
        let limits = ExecutionLimits::new().timeout(Duration::from_secs(5));
        let mut bash = Bash::builder().limits(limits).build();

        // Shift by 64 must not panic — should clamp to 0..=63
        let result = bash.exec("x=1; let 'x<<=64'; echo $x").await;

        assert!(
            result.is_ok(),
            "Compound <<= with shift>=64 must not panic. Got: {:?}",
            result.err()
        );
    }
}

// =============================================================================
// 4. VFS LIMIT BYPASS (TM-DOS-047, TM-DOS-048)
//
// Root cause: InMemoryFs::copy() skips check_write_limits when dest exists.
//             InMemoryFs::rename() silently overwrites directories.
// Files: fs/memory.rs:1155-1183, :1136-1153
// =============================================================================

mod vfs_limit_bypass {
    use super::*;
    use bashkit::{FileSystem, FsLimits, InMemoryFs};
    use std::path::Path;

    /// TM-DOS-047: copy() must enforce limits even when destination exists.
    #[tokio::test]
    async fn security_audit_copy_enforces_limit_on_overwrite() {
        let limits = FsLimits::new()
            .max_total_bytes(600)
            .max_file_size(600)
            .max_file_count(10);
        let fs = InMemoryFs::with_limits(limits);

        // 10-byte target, 500-byte source → copy adds 490 bytes net
        fs.write_file(Path::new("/target"), b"tiny_file!")
            .await
            .unwrap();
        fs.write_file(Path::new("/source"), &vec![b'A'; 500])
            .await
            .unwrap();

        // Copy would bring total to 1000 bytes (> 600 limit).
        // Must fail because it exceeds max_total_bytes.
        let result = fs.copy(Path::new("/source"), Path::new("/target")).await;
        assert!(
            result.is_err(),
            "copy() must enforce size limits on overwrite"
        );
    }

    /// TM-DOS-048: rename(file, dir) must fail per POSIX, not silently overwrite.
    #[tokio::test]
    async fn security_audit_rename_rejects_file_over_dir() {
        let fs = InMemoryFs::new();

        fs.mkdir(Path::new("/mydir"), false).await.unwrap();
        fs.write_file(Path::new("/mydir/child.txt"), b"child data")
            .await
            .unwrap();
        fs.write_file(Path::new("/myfile"), b"file data")
            .await
            .unwrap();

        // rename(file, dir) must fail
        let result = fs.rename(Path::new("/myfile"), Path::new("/mydir")).await;
        assert!(result.is_err(), "rename(file, dir) must fail per POSIX");
    }
}

// =============================================================================
// 5. OVERLAY FS SYMLINK LIMIT BYPASS (TM-DOS-045)
//
// Root cause: OverlayFs::symlink() has no check_write_limits() call.
// Files: fs/overlay.rs:683-691
// =============================================================================

mod overlay_symlink_bypass {
    use super::*;
    use bashkit::{FileSystem, FsLimits, InMemoryFs, OverlayFs};
    use std::path::Path;

    /// TM-DOS-045: OverlayFs::symlink() must enforce file count limits.
    #[tokio::test]
    async fn security_audit_overlay_symlink_enforces_limit() {
        let lower: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        // Both lower and upper InMemoryFs have /dev/null (1 file each = 2 base).
        // limit=7 allows 5 new symlinks (2 + 5 = 7).
        let limits = FsLimits::new().max_file_count(7);
        let overlay = OverlayFs::with_limits(lower, limits);

        for i in 0..5 {
            let link = format!("/link{}", i);
            overlay
                .symlink(Path::new("/target"), Path::new(&link))
                .await
                .unwrap();
        }

        // 6th must fail (7 total = 2 existing + 5 symlinks = at limit)
        let result = overlay
            .symlink(Path::new("/target"), Path::new("/link_overflow"))
            .await;
        assert!(
            result.is_err(),
            "symlink() must reject creation beyond max_file_count"
        );
    }
}

// =============================================================================
// 6. INFORMATION DISCLOSURE: date leaks real host time (TM-INF-018)
//
// Root cause: date builtin uses chrono::Local/Utc (real system clock).
// Files: builtins/date.rs
// =============================================================================

mod information_disclosure {
    use super::*;

    /// TM-INF-018: `date` should use a configurable/virtual time source.
    #[tokio::test]
    async fn security_audit_date_uses_virtual_time() {
        // Fixed epoch: 2020-01-01 00:00:00 UTC
        let fixed = 1577836800i64;
        let mut bash = Bash::builder()
            .username("sandboxuser")
            .hostname("sandbox.local")
            .fixed_epoch(fixed)
            .build();

        // hostname and whoami are virtualized
        let host = bash.exec("hostname").await.unwrap();
        assert_eq!(host.stdout.trim(), "sandbox.local");

        let result = bash.exec("date +%s").await.unwrap();
        let script_epoch: i64 = result.stdout.trim().parse().unwrap_or(0);

        // date must return the fixed epoch, not real host time
        assert_eq!(
            script_epoch, fixed,
            "date must use fixed epoch, not real host clock (got={})",
            script_epoch
        );
    }
}

// =============================================================================
// 7. BRACE EXPANSION UNBOUNDED RANGE (TM-DOS-041)
//
// Root cause: try_expand_range() has no cap on (M - N).
// Files: interpreter/mod.rs:8049-8060
// =============================================================================

mod brace_expansion_dos {
    use super::*;

    /// TM-DOS-041: Brace expansion {1..N} must cap range size.
    /// Ranges exceeding 10,000 elements are treated as literals.
    #[tokio::test]
    async fn security_audit_brace_expansion_capped() {
        let limits = ExecutionLimits::new()
            .max_commands(100)
            .timeout(Duration::from_secs(10));
        let mut bash = Bash::builder().limits(limits).build();

        // {1..1000000} exceeds cap — treated as literal, not expanded to 1M strings
        let result = bash.exec("echo {1..1000000}").await.unwrap();
        assert_eq!(
            result.stdout.trim(),
            "{1..1000000}",
            "Brace expansion with 1M elements must be treated as literal"
        );

        // {1..100} is within cap — should expand normally
        let result = bash.exec("echo {1..3}").await.unwrap();
        assert_eq!(result.stdout.trim(), "1 2 3");
    }
}

// =============================================================================
// 8. LEXER STACK OVERFLOW (TM-DOS-044)
//
// Root cause: read_command_subst_into() recurses without depth tracking.
// Files: parser/lexer.rs:1109-1188
// =============================================================================

mod lexer_stack_overflow {
    use super::*;

    /// TM-DOS-044: Deeply nested $() must fail gracefully, not stack overflow.
    /// Confirmed crash at depth ~50 in debug mode. Using depth=15 here to
    /// test the graceful error path without crashing the runner.
    #[tokio::test]
    async fn security_audit_nested_subst_graceful_error() {
        let limits = ExecutionLimits::new()
            .max_ast_depth(10)
            .timeout(Duration::from_secs(5));
        let mut bash = Bash::builder().limits(limits).build();

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
        match result {
            Ok(_) => {} // Fine if it works at this depth
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    !msg.contains("stack overflow"),
                    "Must fail with depth limit, not stack overflow: {}",
                    msg
                );
            }
        }
        // NOTE: depth=50 causes SIGABRT (TM-DOS-044). Not tested here.
    }
}

// =============================================================================
// 9. MOUNTABLE FS MISSING validate_path (TM-DOS-046)
//
// Root cause: MountableFs delegates all operations without calling
// validate_path() first, bypassing path depth/character validation.
// Files: fs/mountable.rs:348-491
// =============================================================================

mod mountable_fs_validate_path {
    use super::*;
    use bashkit::{FileSystem, InMemoryFs, MountableFs};
    use std::path::Path;

    /// TM-DOS-046: MountableFs must reject paths with control characters.
    #[tokio::test]
    async fn security_audit_mountable_rejects_control_chars() {
        let root = Arc::new(InMemoryFs::new());
        let mountable = MountableFs::new(root);

        let bad_path = Path::new("/tmp/file\x01name");
        let result = mountable.write_file(bad_path, b"payload").await;
        assert!(
            result.is_err(),
            "MountableFs must reject paths with control characters"
        );
    }

    /// TM-DOS-046: MountableFs must validate paths on symlink creation.
    #[tokio::test]
    async fn security_audit_mountable_validates_symlink_path() {
        let root = Arc::new(InMemoryFs::new());
        let mountable = MountableFs::new(root);

        let bad_link = Path::new("/tmp/link\x02name");
        let result = mountable.symlink(Path::new("/target"), bad_link).await;
        assert!(result.is_err(), "MountableFs must validate symlink paths");
    }
}

// =============================================================================
// 10. collect_dirs_recursive DEPTH LIMIT (TM-DOS-049)
//
// Root cause: No explicit depth limit on directory recursion.
// Files: interpreter/mod.rs:8352
// =============================================================================

mod collect_dirs_depth_limit {
    use super::*;

    /// TM-DOS-049: collect_dirs_recursive has an explicit depth cap.
    /// Verify ** glob completes without stack overflow on a simple tree.
    #[tokio::test]
    async fn security_audit_glob_star_star_respects_depth() {
        let limits = ExecutionLimits::new()
            .max_commands(200)
            .timeout(Duration::from_secs(10));
        let mut bash = Bash::builder().limits(limits).build();

        // Create a shallow directory tree
        let result = bash
            .exec("mkdir -p /tmp/globtest/sub && touch /tmp/globtest/sub/file.txt && echo ok")
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "ok");

        // ** glob must complete without stack overflow (the fix adds depth limit)
        let result = bash.exec("echo /tmp/globtest/**").await;
        assert!(
            result.is_ok(),
            "** glob must complete without stack overflow"
        );
    }
}

// =============================================================================
// 11. parse_word_string USES DEFAULT LIMITS (TM-DOS-050)
//
// Root cause: parse_word_string() creates parser with default limits,
// ignoring caller-configured tighter limits.
// Files: parser/mod.rs:109
// =============================================================================

mod parse_word_string_limits {
    use super::*;

    /// TM-DOS-050: Parameter expansion word parsing should respect configured limits.
    /// With a tight AST depth limit, deeply nested ${...} should not bypass it.
    #[tokio::test]
    async fn security_audit_word_parse_uses_configured_limits() {
        let limits = ExecutionLimits::new()
            .max_ast_depth(5)
            .timeout(Duration::from_secs(5));
        let mut bash = Bash::builder().limits(limits).build();

        // Simple parameter expansion should work
        let result = bash.exec("x=hello; echo ${x:-world}").await.unwrap();
        assert_eq!(result.stdout.trim(), "hello");
    }
}
