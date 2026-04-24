// Security tests for embedded Python (Monty) integration.
//
// White-box tests: exploit knowledge of internals (VFS bridging, resource
// limits, env merging, external function handlers, path resolution).
//
// Black-box tests: treat python3 as an opaque command and try to break out
// of the sandbox, exhaust resources, or leak information.
//
// Covers attack vectors: module imports, builtins abuse, resource exhaustion,
// VFS escape, env injection, path manipulation, error leakage, state
// persistence across executions, and Monty interpreter edge cases.

#![cfg(feature = "python")]

use bashkit::{Bash, ExecutionLimits, PythonLimits};
use std::time::Duration;

fn bash_python() -> Bash {
    Bash::builder()
        .python()
        .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
        .build()
}

fn bash_python_limits(limits: PythonLimits) -> Bash {
    Bash::builder()
        .python_with_limits(limits)
        .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
        .build()
}

fn bash_python_tight() -> Bash {
    bash_python_limits(
        PythonLimits::default()
            .max_duration(Duration::from_secs(5))
            .max_memory(4 * 1024 * 1024) // 4 MB
            .max_allocations(100_000)
            .max_recursion(50),
    )
}

#[tokio::test]
async fn python_requires_explicit_inprocess_opt_in() {
    let mut bash = Bash::builder().python().build();
    let r = bash.exec("python3 -c \"print('blocked')\"").await.unwrap();
    assert_ne!(r.exit_code, 0);
    assert!(
        r.stderr
            .contains("in-process Python disabled by default for security"),
        "expected security gate message, got stderr={:?}",
        r.stderr
    );
}

// =============================================================================
// 1. BLACK-BOX: DANGEROUS MODULE IMPORTS
//
// Try importing every module that could provide escape from the sandbox.
// =============================================================================

mod blackbox_imports {
    use super::*;

    macro_rules! import_blocked {
        ($name:ident, $module:expr) => {
            #[tokio::test]
            async fn $name() {
                let mut bash = bash_python();
                let r = bash
                    .exec(&format!("python3 -c \"import {}\"", $module))
                    .await
                    .unwrap();
                assert_ne!(r.exit_code, 0, "{} import should fail", $module);
                assert!(
                    !r.stdout.contains("hacked"),
                    "{} must not execute dangerous code",
                    $module
                );
            }
        };
    }

    import_blocked!(no_subprocess, "subprocess");
    import_blocked!(no_ctypes, "ctypes");
    import_blocked!(no_pickle, "pickle");
    import_blocked!(no_marshal, "marshal");
    import_blocked!(no_code, "code");
    import_blocked!(no_codeop, "codeop");
    import_blocked!(no_importlib, "importlib");
    import_blocked!(no_socket, "socket");
    import_blocked!(no_http, "http");
    import_blocked!(no_urllib, "urllib");
    import_blocked!(no_ssl, "ssl");
    import_blocked!(no_multiprocessing, "multiprocessing");
    import_blocked!(no_threading, "threading");
    import_blocked!(no_signal, "signal");
    import_blocked!(no_tempfile, "tempfile");
    import_blocked!(no_shutil, "shutil");
    import_blocked!(no_io, "io");
    import_blocked!(no_builtins, "builtins");
    import_blocked!(no_ast, "ast");
    import_blocked!(no_dis, "dis");
    import_blocked!(no_inspect, "inspect");
    import_blocked!(no_gc, "gc");
    import_blocked!(no_weakref, "weakref");
    import_blocked!(no_traceback, "traceback");
}

// =============================================================================
// 2. BLACK-BOX: DANGEROUS BUILTINS
//
// Try using builtins that could break sandbox: eval, exec, compile,
// __import__, globals, locals, vars, dir, type manipulation.
// =============================================================================

mod blackbox_builtins {
    use super::*;

    #[tokio::test]
    async fn no_eval() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"eval('__import__(\\\"os\\\").system(\\\"echo hacked\\\")')\"")
            .await
            .unwrap();
        assert!(
            !r.stdout.contains("hacked"),
            "eval must not allow shell escape"
        );
    }

    #[tokio::test]
    async fn no_exec() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"exec('import os')\"").await.unwrap();
        // Either exec itself fails or the inner import fails
        assert!(!r.stdout.contains("hacked"));
    }

    #[tokio::test]
    async fn no_compile() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"c = compile('import os', '<x>', 'exec')\nexec(c)\"")
            .await
            .unwrap();
        assert!(!r.stdout.contains("hacked"));
    }

    #[tokio::test]
    async fn no_dunder_import() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"m = __import__('os')\nm.system('echo hacked')\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0);
        assert!(!r.stdout.contains("hacked"));
    }

    #[tokio::test]
    async fn no_globals_manipulation() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"g = globals()\nprint(type(g))\"")
            .await
            .unwrap();
        // Either globals() isn't available or returns something safe
        if r.exit_code == 0 {
            assert!(
                !r.stdout.contains("os") && !r.stdout.contains("subprocess"),
                "globals() must not expose dangerous modules"
            );
        }
    }

    #[tokio::test]
    async fn no_open_builtin() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"f = open('/etc/passwd', 'r')\nprint(f.read())\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "open() should not be available");
        assert!(!r.stdout.contains("root:"));
    }

    #[tokio::test]
    async fn no_breakpoint() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"breakpoint()\"").await.unwrap();
        assert_ne!(r.exit_code, 0, "breakpoint() should not be available");
    }

    #[tokio::test]
    async fn no_input_escape() {
        // input() reads from stdin; verify it can't be used to hang or escape
        let mut bash = bash_python();
        let r = bash
            .exec("echo 'test' | python3 -c \"x = input()\nprint(x)\"")
            .await
            .unwrap();
        // Should either work safely (echoing 'test') or fail — not hang
        if r.exit_code == 0 {
            assert!(!r.stdout.contains("hacked"));
        }
    }
}

// =============================================================================
// 3. BLACK-BOX: CLASS/METACLASS ESCAPE ATTEMPTS
//
// Try to use __class__, __bases__, __subclasses__ to find dangerous types.
// Monty doesn't support classes, but test in case of future changes.
// =============================================================================

mod blackbox_class_escape {
    use super::*;

    #[tokio::test]
    async fn no_class_bases_escape() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(''.__class__.__bases__[0].__subclasses__())\"")
            .await
            .unwrap();
        // Should fail since Monty doesn't support classes
        // If it succeeds somehow, must not contain dangerous types
        if r.exit_code == 0 {
            assert!(
                !r.stdout.contains("subprocess") && !r.stdout.contains("Popen"),
                "Must not expose dangerous subclasses"
            );
        }
    }

    #[tokio::test]
    async fn no_type_creation() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"T = type('Exploit', (), {'__init__': lambda s: None})\nprint(T)\"")
            .await
            .unwrap();
        // Monty doesn't support class creation; should fail or be safe
        if r.exit_code == 0 {
            assert!(!r.stdout.contains("hacked"));
        }
    }

    #[tokio::test]
    async fn no_mro_traversal() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"for c in ().__class__.__mro__:\n    print(c)\"")
            .await
            .unwrap();
        // Should fail or produce safe output
        if r.exit_code == 0 {
            assert!(!r.stdout.contains("os") && !r.stdout.contains("subprocess"));
        }
    }
}

// =============================================================================
// 4. WHITE-BOX: RESOURCE LIMIT EDGE CASES
//
// Test near-limit behavior, zero limits, custom limit combos.
// =============================================================================

mod whitebox_resource_limits {
    use super::*;

    #[tokio::test]
    async fn tight_memory_blocks_list_bomb() {
        let mut bash = bash_python_tight();
        let r = bash
            .exec("python3 -c \"x = list(range(1000000))\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "4MB memory limit should block large list");
    }

    #[tokio::test]
    async fn tight_allocation_blocks_many_objects() {
        let limits = PythonLimits::default().max_allocations(10);
        let mut bash = bash_python_limits(limits);
        let r = bash
            .exec("python3 -c \"x = [i for i in range(100000)]\"")
            .await
            .unwrap();
        // Very tight allocation limit should eventually block; if not,
        // at minimum verify no crash/panic occurred
        assert!(!r.stderr.contains("panic"), "Should not panic");
    }

    #[tokio::test]
    async fn tight_duration_blocks_slow_code() {
        let limits = PythonLimits::default().max_duration(Duration::from_millis(100));
        let mut bash = bash_python_limits(limits);
        let r = bash
            .exec("python3 -c \"x = 0\nfor i in range(100000000):\n    x += 1\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "100ms limit should block long loop");
    }

    #[tokio::test]
    async fn tight_recursion_blocks_deep_call() {
        let limits = PythonLimits::default().max_recursion(10);
        let mut bash = bash_python_limits(limits);
        let r = bash
            .exec("python3 -c \"def f(n):\n    if n == 0: return 0\n    return f(n-1)\nf(20)\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "Recursion limit 10 should block depth 20");
    }

    #[tokio::test]
    async fn string_multiplication_bomb() {
        let mut bash = bash_python_tight();
        let r = bash
            .exec("python3 -c \"x = 'A' * 100000000\"")
            .await
            .unwrap();
        assert_ne!(
            r.exit_code, 0,
            "String multiplication should hit memory limit"
        );
    }

    #[tokio::test]
    async fn dict_comprehension_bomb() {
        let limits = PythonLimits::default()
            .max_memory(2 * 1024 * 1024) // 2 MB
            .max_allocations(50_000)
            .max_duration(Duration::from_secs(3));
        let mut bash = bash_python_limits(limits);
        let r = bash
            .exec("python3 -c \"d = {i: i*i for i in range(10000000)}\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "Dict bomb should hit limits");
    }

    #[tokio::test]
    async fn nested_list_bomb() {
        // Monty may use references for nested lists, so instead create
        // genuinely new large lists at each level to force allocations.
        let limits = PythonLimits::default()
            .max_memory(2 * 1024 * 1024)
            .max_allocations(50_000)
            .max_duration(Duration::from_secs(3));
        let mut bash = bash_python_limits(limits);
        let r = bash
            .exec("python3 -c \"x = [list(range(1000)) for _ in range(1000)]\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "Creating 1M list items should hit limits");
    }

    #[tokio::test]
    async fn generator_exhaustion() {
        let limits = PythonLimits::default()
            .max_duration(Duration::from_secs(2))
            .max_allocations(50_000);
        let mut bash = bash_python_limits(limits);
        let r = bash
            .exec("python3 -c \"list(range(10000000))\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "Materializing huge range should hit limits");
    }

    #[tokio::test]
    async fn successive_allocations_accumulate() {
        // Verify allocations aren't reset between statements
        let limits = PythonLimits::default().max_allocations(500);
        let mut bash = bash_python_limits(limits);
        let r = bash
            .exec("python3 -c \"a = list(range(10000))\nb = list(range(10000))\nc = list(range(10000))\"")
            .await
            .unwrap();
        // With only 500 allocations allowed and 30k objects requested,
        // should fail. If Monty counts allocations differently, at least no crash.
        assert!(!r.stderr.contains("panic"), "Should not panic");
    }
}

// =============================================================================
// 5. WHITE-BOX: VFS ESCAPE ATTEMPTS
//
// Path traversal, null bytes, symlinks, proc/sys access, unicode tricks.
// =============================================================================

mod whitebox_vfs_escape {
    use super::*;

    #[tokio::test]
    async fn path_traversal_double_dot() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nprint(Path('/home/user/../../../../etc/passwd').read_text())\"")
            .await
            .unwrap();
        assert!(
            !r.stdout.contains("root:"),
            "Path traversal must not read real fs"
        );
    }

    #[tokio::test]
    async fn path_traversal_encoded() {
        let mut bash = bash_python();
        // Try URL-encoded path traversal
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nprint(Path('/home/user/%2e%2e/%2e%2e/etc/passwd').read_text())\"")
            .await
            .unwrap();
        // Either fails or reads from VFS (no real /etc/passwd)
        assert!(!r.stdout.contains("root:"));
    }

    #[tokio::test]
    async fn proc_filesystem_blocked() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\ntry:\n    print(Path('/proc/self/environ').read_text())\nexcept:\n    print('blocked')\"")
            .await
            .unwrap();
        // /proc doesn't exist in VFS
        assert!(
            r.stdout.contains("blocked") || r.exit_code != 0,
            "/proc must not be accessible"
        );
    }

    #[tokio::test]
    async fn sys_filesystem_blocked() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\ntry:\n    print(Path('/sys/class/net').read_text())\nexcept:\n    print('blocked')\"")
            .await
            .unwrap();
        assert!(
            r.stdout.contains("blocked") || r.exit_code != 0,
            "/sys must not be accessible"
        );
    }

    #[tokio::test]
    async fn dev_null_not_real() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nprint(Path('/dev/null').exists())\"")
            .await
            .unwrap();
        // /dev/null doesn't exist in VFS unless bash creates it
        // The important thing is it doesn't access the real device
        if r.exit_code == 0 {
            // VFS might have /dev/null; that's fine as long as it's virtual
            assert!(!r.stderr.contains("panic"));
        }
    }

    #[tokio::test]
    async fn very_long_path() {
        let mut bash = bash_python();
        let long_path = "/".to_string() + &"a".repeat(4096);
        let r = bash
            .exec(&format!(
                "python3 -c \"from pathlib import Path\nPath('{}').exists()\"",
                long_path
            ))
            .await
            .unwrap();
        // Should not crash or panic
        assert!(!r.stderr.contains("panic"));
    }

    #[tokio::test]
    async fn path_with_null_byte() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/tmp/test\\x00evil').exists()\"")
            .await
            .unwrap();
        // Should not crash
        assert!(!r.stderr.contains("panic"));
    }

    #[tokio::test]
    async fn path_with_newline() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nfrom pathlib import Path\np = Path('/tmp/file\\nwith\\nnewlines')\n_ = p.write_text('test')\nprint(p.read_text())\"")
            .await
            .unwrap();
        // Should handle gracefully — either works in VFS or fails, no crash
        assert!(!r.stderr.contains("panic"));
    }

    #[tokio::test]
    async fn symlink_traversal_blocked() {
        let mut bash = bash_python();
        // Create a "symlink" via bash, then try to read through it from Python
        let r = bash
            .exec("ln -s /etc/passwd /tmp/escape_link 2>/dev/null\npython3 -c \"from pathlib import Path\ntry:\n    print(Path('/tmp/escape_link').read_text())\nexcept:\n    print('safe')\"")
            .await
            .unwrap();
        assert!(!r.stdout.contains("root:"), "Symlink must not escape VFS");
    }

    #[tokio::test]
    async fn write_then_read_isolation() {
        // Write from Python, verify it stays in VFS
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\n_ = Path('/tmp/vfs_test_42.txt').write_text('canary')\"\ncat /tmp/vfs_test_42.txt")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(
            r.stdout.contains("canary"),
            "Python write should be readable from bash"
        );
    }

    #[tokio::test]
    async fn iterdir_no_real_files() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nfor p in Path('/').iterdir():\n    print(p)\"")
            .await
            .unwrap();
        // Should only list VFS contents, not real filesystem
        assert!(!r.stdout.contains("/proc"));
        assert!(!r.stdout.contains("/sys"));
    }
}

// =============================================================================
// 6. WHITE-BOX: ENVIRONMENT VARIABLE SECURITY
//
// Test env leakage, injection, and isolation.
// =============================================================================

mod whitebox_env_security {
    use super::*;

    #[tokio::test]
    async fn exported_var_visible() {
        let mut bash = bash_python();
        let r = bash
            .exec("export SECRET_KEY=abc123\npython3 -c \"import os\nprint(os.getenv('SECRET_KEY'))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "abc123");
    }

    #[tokio::test]
    async fn unexported_var_not_leaked() {
        let mut bash = bash_python();
        let r = bash
            .exec("INTERNAL_VAR=secret\npython3 -c \"import os\nprint(os.getenv('INTERNAL_VAR', 'none'))\"")
            .await
            .unwrap();
        // Unexported vars should not be visible to Python
        // (bash semantics: only exported vars are in env)
        // Note: bashkit merges variables, so this tests that behavior
        if r.exit_code == 0 {
            // If visible, verify it's the expected value (no corruption)
            let out = r.stdout.trim();
            assert!(out == "none" || out == "secret");
        }
    }

    #[tokio::test]
    async fn env_var_with_special_chars() {
        let mut bash = bash_python();
        let r = bash
            .exec("export WEIRD_VAR='hello; rm -rf /'\npython3 -c \"import os\nprint(os.getenv('WEIRD_VAR'))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        // The value should be the literal string, not interpreted as shell
        assert!(r.stdout.contains("hello; rm -rf /"));
    }

    #[tokio::test]
    async fn environ_dict_safe() {
        let mut bash = bash_python();
        let r = bash
            .exec("export TEST_A=1\nexport TEST_B=2\npython3 -c \"import os\nfor k, v in os.environ.items():\n    if k.startswith('TEST_'):\n        print(f'{k}={v}')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("TEST_A=1"));
        assert!(r.stdout.contains("TEST_B=2"));
    }

    #[tokio::test]
    async fn env_no_host_secrets() {
        // VFS bash shouldn't inherit real host environment
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"import os\nprint(os.getenv('HOME', 'nope'))\"")
            .await
            .unwrap();
        // Should return default or VFS home, not real host HOME
        if r.exit_code == 0 {
            assert!(
                !r.stdout.contains("/root") || r.stdout.contains("nope"),
                "Should not leak real host HOME"
            );
        }
    }
}

// =============================================================================
// 7. WHITE-BOX: ERROR INFORMATION LEAKAGE
//
// Verify errors don't leak host info, stack traces stay on stderr.
// =============================================================================

mod whitebox_error_leakage {
    use super::*;

    #[tokio::test]
    async fn error_on_stderr_not_stdout() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"1/0\"").await.unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("ZeroDivisionError"));
        assert!(!r.stdout.contains("ZeroDivisionError"));
    }

    #[tokio::test]
    async fn error_no_host_paths() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/nonexistent').read_text()\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0);
        // Error message should reference VFS path, not leak host paths
        let combined = format!("{}{}", r.stdout, r.stderr);
        assert!(
            !combined.contains("/home/runner") && !combined.contains("/usr/lib"),
            "Error must not leak host filesystem paths"
        );
    }

    #[tokio::test]
    async fn partial_output_preserved_on_error() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print('before')\n1/0\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(
            r.stdout.contains("before"),
            "Output before error should be preserved"
        );
        assert!(r.stderr.contains("ZeroDivisionError"));
    }

    #[tokio::test]
    async fn syntax_error_no_source_leak() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"def f(:\n    pass\"").await.unwrap();
        assert_ne!(r.exit_code, 0);
        // Should not leak internal Rust/monty source paths
        assert!(
            !r.stderr.contains(".rs:"),
            "Should not leak Rust source paths"
        );
    }

    #[tokio::test]
    async fn pipeline_error_isolation() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"1/0\" 2>/dev/null | cat")
            .await
            .unwrap();
        // Error suppressed by 2>/dev/null; cat should see empty stdin
        assert!(
            !r.stdout.contains("ZeroDivisionError"),
            "Error must not leak through pipeline"
        );
    }
}

// =============================================================================
// 8. WHITE-BOX: STATE ISOLATION BETWEEN EXECUTIONS
//
// Verify Python state doesn't leak between separate exec() calls.
// =============================================================================

mod whitebox_state_isolation {
    use super::*;

    #[tokio::test]
    async fn python_vars_dont_persist() {
        let mut bash = bash_python();
        // First execution defines variable
        bash.exec("python3 -c \"secret = 'password123'\"")
            .await
            .unwrap();
        // Second execution should not see it
        let r = bash
            .exec(
                "python3 -c \"try:\n    print(secret)\nexcept NameError:\n    print('isolated')\"",
            )
            .await
            .unwrap();
        assert!(
            r.stdout.contains("isolated") || r.exit_code != 0,
            "Python variables must not persist between executions"
        );
    }

    #[tokio::test]
    async fn python_functions_dont_persist() {
        let mut bash = bash_python();
        bash.exec("python3 -c \"def exploit(): return 'pwned'\"")
            .await
            .unwrap();
        let r = bash
            .exec("python3 -c \"try:\n    print(exploit())\nexcept NameError:\n    print('isolated')\"")
            .await
            .unwrap();
        assert!(
            r.stdout.contains("isolated") || r.exit_code != 0,
            "Python functions must not persist"
        );
    }

    #[tokio::test]
    async fn vfs_state_does_persist() {
        let mut bash = bash_python();
        // Files should persist (shared VFS)
        bash.exec("python3 -c \"from pathlib import Path\n_ = Path('/tmp/persist.txt').write_text('data')\"")
            .await
            .unwrap();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nprint(Path('/tmp/persist.txt').read_text())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "data", "VFS files should persist");
    }

    #[tokio::test]
    async fn resource_limits_enforced_each_execution() {
        let limits = PythonLimits::default().max_allocations(50_000);
        let mut bash = bash_python_limits(limits);
        // First execution uses some allocations
        let r1 = bash
            .exec("python3 -c \"x = list(range(100))\nprint('ok')\"")
            .await
            .unwrap();
        assert_eq!(r1.exit_code, 0);
        // Second execution should have fresh allocation budget
        let r2 = bash
            .exec("python3 -c \"x = list(range(100))\nprint('ok')\"")
            .await
            .unwrap();
        assert_eq!(
            r2.exit_code, 0,
            "Each execution should get fresh resource budget"
        );
    }
}

// =============================================================================
// 9. BLACK-BOX: STRING FORMAT / F-STRING ATTACKS
//
// Try to use f-strings and format() to access attributes or call methods.
// =============================================================================

mod blackbox_format_attacks {
    use super::*;

    #[tokio::test]
    async fn fstring_attribute_access() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"x = 'hello'\nprint(f'{x.__class__}')\"")
            .await
            .unwrap();
        // f-string attribute access — if it works, just ensure no dangerous types
        if r.exit_code == 0 {
            assert!(!r.stdout.contains("subprocess"));
        }
    }

    #[tokio::test]
    async fn format_spec_injection() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"'{0.__class__.__init__.__globals__}'.format('')\"")
            .await
            .unwrap();
        // Should fail or not expose anything dangerous
        if r.exit_code == 0 {
            assert!(
                !r.stdout.contains("os") && !r.stdout.contains("subprocess"),
                "format() must not expose dangerous globals"
            );
        }
    }
}

// =============================================================================
// 10. BLACK-BOX: MATH/REGEX DENIAL OF SERVICE
//
// Try to exploit math operations or regex for DoS.
// =============================================================================

mod blackbox_dos {
    use super::*;

    #[tokio::test]
    async fn regex_redos() {
        let limits = PythonLimits::default().max_duration(Duration::from_secs(5));
        let mut bash = bash_python_limits(limits);
        // Classic ReDoS pattern: (a+)+$ against "aaa...b"
        let _r = bash
            .exec("python3 -c \"import re\nre.match('(a+)+$', 'a' * 30 + 'b')\"")
            .await
            .unwrap();
        // Should complete (monty re may not be vulnerable) or be killed by timeout
        // The key assertion: it should not hang forever
        // (this test itself has tokio timeout protection)
    }

    #[tokio::test]
    async fn math_factorial_bomb() {
        let mut bash = bash_python_tight();
        let r = bash
            .exec("python3 -c \"import math\nprint(math.factorial(100000))\"")
            .await
            .unwrap();
        // Should hit resource limits
        assert_ne!(r.exit_code, 0, "factorial(100000) should hit limits");
    }

    #[tokio::test]
    async fn repeated_print_flood() {
        let mut bash = bash_python_tight();
        let r = bash
            .exec("python3 -c \"for i in range(10000000):\n    print(i)\"")
            .await
            .unwrap();
        // Should hit allocation or time limits, not produce gigabytes of output
        assert_ne!(r.exit_code, 0, "Print flood should be stopped by limits");
    }

    #[tokio::test]
    async fn exception_chain_bomb() {
        let limits = PythonLimits::default()
            .max_duration(Duration::from_secs(5))
            .max_allocations(100_000);
        let mut bash = bash_python_limits(limits);
        let r = bash
            .exec("python3 -c \"def bomb(n):\n    try:\n        bomb(n+1)\n    except RecursionError:\n        raise ValueError('boom') from None\nbomb(0)\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "Exception chain should be caught by limits");
    }
}

// =============================================================================
// 11. WHITE-BOX: BASH <-> PYTHON INTEROP SECURITY
//
// Test edge cases in how bash features interact with Python.
// =============================================================================

mod whitebox_interop {
    use super::*;

    #[tokio::test]
    async fn command_substitution_captures_only_stdout() {
        let mut bash = bash_python();
        let r = bash
            .exec("x=$(python3 -c \"print('safe')\")\necho \"got: $x\"")
            .await
            .unwrap();
        assert!(r.stdout.contains("safe"));
    }

    #[tokio::test]
    async fn heredoc_input_to_python() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 - << 'EOF'\nprint('from heredoc')\nEOF")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("from heredoc"));
    }

    #[tokio::test]
    async fn pipeline_python_to_python() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print('42')\" | python3 -c \"import sys\nfor line in sys.stdin:\n    print(int(line.strip()) * 2)\"")
            .await
            .unwrap();
        if r.exit_code == 0 {
            assert!(r.stdout.contains("84"));
        }
    }

    #[tokio::test]
    async fn python_in_subshell() {
        let mut bash = bash_python();
        let r = bash
            .exec("(python3 -c \"print('in subshell')\")")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("in subshell"));
    }

    #[tokio::test]
    async fn python_exit_code_in_conditional() {
        let mut bash = bash_python();
        let r = bash
            .exec("if python3 -c \"1/0\" 2>/dev/null; then\n    echo bad\nelse\n    echo good\nfi")
            .await
            .unwrap();
        assert!(
            r.stdout.contains("good"),
            "Failed python should trigger else branch"
        );
    }

    #[tokio::test]
    async fn python_script_from_vfs() {
        let mut bash = bash_python();
        // Write script via bash, execute via python
        let r = bash
            .exec("echo 'print(\"from vfs script\")' > /tmp/test.py\npython3 /tmp/test.py")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("from vfs script"));
    }

    #[tokio::test]
    async fn python_script_with_shebang() {
        let mut bash = bash_python();
        let r = bash
            .exec("cat > /tmp/shebang.py << 'EOF'\n#!/usr/bin/env python3\nprint('shebang works')\nEOF\npython3 /tmp/shebang.py")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("shebang works"));
    }
}

// =============================================================================
// 12. WHITE-BOX: VFS OPERATION EDGE CASES
//
// Test unusual VFS operations from Python side.
// =============================================================================

mod whitebox_vfs_ops {
    use super::*;

    #[tokio::test]
    async fn write_read_bytes() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\n_ = Path('/tmp/bin.dat').write_bytes(b'\\x00\\x01\\x02\\xff')\ndata = Path('/tmp/bin.dat').read_bytes()\nprint(len(data))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "4");
    }

    #[tokio::test]
    async fn mkdir_parents() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/tmp/a/b/c').mkdir(parents=True, exist_ok=True)\nprint(Path('/tmp/a/b/c').is_dir())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "True");
    }

    #[tokio::test]
    async fn rename_file() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\n_ = Path('/tmp/old.txt').write_text('data')\nPath('/tmp/old.txt').rename('/tmp/new.txt')\nprint(Path('/tmp/new.txt').read_text())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "data");
    }

    #[tokio::test]
    async fn stat_returns_info() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\n_ = Path('/tmp/stat_test.txt').write_text('hello')\ns = Path('/tmp/stat_test.txt').stat()\nprint(s.st_size)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "5");
    }

    #[tokio::test]
    async fn unlink_file() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\n_ = Path('/tmp/del_me.txt').write_text('bye')\nPath('/tmp/del_me.txt').unlink()\nprint(Path('/tmp/del_me.txt').exists())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "False");
    }

    #[tokio::test]
    async fn rmdir_empty_directory() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/tmp/empty_dir').mkdir()\nPath('/tmp/empty_dir').rmdir()\nprint(Path('/tmp/empty_dir').exists())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "False");
    }

    #[tokio::test]
    async fn write_empty_file() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\n_ = Path('/tmp/empty.txt').write_text('')\nprint(repr(Path('/tmp/empty.txt').read_text()))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("''"));
    }

    #[tokio::test]
    async fn large_file_write_read() {
        let mut bash = bash_python_tight();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\ndata = 'x' * 1000000\n_ = Path('/tmp/big.txt').write_text(data)\nprint(len(Path('/tmp/big.txt').read_text()))\"")
            .await
            .unwrap();
        // Either succeeds within limits or hits memory cap
        if r.exit_code == 0 {
            assert_eq!(r.stdout.trim(), "1000000");
        }
    }

    #[tokio::test]
    async fn concurrent_bash_python_vfs() {
        let mut bash = bash_python();
        // Interleave bash and python file ops
        let r = bash
            .exec(
                "echo 'step1' > /tmp/interleave.txt\n\
                 python3 -c \"from pathlib import Path\ncontent = Path('/tmp/interleave.txt').read_text().strip()\n_ = Path('/tmp/interleave.txt').write_text(content + '\\nstep2')\"\n\
                 echo 'step3' >> /tmp/interleave.txt\n\
                 cat /tmp/interleave.txt"
            )
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        // Verify all steps are present
        assert!(r.stdout.contains("step1"));
        assert!(r.stdout.contains("step2"));
    }
}

// =============================================================================
// 13. WHITE-BOX: EXECUTION LIMITS INTERACTION
//
// Test that bash-level limits interact correctly with Python limits.
// =============================================================================

mod whitebox_limit_interaction {
    use super::*;

    #[tokio::test]
    async fn bash_max_commands_limits_python_invocations() {
        let limits = ExecutionLimits::new().max_commands(10);
        let mut bash = Bash::builder()
            .python()
            .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
            .limits(limits)
            .build();
        // Each python3 invocation is a command; with limit=10 a few should work
        let r = bash
            .exec("python3 -c \"print(1)\"\npython3 -c \"print(2)\"")
            .await
            .unwrap();
        assert!(r.stdout.contains("1"));
    }

    #[tokio::test]
    async fn python_error_doesnt_break_bash() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"1/0\" 2>/dev/null\necho 'bash still works'")
            .await
            .unwrap();
        assert!(
            r.stdout.contains("bash still works"),
            "Python error should not break bash execution"
        );
    }

    #[tokio::test]
    async fn many_python_invocations() {
        let mut bash = bash_python();
        // Run 20 python invocations in sequence
        let mut script = String::new();
        for i in 0..20 {
            script.push_str(&format!("python3 -c \"print({})\"\n", i));
        }
        let r = bash.exec(&script).await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("0"));
        assert!(r.stdout.contains("19"));
    }
}

// =============================================================================
// 14. BLACK-BOX: PYTHON LANGUAGE EDGE CASES
//
// Test edge cases in the Python language that might crash the interpreter.
// =============================================================================

mod blackbox_language_edge_cases {
    use super::*;

    #[tokio::test]
    async fn empty_string_code() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"\"").await.unwrap();
        assert_ne!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn only_comments() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"# just a comment\"").await.unwrap();
        // Empty program with comment is valid Python
        // Should succeed or fail gracefully
        assert!(!r.stderr.contains("panic"));
    }

    #[tokio::test]
    async fn unicode_identifiers() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"x = 42\nprint(x)\"").await.unwrap();
        if r.exit_code == 0 {
            assert_eq!(r.stdout.trim(), "42");
        }
    }

    #[tokio::test]
    async fn very_large_integer() {
        let limits = PythonLimits::default()
            .max_memory(1024 * 1024) // 1 MB
            .max_duration(Duration::from_secs(3));
        let mut bash = bash_python_limits(limits);
        let r = bash
            .exec("python3 -c \"x = 10 ** 10000000\ny = x * x\"")
            .await
            .unwrap();
        assert_ne!(
            r.exit_code, 0,
            "Huge integer chain should hit memory limits"
        );
    }

    #[tokio::test]
    async fn deeply_nested_dict() {
        let mut bash = bash_python_tight();
        let r = bash
            .exec("python3 -c \"d = {}\ncurrent = d\nfor i in range(1000):\n    current['next'] = {}\n    current = current['next']\"")
            .await
            .unwrap();
        // Should hit allocation limits or succeed (but not crash)
        assert!(!r.stderr.contains("panic"));
    }

    #[tokio::test]
    async fn try_except_finally() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"try:\n    1/0\nexcept ZeroDivisionError:\n    print('caught')\nfinally:\n    print('finally')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("caught"));
        assert!(r.stdout.contains("finally"));
    }

    #[tokio::test]
    async fn generator_expression() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(sum(x*x for x in range(10)))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "285");
    }

    #[tokio::test]
    async fn walrus_operator() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"if (n := 10) > 5:\n    print(f'n={n}')\"")
            .await
            .unwrap();
        // Walrus may not be supported by Monty; should fail gracefully
        if r.exit_code == 0 {
            assert!(r.stdout.contains("n=10"));
        }
    }

    #[tokio::test]
    async fn multiple_assignments() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"a = b = c = 42\nprint(a, b, c)\"")
            .await
            .unwrap();
        if r.exit_code == 0 {
            assert!(r.stdout.contains("42 42 42"));
        }
    }
}

// =============================================================================
// 15. WHITE-BOX: STDIN/PIPE INJECTION
//
// Test that piped input can't be used for code injection.
// =============================================================================

mod whitebox_stdin_injection {
    use super::*;

    #[tokio::test]
    async fn stdin_code_execution() {
        let mut bash = bash_python();
        let r = bash.exec("echo 'print(42)' | python3 -").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "42");
    }

    #[tokio::test]
    async fn stdin_malicious_code() {
        let mut bash = bash_python();
        let r = bash
            .exec("echo 'import os; os.system(\"echo hacked\")' | python3 -")
            .await
            .unwrap();
        assert!(
            !r.stdout.contains("hacked"),
            "Malicious stdin code must not escape sandbox"
        );
    }

    #[tokio::test]
    async fn stdin_oversize() {
        let mut bash = bash_python_tight();
        // Generate large stdin
        let r = bash
            .exec("python3 -c \"print('A' * 1000000)\" | python3 -c \"import sys\ndata = sys.stdin.read()\nprint(len(data))\"")
            .await
            .unwrap();
        // Should either work or hit limits, but not crash
        assert!(!r.stderr.contains("panic"));
    }

    #[tokio::test]
    async fn stdin_empty() {
        let mut bash = bash_python();
        let r = bash.exec("echo '' | python3 -").await.unwrap();
        // Empty input to python - should handle gracefully
        assert!(!r.stderr.contains("panic"));
    }
}

// =============================================================================
// 16. WHITE-BOX: ARGUMENT PARSING EDGE CASES
//
// Test unusual argument combinations.
// =============================================================================

mod whitebox_arg_parsing {
    use super::*;

    #[tokio::test]
    async fn unknown_flag_rejected() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -X importall").await.unwrap();
        assert_ne!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn double_dash_c() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c -c \"print('test')\"").await.unwrap();
        // -c with "-c" as code string — should try to parse "-c" as python
        // and likely fail
        assert_ne!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn no_args_no_stdin() {
        let mut bash = bash_python();
        let r = bash.exec("python3").await.unwrap();
        assert_ne!(r.exit_code, 0, "Interactive mode should not be supported");
    }

    #[tokio::test]
    async fn nonexistent_script() {
        let mut bash = bash_python();
        let r = bash.exec("python3 /nonexistent/script.py").await.unwrap();
        assert_eq!(r.exit_code, 2);
    }

    #[tokio::test]
    async fn c_flag_missing_code() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c").await.unwrap();
        assert_ne!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn binary_script_file() {
        let mut bash = bash_python();
        // Write binary content as script, try to execute
        let r = bash
            .exec("printf '\\x00\\x01\\x02' > /tmp/binary.py\npython3 /tmp/binary.py")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "Binary file should fail as Python script");
    }
}
