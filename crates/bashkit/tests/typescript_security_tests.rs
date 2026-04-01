// Security tests for embedded TypeScript (ZapCode) integration.
//
// White-box tests: exploit knowledge of internals (VFS bridging, resource
// limits, external function handlers, path resolution).
//
// Black-box tests: treat ts as an opaque command and try to break out
// of the sandbox, exhaust resources, or leak information.
//
// Covers attack vectors: eval/import/require, resource exhaustion,
// VFS escape, path manipulation, error leakage, state persistence,
// and ZapCode interpreter edge cases.
//
// NOTE: TypeScript feature is opt-in. These tests verify that when enabled,
// the sandbox is robust.

#![cfg(feature = "typescript")]

use bashkit::{Bash, ExecutionLimits, TypeScriptLimits};
use std::time::Duration;

fn bash_ts() -> Bash {
    Bash::builder().typescript().build()
}

fn bash_ts_limits(limits: TypeScriptLimits) -> Bash {
    Bash::builder().typescript_with_limits(limits).build()
}

fn bash_ts_tight() -> Bash {
    bash_ts_limits(
        TypeScriptLimits::default()
            .max_duration(Duration::from_secs(3))
            .max_memory(4 * 1024 * 1024) // 4 MB
            .max_allocations(50_000)
            .max_stack_depth(100),
    )
}

// =============================================================================
// 1. BLACK-BOX: BLOCKED LANGUAGE FEATURES
//
// Try using language features that could escape the sandbox.
// =============================================================================

mod blackbox_blocked_features {
    use super::*;

    #[tokio::test]
    async fn no_eval() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"eval('console.log(\\\"hacked\\\")')\"")
            .await
            .unwrap();
        assert!(
            !r.stdout.contains("hacked"),
            "eval must not execute code, got: {}",
            r.stdout
        );
    }

    #[tokio::test]
    async fn no_function_constructor() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"const f = new Function('return 42'); console.log(f())\"")
            .await
            .unwrap();
        assert!(
            !r.stdout.contains("42") || r.exit_code != 0,
            "Function constructor must not work"
        );
    }

    #[tokio::test]
    async fn no_import() {
        let mut bash = bash_ts();
        let r = bash.exec("ts -c \"import fs from 'fs'\"").await.unwrap();
        assert_ne!(r.exit_code, 0, "import must fail");
    }

    #[tokio::test]
    async fn no_require() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"const fs = require('fs')\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "require must fail");
    }

    #[tokio::test]
    async fn no_dynamic_import() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"const m = await import('fs')\"")
            .await
            .unwrap();
        // Dynamic import should fail or be treated as unknown external function
        assert!(
            r.exit_code != 0 || !r.stdout.contains("readFile"),
            "dynamic import must not succeed"
        );
    }

    #[tokio::test]
    async fn no_process_global() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"console.log(process.env.HOME)\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "process global should not exist");
    }

    #[tokio::test]
    async fn no_deno_global() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"console.log(Deno.readTextFileSync('/etc/passwd'))\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "Deno global should not exist");
        assert!(
            !r.stdout.contains("root:"),
            "should not read host filesystem"
        );
    }

    #[tokio::test]
    async fn no_bun_global() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"Bun.file('/etc/passwd')\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "Bun global should not exist");
    }
}

// =============================================================================
// 2. BLACK-BOX: RESOURCE EXHAUSTION
//
// Try to exhaust CPU, memory, or stack via TypeScript code.
// =============================================================================

mod blackbox_resource_exhaustion {
    use super::*;

    /// TM-TS-001: Infinite loop blocked by time limit
    #[tokio::test]
    async fn threat_ts_infinite_loop() {
        let mut bash = bash_ts_tight();
        let r = bash.exec("ts -c \"while (true) {}\"").await.unwrap();
        assert_ne!(r.exit_code, 0, "infinite loop should not succeed");
    }

    /// TM-TS-002: Memory exhaustion blocked
    #[tokio::test]
    async fn threat_ts_memory_exhaustion() {
        let mut bash = bash_ts_tight();
        let r = bash
            .exec("ts -c \"const arr: number[] = []; while (true) { arr.push(1); }\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "memory bomb should not succeed");
    }

    /// TM-TS-003: Stack overflow blocked by depth limit
    #[tokio::test]
    async fn threat_ts_stack_overflow() {
        let mut bash = bash_ts_tight();
        let r = bash
            .exec("ts -c \"const f = (): number => f(); f()\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "stack overflow should not succeed");
    }

    /// TM-TS-004: Allocation bomb blocked
    #[tokio::test]
    async fn threat_ts_allocation_bomb() {
        let mut bash = bash_ts_tight();
        let r = bash
            .exec("ts -c \"for (let i = 0; i < 10000000; i++) { const x = [1,2,3]; }\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "allocation bomb should not succeed");
    }

    /// String bomb - exponential string growth
    #[tokio::test]
    async fn threat_ts_string_bomb() {
        let mut bash = bash_ts_tight();
        let r = bash
            .exec("ts -c \"let s = 'a'; for (let i = 0; i < 30; i++) { s = s + s; }\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "string bomb should be limited");
    }

    /// Generous limits should succeed for normal code
    #[tokio::test]
    async fn normal_code_within_limits() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"let sum = 0; for (let i = 0; i < 100; i++) { sum += i; } console.log(sum)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "4950");
    }
}

// =============================================================================
// 3. WHITE-BOX: VFS SECURITY
//
// Test that VFS bridging is secure and cannot escape the sandbox.
// =============================================================================

mod whitebox_vfs_security {
    use super::*;

    /// TM-TS-005: VFS reads from virtual filesystem, not host
    #[tokio::test]
    async fn threat_ts_vfs_no_real_fs() {
        let mut bash = bash_ts();
        // /etc/passwd exists on real Linux but not in VFS
        let r = bash
            .exec("ts -c \"const content = await readFile('/etc/passwd'); console.log(content)\"")
            .await
            .unwrap();
        // Should either error or return VFS content (which doesn't have real data)
        assert!(
            !r.stdout.contains("root:"),
            "must not read real /etc/passwd"
        );
    }

    /// TM-TS-006: VFS write stays in virtual filesystem
    #[tokio::test]
    async fn threat_ts_vfs_write_sandboxed() {
        let mut bash = bash_ts();
        let r = bash
            .exec(
                "ts -c \"await writeFile('/tmp/sandbox_test.txt', 'test'); await readFile('/tmp/sandbox_test.txt')\"",
            )
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "test");
    }

    /// TM-TS-007: Path traversal blocked
    #[tokio::test]
    async fn threat_ts_vfs_path_traversal() {
        let mut bash = bash_ts();
        let r = bash
            .exec(
                "ts -c \"const content = await readFile('/tmp/../../../etc/passwd'); console.log(content)\"",
            )
            .await
            .unwrap();
        assert!(
            !r.stdout.contains("root:"),
            "path traversal must not escape VFS"
        );
    }

    /// TM-TS-008: Bash/TypeScript VFS data flows correctly
    #[tokio::test]
    async fn threat_ts_vfs_bash_ts_shared() {
        let mut bash = bash_ts();
        // Write from bash, read from TypeScript
        let r = bash
            .exec("echo 'from bash' > /tmp/shared.txt\nts -c \"await readFile('/tmp/shared.txt')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("from bash"));
    }

    /// TM-TS-009: File not found handled gracefully (no crash)
    #[tokio::test]
    async fn threat_ts_vfs_error_handling() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"await readFile('/no/such/file.txt')\"")
            .await
            .unwrap();
        // Should return an error string, not crash
        assert!(
            r.stdout.contains("Error") || r.exit_code != 0,
            "missing file should be handled gracefully"
        );
    }

    /// TM-TS-010: VFS mkdir sandboxed
    #[tokio::test]
    async fn threat_ts_vfs_mkdir_sandboxed() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"await mkdir('/tmp/tsdir'); await exists('/tmp/tsdir')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "true");
    }

    /// TM-TS-011: VFS operations don't escape to host /tmp
    #[tokio::test]
    async fn threat_ts_vfs_no_host_escape() {
        let mut bash = bash_ts();
        bash.exec("ts -c \"await writeFile('/tmp/ts_escape_test', 'payload')\"")
            .await
            .unwrap();
        // Verify file doesn't exist on real host (we're in VFS)
        // The bash `test -f` in BashKit also operates on VFS, so this
        // verifies the write went to VFS, not a real assertion about host fs
        let r = bash.exec("cat /tmp/ts_escape_test").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "payload");
    }
}

// =============================================================================
// 4. WHITE-BOX: ERROR HANDLING SECURITY
//
// Errors should not leak internal information.
// =============================================================================

mod whitebox_error_handling {
    use super::*;

    /// TM-TS-012: Error output goes to stderr, not stdout
    #[tokio::test]
    async fn threat_ts_error_isolation() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"throw new Error('test error')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(
            r.stderr.contains("Error") || r.stderr.contains("error"),
            "error should be on stderr: '{}'",
            r.stderr
        );
    }

    /// TM-TS-013: Syntax error returns non-zero exit code
    #[tokio::test]
    async fn threat_ts_syntax_error_exit() {
        let mut bash = bash_ts();
        let r = bash.exec("ts -c \"if {\"").await.unwrap();
        assert_ne!(r.exit_code, 0, "syntax error should fail");
    }

    /// TM-TS-014: Exit code propagates to bash correctly
    #[tokio::test]
    async fn threat_ts_exit_code_propagation() {
        let mut bash = bash_ts();
        // Success case
        let r = bash
            .exec("ts -c \"console.log('ok')\"\necho $?")
            .await
            .unwrap();
        assert!(r.stdout.contains("0"), "success should give exit 0");

        // Failure case
        let r = bash
            .exec("ts -c \"throw new Error()\" 2>/dev/null\necho $?")
            .await
            .unwrap();
        assert!(r.stdout.contains("1"), "error should give exit 1");
    }

    /// TM-TS-015: Empty code fails gracefully
    #[tokio::test]
    async fn threat_ts_empty_code() {
        let mut bash = bash_ts();
        let r = bash.exec("ts -c \"\"").await.unwrap();
        assert_ne!(r.exit_code, 0);
    }

    /// TM-TS-016: Pipeline error handling
    #[tokio::test]
    async fn threat_ts_pipeline_error_handling() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"throw new Error('boom')\" 2>/dev/null | cat")
            .await
            .unwrap();
        assert!(
            !r.stdout.contains("Error"),
            "error should not leak to pipeline stdout"
        );
    }

    /// TM-TS-017: Unknown options rejected
    #[tokio::test]
    async fn threat_ts_unknown_options() {
        let mut bash = bash_ts();
        let r = bash.exec("ts --unsafe-eval code").await.unwrap();
        assert_ne!(r.exit_code, 0, "unknown options should be rejected");
    }
}

// =============================================================================
// 5. WHITE-BOX: BASH INTEGRATION SECURITY
//
// Verify TypeScript integrates safely with bash features.
// =============================================================================

mod whitebox_bash_integration {
    use super::*;

    /// TM-TS-018: TypeScript respects BashKit command limits
    #[tokio::test]
    async fn threat_ts_respects_bash_limits() {
        let limits = ExecutionLimits::new().max_commands(5);
        let mut bash = Bash::builder().typescript().limits(limits).build();
        // Each ts invocation is 1 command; should succeed with generous limits
        let r = bash.exec("ts -c \"console.log('ok')\"").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "ok\n");
    }

    /// TM-TS-019: Command substitution captures only stdout
    #[tokio::test]
    async fn threat_ts_subst_captures_stdout() {
        let mut bash = bash_ts();
        let r = bash
            .exec("result=$(ts -c \"console.log(42)\")\necho $result")
            .await
            .unwrap();
        assert_eq!(r.stdout.trim(), "42");
    }

    /// TM-TS-020: Bash variable expansion before TypeScript (by design)
    #[tokio::test]
    async fn threat_ts_variable_expansion() {
        let mut bash = bash_ts();
        // Double-quoted: bash expands $VAR before passing to ts
        bash.exec("export MYVAR=injected").await.unwrap();
        let r = bash.exec("ts -c \"console.log('$MYVAR')\"").await.unwrap();
        assert_eq!(r.stdout.trim(), "injected");

        // Single-quoted: no expansion (safe)
        let r = bash.exec("ts -c 'console.log(\"$MYVAR\")'").await.unwrap();
        assert_eq!(r.stdout.trim(), "$MYVAR");
    }

    /// TM-TS-021: TypeScript cannot execute shell commands
    #[tokio::test]
    async fn threat_ts_no_shell_exec() {
        let mut bash = bash_ts();
        // No way to execute shell commands from TypeScript
        let r = bash
            .exec("ts -c \"console.log(process.env)\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "process.env should not exist");
        assert!(
            !r.stdout.contains("hacked"),
            "should not execute shell commands"
        );
    }

    /// TM-TS-022: Script file from VFS (not host filesystem)
    #[tokio::test]
    async fn threat_ts_script_from_vfs() {
        let mut bash = bash_ts();
        // Write a script to VFS and execute it
        bash.exec("echo 'console.log(\"from vfs\")' > /tmp/script.ts")
            .await
            .unwrap();
        let r = bash.exec("ts /tmp/script.ts").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "from vfs");
    }

    /// TM-TS-023: Shebang line stripped safely
    #[tokio::test]
    async fn threat_ts_shebang_stripped() {
        let mut bash = bash_ts();
        bash.exec("printf '#!/usr/bin/env ts\\nconsole.log(\"safe\")' > /tmp/shebang.ts")
            .await
            .unwrap();
        let r = bash.exec("ts /tmp/shebang.ts").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "safe");
    }
}

// =============================================================================
// 6. WHITE-BOX: OPT-IN VERIFICATION
//
// Verify that TypeScript is NOT available unless explicitly opted in.
// =============================================================================

mod optin_verification {
    use bashkit::Bash;

    /// TypeScript commands are NOT registered by default
    #[tokio::test]
    async fn ts_not_available_by_default() {
        let mut bash = Bash::builder().build();
        let r = bash.exec("ts -c \"console.log('hi')\"").await.unwrap();
        assert_ne!(r.exit_code, 0, "ts should not be available without opt-in");
    }

    /// Node command is NOT registered by default
    #[tokio::test]
    async fn node_not_available_by_default() {
        let mut bash = Bash::builder().build();
        let r = bash.exec("node -e \"console.log('hi')\"").await.unwrap();
        assert_ne!(
            r.exit_code, 0,
            "node should not be available without opt-in"
        );
    }

    /// Deno command is NOT registered by default
    #[tokio::test]
    async fn deno_not_available_by_default() {
        let mut bash = Bash::builder().build();
        let r = bash.exec("deno -e \"console.log('hi')\"").await.unwrap();
        assert_ne!(
            r.exit_code, 0,
            "deno should not be available without opt-in"
        );
    }

    /// Bun command is NOT registered by default
    #[tokio::test]
    async fn bun_not_available_by_default() {
        let mut bash = Bash::builder().build();
        let r = bash.exec("bun -e \"console.log('hi')\"").await.unwrap();
        assert_ne!(r.exit_code, 0, "bun should not be available without opt-in");
    }

    /// TypeScript IS available after .typescript() builder call
    #[tokio::test]
    async fn ts_available_after_optin() {
        let mut bash = Bash::builder().typescript().build();
        let r = bash.exec("ts -c \"console.log('hi')\"").await.unwrap();
        assert_eq!(r.exit_code, 0, "ts should work after opt-in");
        assert_eq!(r.stdout.trim(), "hi");
    }

    /// All aliases available after .typescript()
    #[tokio::test]
    async fn all_aliases_available_after_optin() {
        let mut bash = Bash::builder().typescript().build();
        for cmd in &["ts", "typescript", "node", "deno", "bun"] {
            let flag = if *cmd == "ts" || *cmd == "typescript" {
                "-c"
            } else {
                "-e"
            };
            let r = bash
                .exec(&format!("{cmd} {flag} \"console.log('ok')\""))
                .await
                .unwrap();
            assert_eq!(r.exit_code, 0, "{cmd} should work after opt-in");
        }
    }

    /// When compat_aliases=false, only ts/typescript are registered
    #[tokio::test]
    async fn compat_aliases_disabled() {
        use bashkit::TypeScriptConfig;
        let mut bash = Bash::builder()
            .typescript_with_config(TypeScriptConfig::default().compat_aliases(false))
            .build();

        // ts and typescript should work
        let r = bash.exec("ts -c \"console.log('ok')\"").await.unwrap();
        assert_eq!(r.exit_code, 0, "ts should work");

        let r = bash
            .exec("typescript -c \"console.log('ok')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0, "typescript should work");

        // node, deno, bun should NOT be available
        for cmd in &["node", "deno", "bun"] {
            let r = bash
                .exec(&format!("{cmd} -e \"console.log('hi')\""))
                .await
                .unwrap();
            assert_ne!(
                r.exit_code, 0,
                "{cmd} should not be available with compat_aliases=false"
            );
        }
    }

    /// Unsupported mode hints show helpful text for node --inspect
    #[tokio::test]
    async fn unsupported_mode_hint_node_inspect() {
        let mut bash = Bash::builder().typescript().build();
        let r = bash.exec("node --inspect app.js").await.unwrap();
        assert_ne!(r.exit_code, 0);
        assert!(
            r.stderr.contains("hint:"),
            "should show hint text for --inspect"
        );
        assert!(
            r.stderr.contains("ZapCode"),
            "should mention ZapCode in hint"
        );
    }

    /// Unsupported mode hints show helpful text for deno subcommands
    #[tokio::test]
    async fn unsupported_mode_hint_deno_run() {
        let mut bash = Bash::builder().typescript().build();
        let r = bash.exec("deno run script.ts").await.unwrap();
        assert_ne!(r.exit_code, 0);
        assert!(r.stderr.contains("hint:"));
    }

    /// Unsupported mode hints show helpful text for bun subcommands
    #[tokio::test]
    async fn unsupported_mode_hint_bun_install() {
        let mut bash = Bash::builder().typescript().build();
        let r = bash.exec("bun install").await.unwrap();
        assert_ne!(r.exit_code, 0);
        assert!(r.stderr.contains("hint:"));
    }

    /// Hints can be disabled via config
    #[tokio::test]
    async fn unsupported_mode_hint_disabled() {
        use bashkit::TypeScriptConfig;
        let mut bash = Bash::builder()
            .typescript_with_config(TypeScriptConfig::default().unsupported_mode_hint(false))
            .build();
        let r = bash.exec("node --inspect app.js").await.unwrap();
        assert_ne!(r.exit_code, 0);
        assert!(
            !r.stderr.contains("hint:"),
            "should NOT show hint when disabled"
        );
    }
}

// =============================================================================
// 7. PROTOTYPE POLLUTION / OBJECT MANIPULATION
//
// Attempt to abuse JavaScript's dynamic features.
// =============================================================================

mod prototype_attacks {
    use super::*;

    /// Try __proto__ manipulation
    #[tokio::test]
    async fn no_proto_pollution() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"const obj: any = {}; obj.__proto__.polluted = true; console.log(({} as any).polluted)\"")
            .await
            .unwrap();
        // Should either fail or print undefined (not "true")
        assert!(
            !r.stdout.contains("true"),
            "__proto__ pollution should not work"
        );
    }

    /// Try constructor manipulation
    #[tokio::test]
    async fn no_constructor_abuse() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"const obj: any = {}; obj.constructor.constructor('return this')()\"")
            .await
            .unwrap();
        // Should fail — no Function constructor escape
        assert!(
            r.exit_code != 0 || !r.stdout.contains("[object"),
            "constructor abuse should not work"
        );
    }

    /// Try globalThis access
    #[tokio::test]
    async fn no_globalthis_escape() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"const keys = Object.keys(globalThis); console.log(keys.length)\"")
            .await
            .unwrap();
        // globalThis might work but should only expose safe builtins
        // The important thing is no process/require/Deno/Bun on it
        if r.exit_code == 0 {
            assert!(
                !r.stdout.contains("process") && !r.stdout.contains("require"),
                "globalThis should not expose dangerous globals"
            );
        }
    }
}

// =============================================================================
// 8. CUSTOM LIMITS TESTS
//
// Verify that custom limits are actually enforced.
// =============================================================================

mod custom_limits {
    use super::*;

    /// Very tight time limit stops long computation
    #[tokio::test]
    async fn tight_time_limit() {
        let mut bash =
            bash_ts_limits(TypeScriptLimits::default().max_duration(Duration::from_millis(100)));
        let r = bash
            .exec("ts -c \"let i = 0; while (true) { i++; }\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0);
    }

    /// Very tight stack depth limit
    #[tokio::test]
    async fn tight_stack_limit() {
        let mut bash = bash_ts_limits(TypeScriptLimits::default().max_stack_depth(10));
        let r = bash
            .exec("ts -c \"const f = (n: number): number => n <= 0 ? 0 : f(n - 1); f(100)\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0);
    }

    /// Default limits allow normal programs
    #[tokio::test]
    async fn default_limits_normal_code() {
        let mut bash = bash_ts();
        let r = bash
            .exec("ts -c \"let sum = 0; for (let i = 0; i < 100; i++) { sum += i; } console.log(sum)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "4950");
    }
}
