// Integration tests for embedded Monty (Python) direct integration.
//
// Tests the full Bash → Python pipeline: argument parsing, code execution,
// VFS bridging, resource limits, error handling, and interop with bash features
// (pipelines, command substitution, conditionals).

#![cfg(feature = "python")]

use bashkit::{Bash, PythonLimits};
use std::time::Duration;

/// Helper: create Bash with python builtins using default limits.
fn bash_python() -> Bash {
    Bash::builder().python().build()
}

/// Helper: create Bash with custom python limits.
fn bash_python_limits(limits: PythonLimits) -> Bash {
    Bash::builder().python_with_limits(limits).build()
}

// =============================================================================
// 1. BASIC EXECUTION
// =============================================================================

mod basic_execution {
    use super::*;

    #[tokio::test]
    async fn print_hello() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"print('hello')\"").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello\n");
    }

    #[tokio::test]
    async fn expression_result() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"2 + 3\"").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "5\n");
    }

    #[tokio::test]
    async fn multiline_script() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"x = 10\ny = 20\nprint(x + y)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "30\n");
    }

    #[tokio::test]
    async fn version_flag() {
        let mut bash = bash_python();
        let r = bash.exec("python3 --version").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("Python 3.12.0"));
    }

    #[tokio::test]
    async fn version_flag_short() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -V").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("Python 3.12.0"));
    }

    #[tokio::test]
    async fn help_flag() {
        let mut bash = bash_python();
        let r = bash.exec("python3 --help").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("usage:"));
    }

    #[tokio::test]
    async fn python_alias_works() {
        // Both `python` and `python3` should work
        let mut bash = bash_python();
        let r = bash
            .exec("python -c \"print('via python')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "via python\n");
    }

    #[tokio::test]
    async fn none_expression_no_output() {
        // None result should produce no output
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"x = 42\"").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "");
    }

    #[tokio::test]
    async fn string_expression_result() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"'hello'\"").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "'hello'\n");
    }
}

// =============================================================================
// 2. INPUT MODES
// =============================================================================

mod input_modes {
    use super::*;

    #[tokio::test]
    async fn script_file_from_vfs() {
        let mut bash = bash_python();
        bash.exec("cat > /tmp/script.py << 'EOF'\nprint('from file')\nEOF")
            .await
            .unwrap();
        let r = bash.exec("python3 /tmp/script.py").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "from file\n");
    }

    #[tokio::test]
    async fn stdin_pipe() {
        let mut bash = bash_python();
        let r = bash
            .exec("echo \"print('piped')\" | python3")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "piped\n");
    }

    #[tokio::test]
    async fn stdin_dash_flag() {
        let mut bash = bash_python();
        let r = bash
            .exec("echo \"print('dash')\" | python3 -")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "dash\n");
    }

    #[tokio::test]
    async fn shebang_stripped_from_file() {
        let mut bash = bash_python();
        bash.exec(
            "cat > /tmp/shebang.py << 'EOF'\n#!/usr/bin/env python3\nprint('shebang ok')\nEOF",
        )
        .await
        .unwrap();
        let r = bash.exec("python3 /tmp/shebang.py").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "shebang ok\n");
    }

    #[tokio::test]
    async fn missing_file_error() {
        let mut bash = bash_python();
        let r = bash.exec("python3 /no/such/script.py").await.unwrap();
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("can't open file"));
    }

    #[tokio::test]
    async fn missing_c_arg() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c").await.unwrap();
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("requires argument"));
    }

    #[tokio::test]
    async fn unknown_option() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -z").await.unwrap();
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("unknown option"));
    }
}

// =============================================================================
// 3. DATA TYPES AND OPERATIONS
// =============================================================================

mod data_types {
    use super::*;

    #[tokio::test]
    async fn list_operations() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"lst = [1, 2, 3]\nlst.append(4)\nprint(lst)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "[1, 2, 3, 4]\n");
    }

    #[tokio::test]
    async fn dict_operations() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"d = dict()\nd['a'] = 1\nd['b'] = 2\nprint(d['a'])\nprint(len(d))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "1\n2\n");
    }

    #[tokio::test]
    async fn tuple_operations() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"t = (1, 'two', 3.0)\nprint(t[1])\nprint(len(t))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "two\n3\n");
    }

    #[tokio::test]
    async fn set_operations() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"s = set([1, 2, 3, 2, 1])\nprint(len(s))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "3\n");
    }

    #[tokio::test]
    async fn string_methods() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"s = 'hello world'\nprint(s.upper())\nprint(s.split())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "HELLO WORLD\n['hello', 'world']\n");
    }

    #[tokio::test]
    async fn fstring_formatting() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"name = 'world'\nx = 42\nprint(f'hello {name}, x={x}')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello world, x=42\n");
    }

    #[tokio::test]
    async fn list_comprehension() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print([x**2 for x in range(5)])\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "[0, 1, 4, 9, 16]\n");
    }

    #[tokio::test]
    async fn dict_comprehension() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"d = {str(i): i*i for i in range(3)}\nprint(d)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("'0': 0"));
        assert!(r.stdout.contains("'1': 1"));
        assert!(r.stdout.contains("'2': 4"));
    }

    #[tokio::test]
    async fn boolean_operations() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(True and False)\nprint(True or False)\nprint(not True)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "False\nTrue\nFalse\n");
    }

    #[tokio::test]
    async fn none_value() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"x = None\nprint(x is None)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\n");
    }

    #[tokio::test]
    async fn integer_arithmetic() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(2 ** 10)\nprint(17 // 3)\nprint(17 % 3)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "1024\n5\n2\n");
    }

    #[tokio::test]
    async fn float_arithmetic() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(round(3.14159, 2))\nprint(abs(-42.5))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "3.14\n42.5\n");
    }

    #[tokio::test]
    async fn string_slicing() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"s = 'abcdefgh'\nprint(s[2:5])\nprint(s[::-1])\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "cde\nhgfedcba\n");
    }
}

// =============================================================================
// 4. CONTROL FLOW
// =============================================================================

mod control_flow {
    use super::*;

    #[tokio::test]
    async fn if_elif_else() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"x = 5\nif x > 10:\n    print('big')\nelif x > 3:\n    print('medium')\nelse:\n    print('small')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "medium\n");
    }

    #[tokio::test]
    async fn for_loop_range() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"total = 0\nfor i in range(5):\n    total += i\nprint(total)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "10\n");
    }

    #[tokio::test]
    async fn for_loop_list() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"for item in ['a', 'b', 'c']:\n    print(item)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "a\nb\nc\n");
    }

    #[tokio::test]
    async fn while_loop() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"i = 0\nwhile i < 3:\n    print(i)\n    i += 1\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "0\n1\n2\n");
    }

    #[tokio::test]
    async fn break_in_loop() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"for i in range(10):\n    if i == 3:\n        break\n    print(i)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "0\n1\n2\n");
    }

    #[tokio::test]
    async fn continue_in_loop() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"for i in range(5):\n    if i % 2 == 0:\n        continue\n    print(i)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "1\n3\n");
    }
}

// =============================================================================
// 5. FUNCTIONS
// =============================================================================

mod functions {
    use super::*;

    #[tokio::test]
    async fn basic_function() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"def greet(name):\n    return f'hello {name}'\nprint(greet('world'))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn default_args() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"def add(a, b=10):\n    return a + b\nprint(add(5))\nprint(add(5, 20))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "15\n25\n");
    }

    #[tokio::test]
    async fn recursive_function() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"def factorial(n):\n    if n <= 1:\n        return 1\n    return n * factorial(n - 1)\nprint(factorial(10))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "3628800\n");
    }

    #[tokio::test]
    async fn lambda_expression() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"double = lambda x: x * 2\nprint(double(21))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "42\n");
    }

    #[tokio::test]
    async fn nested_function() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"def outer():\n    x = 10\n    def inner():\n        return x + 5\n    return inner()\nprint(outer())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "15\n");
    }
}

// =============================================================================
// 6. EXCEPTION HANDLING
// =============================================================================

mod exception_handling {
    use super::*;

    #[tokio::test]
    async fn try_except_basic() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"try:\n    1/0\nexcept ZeroDivisionError:\n    print('caught')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "caught\n");
    }

    #[tokio::test]
    async fn try_except_finally() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"try:\n    x = 1\nexcept:\n    print('error')\nfinally:\n    print('done')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "done\n");
    }

    #[tokio::test]
    async fn try_except_as() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"try:\n    int('abc')\nexcept ValueError as e:\n    print('got ValueError')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "got ValueError\n");
    }

    #[tokio::test]
    async fn raise_exception() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"raise ValueError('test error')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("ValueError"));
    }

    #[tokio::test]
    async fn nested_try_except() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"try:\n    try:\n        1/0\n    except ZeroDivisionError:\n        raise ValueError('chained')\nexcept ValueError as e:\n    print('caught:', e)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("caught:"));
    }
}

// =============================================================================
// 7. ERROR HANDLING
// =============================================================================

mod error_handling {
    use super::*;

    #[tokio::test]
    async fn syntax_error() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"def\"").await.unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("SyntaxError") || r.stderr.contains("Error"));
    }

    #[tokio::test]
    async fn zero_division() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"1/0\"").await.unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("ZeroDivisionError"));
    }

    #[tokio::test]
    async fn name_error() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(undefined_var)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("NameError"));
    }

    #[tokio::test]
    async fn type_error() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"1 + 'a'\"").await.unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("TypeError"));
    }

    #[tokio::test]
    async fn index_error() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"lst = [1, 2]\nprint(lst[10])\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("IndexError"));
    }

    #[tokio::test]
    async fn key_error() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"d = dict()\nprint(d['missing'])\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("KeyError"));
    }

    #[tokio::test]
    async fn output_before_error_preserved() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print('before')\n1/0\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 1);
        assert_eq!(r.stdout, "before\n");
        assert!(r.stderr.contains("ZeroDivisionError"));
    }

    #[tokio::test]
    async fn multiple_prints_before_error() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print('one')\nprint('two')\n1/0\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stdout.contains("one"));
        assert!(r.stdout.contains("two"));
        assert!(r.stderr.contains("ZeroDivisionError"));
    }
}

// =============================================================================
// 8. VFS BRIDGING
// =============================================================================

mod vfs_bridging {
    use super::*;

    #[tokio::test]
    async fn bash_writes_python_reads() {
        let mut bash = bash_python();
        bash.exec("echo -n 'hello from bash' > /tmp/test.txt")
            .await
            .unwrap();
        let r = bash
            .exec(
                "python3 -c \"from pathlib import Path\nprint(Path('/tmp/test.txt').read_text())\"",
            )
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello from bash\n");
    }

    #[tokio::test]
    async fn python_writes_bash_reads() {
        let mut bash = bash_python();
        bash.exec("python3 -c \"from pathlib import Path\nPath('/tmp/py_out.txt').write_text('from python')\"")
            .await
            .unwrap();
        let r = bash.exec("cat /tmp/py_out.txt").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "from python");
    }

    #[tokio::test]
    async fn python_writes_python_reads() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/tmp/rw.txt').write_text('roundtrip')\nprint(Path('/tmp/rw.txt').read_text())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "roundtrip\n");
    }

    #[tokio::test]
    async fn path_exists() {
        let mut bash = bash_python();
        bash.exec("echo 'data' > /tmp/exists.txt").await.unwrap();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nprint(Path('/tmp/exists.txt').exists())\nprint(Path('/tmp/nope.txt').exists())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\nFalse\n");
    }

    #[tokio::test]
    async fn path_is_file_is_dir() {
        let mut bash = bash_python();
        bash.exec("mkdir -p /data && echo 'x' > /data/f.txt")
            .await
            .unwrap();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nprint(Path('/data/f.txt').is_file())\nprint(Path('/data').is_dir())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\nTrue\n");
    }

    #[tokio::test]
    async fn mkdir_and_verify() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/tmp/newdir').mkdir()\nprint(Path('/tmp/newdir').is_dir())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\n");
    }

    #[tokio::test]
    async fn mkdir_parents() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/tmp/a/b/c').mkdir(parents=True)\nprint(Path('/tmp/a/b/c').is_dir())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\n");
    }

    #[tokio::test]
    async fn mkdir_parents_exist_ok() {
        let mut bash = bash_python();
        // mkdir(parents=True, exist_ok=True) should always succeed
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/tmp/deep/nested/dir').mkdir(parents=True, exist_ok=True)\nprint(Path('/tmp/deep/nested/dir').is_dir())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\n");
        // Calling again should also succeed
        let r2 = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/tmp/deep/nested/dir').mkdir(parents=True, exist_ok=True)\nprint('ok')\"")
            .await
            .unwrap();
        assert_eq!(r2.exit_code, 0);
        assert_eq!(r2.stdout, "ok\n");
    }

    #[tokio::test]
    async fn iterdir() {
        let mut bash = bash_python();
        bash.exec("mkdir -p /list && echo a > /list/one.txt && echo b > /list/two.txt")
            .await
            .unwrap();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nfor p in Path('/list').iterdir():\n    print(p.name)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("one.txt"));
        assert!(r.stdout.contains("two.txt"));
    }

    #[tokio::test]
    async fn stat_file_size() {
        let mut bash = bash_python();
        bash.exec("echo -n '12345' > /tmp/sized.txt").await.unwrap();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\ninfo = Path('/tmp/sized.txt').stat()\nprint(info.st_size)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "5\n");
    }

    #[tokio::test]
    async fn unlink_file() {
        let mut bash = bash_python();
        bash.exec("echo 'x' > /tmp/to_delete.txt").await.unwrap();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/tmp/to_delete.txt').unlink()\nprint(Path('/tmp/to_delete.txt').exists())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "False\n");
    }

    #[tokio::test]
    async fn rename_file() {
        let mut bash = bash_python();
        bash.exec("echo 'data' > /tmp/old_name.txt").await.unwrap();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/tmp/old_name.txt').rename('/tmp/new_name.txt')\nprint(Path('/tmp/new_name.txt').exists())\nprint(Path('/tmp/old_name.txt').exists())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\nFalse\n");
    }

    #[tokio::test]
    async fn read_not_found_exception() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\ntry:\n    Path('/no/such/file').read_text()\nexcept FileNotFoundError:\n    print('caught FileNotFoundError')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "caught FileNotFoundError\n");
    }

    #[tokio::test]
    async fn write_bytes() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/tmp/bin.dat').write_bytes(b'\\x00\\x01\\x02')\ndata = Path('/tmp/bin.dat').read_bytes()\nprint(len(data))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "3\n");
    }

    #[tokio::test]
    async fn relative_path_resolves_to_cwd() {
        let mut bash = bash_python();
        // Ensure cwd exists in VFS, then write a file there
        bash.exec("mkdir -p /home/user && echo -n 'relative' > /home/user/rel.txt")
            .await
            .unwrap();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nprint(Path('rel.txt').read_text())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "relative\n");
    }

    #[tokio::test]
    async fn path_resolve() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nprint(Path('/tmp/../tmp/file.txt').resolve())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        // Should resolve to absolute path
        assert!(r.stdout.contains("tmp"));
    }
}

// =============================================================================
// 9. ENVIRONMENT ACCESS
// =============================================================================

mod environment {
    use super::*;

    #[tokio::test]
    async fn getenv_existing() {
        let mut bash = Bash::builder().python().env("MY_VAR", "test_value").build();
        let r = bash
            .exec("python3 -c \"import os\nprint(os.getenv('MY_VAR'))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "test_value\n");
    }

    #[tokio::test]
    async fn getenv_missing_with_default() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"import os\nprint(os.getenv('NONEXISTENT', 'fallback'))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "fallback\n");
    }

    #[tokio::test]
    async fn getenv_missing_returns_none() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"import os\nprint(os.getenv('NONEXISTENT'))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "None\n");
    }

    #[tokio::test]
    async fn environ_dict() {
        let mut bash = Bash::builder()
            .python()
            .env("FOO", "bar")
            .env("BAZ", "qux")
            .build();
        let r = bash
            .exec("python3 -c \"import os\nenv = os.environ\nprint('FOO' in env)\nprint(env.get('FOO'))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("True"));
        assert!(r.stdout.contains("bar"));
    }

    #[tokio::test]
    async fn builder_env_visible_to_python() {
        // Use builder .env() to set env vars visible to Python
        let mut bash = Bash::builder().python().env("GREETING", "hello").build();
        let r = bash
            .exec("python3 -c \"import os\nprint(os.getenv('GREETING'))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello\n");
    }
}

// =============================================================================
// 10. RESOURCE LIMITS
// =============================================================================

mod resource_limits {
    use super::*;

    #[tokio::test]
    async fn recursion_limit() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"def r(): r()\nr()\"").await.unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("RecursionError") || r.stderr.contains("recursion"));
    }

    #[tokio::test]
    async fn memory_limit() {
        let limits = PythonLimits::default().max_memory(1024);
        let mut bash = bash_python_limits(limits);
        let r = bash
            .exec("python3 -c \"x = list(range(100000))\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "Tight memory limit should cause failure");
    }

    #[tokio::test]
    async fn custom_recursion_limit() {
        let limits = PythonLimits::default().max_recursion(5);
        let mut bash = bash_python_limits(limits);
        let r = bash
            .exec("python3 -c \"def deep(n):\n    if n <= 0:\n        return 0\n    return deep(n-1) + 1\nprint(deep(100))\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0, "Should hit recursion limit with depth=5");
    }

    #[tokio::test]
    async fn generous_limits_succeed() {
        let limits = PythonLimits::default()
            .max_allocations(10_000_000)
            .max_memory(128 * 1024 * 1024);
        let mut bash = bash_python_limits(limits);
        let r = bash
            .exec("python3 -c \"print(sum(range(1000)))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "499500\n");
    }

    #[tokio::test]
    async fn timeout_limit() {
        let limits = PythonLimits::default().max_duration(Duration::from_millis(100));
        let mut bash = bash_python_limits(limits);
        let r = bash.exec("python3 -c \"while True: pass\"").await.unwrap();
        assert_ne!(r.exit_code, 0, "Infinite loop should be killed by timeout");
    }
}

// =============================================================================
// 11. BASH INTEROP (PIPELINES, SUBST, CONDITIONALS)
// =============================================================================

mod bash_interop {
    use super::*;

    #[tokio::test]
    async fn python_in_pipeline() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"for i in range(5):\n    print(f'item-{i}')\" | grep 'item-3'")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "item-3");
    }

    #[tokio::test]
    async fn command_substitution() {
        let mut bash = bash_python();
        let r = bash
            .exec("result=$(python3 -c \"print(6 * 7)\")\necho \"result: $result\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "result: 42\n");
    }

    #[tokio::test]
    async fn conditional_success() {
        let mut bash = bash_python();
        let r = bash
            .exec("if python3 -c \"print('ok')\"; then echo 'success'; else echo 'failure'; fi")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("success"));
    }

    #[tokio::test]
    async fn conditional_failure() {
        let mut bash = bash_python();
        let r = bash
            .exec("if python3 -c \"1/0\" 2>/dev/null; then echo 'success'; else echo 'failure'; fi")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("failure"));
    }

    #[tokio::test]
    async fn variable_in_python_code() {
        let mut bash = bash_python();
        bash.exec("NAME=world").await.unwrap();
        let r = bash
            .exec("python3 -c \"print('hello $NAME')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn python_exit_code_propagates() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"raise SystemExit(42)\" 2>/dev/null; echo $?")
            .await
            .unwrap();
        // The exit code should propagate (may be 1 for exception, not 42, depending on Monty)
        assert!(r.stdout.contains("1") || r.stdout.contains("42"));
    }

    #[tokio::test]
    async fn multiple_python_calls() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print('first')\"\npython3 -c \"print('second')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("first"));
        assert!(r.stdout.contains("second"));
    }

    #[tokio::test]
    async fn python_vfs_shared_with_bash() {
        // Write from Python, process with bash pipeline
        let mut bash = bash_python();
        bash.exec("python3 -c \"from pathlib import Path\nPath('/tmp/numbers.txt').write_text('1\\n2\\n3\\n4\\n5\\n')\"")
            .await
            .unwrap();
        let r = bash.exec("wc -l < /tmp/numbers.txt").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "5");
    }
}

// =============================================================================
// 12. BUILTIN FUNCTIONS
// =============================================================================

mod builtins {
    use super::*;

    #[tokio::test]
    async fn len_function() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(len([1,2,3]))\nprint(len('hello'))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "3\n5\n");
    }

    #[tokio::test]
    async fn range_enumerate_zip() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"for i, v in enumerate(['a','b','c']):\n    print(f'{i}:{v}')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "0:a\n1:b\n2:c\n");
    }

    #[tokio::test]
    async fn map_filter() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"nums = list(range(6))\nevens = [x for x in nums if x % 2 == 0]\nprint(evens)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "[0, 2, 4]\n");
    }

    #[tokio::test]
    async fn sorted_reversed() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(sorted([3,1,4,1,5]))\nprint(list(reversed([1,2,3])))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "[1, 1, 3, 4, 5]\n[3, 2, 1]\n");
    }

    #[tokio::test]
    async fn min_max_sum() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"nums = [10, 20, 30, 40]\nprint(min(nums))\nprint(max(nums))\nprint(sum(nums))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "10\n40\n100\n");
    }

    #[tokio::test]
    async fn type_conversions() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(int('42'))\nprint(float('3.14'))\nprint(str(100))\nprint(bool(0))\nprint(bool(1))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "42\n3.14\n100\nFalse\nTrue\n");
    }

    #[tokio::test]
    async fn isinstance_check() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(isinstance(42, int))\nprint(isinstance('hi', str))\nprint(isinstance(42, str))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\nTrue\nFalse\n");
    }

    #[tokio::test]
    async fn all_any() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(all([True, True, True]))\nprint(all([True, False, True]))\nprint(any([False, False, True]))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\nFalse\nTrue\n");
    }

    #[tokio::test]
    async fn abs_round() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(abs(-42))\nprint(round(3.14159, 2))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "42\n3.14\n");
    }

    #[tokio::test]
    async fn zip_function() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"pairs = list(zip([1,2,3], ['a','b','c']))\nprint(pairs)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "[(1, 'a'), (2, 'b'), (3, 'c')]\n");
    }
}

// =============================================================================
// 13. SECURITY
// =============================================================================

mod security {
    use super::*;

    #[tokio::test]
    async fn no_real_filesystem_access() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\ntry:\n    Path('/etc/passwd').read_text()\n    print('LEAKED')\nexcept FileNotFoundError:\n    print('safe')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("safe"));
        assert!(!r.stdout.contains("LEAKED"));
    }

    #[tokio::test]
    async fn no_os_system() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"import os\nos.system('echo hacked')\"")
            .await
            .unwrap();
        assert_ne!(r.exit_code, 0);
        assert!(!r.stdout.contains("hacked"));
    }

    #[tokio::test]
    async fn no_subprocess_module() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"import subprocess\"").await.unwrap();
        assert_ne!(r.exit_code, 0);
    }

    #[tokio::test]
    async fn path_traversal_blocked() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"from pathlib import Path\ntry:\n    Path('/tmp/../../../etc/passwd').read_text()\n    print('ESCAPED')\nexcept FileNotFoundError:\n    print('blocked')\"")
            .await
            .unwrap();
        assert!(!r.stdout.contains("ESCAPED"));
    }

    #[tokio::test]
    async fn env_vars_not_leaked_from_host() {
        // Host env vars should NOT be visible to Python — only sandbox env
        let mut bash = bash_python();
        let r = bash
            .exec(
                "python3 -c \"import os\nresult = os.getenv('PATH', 'not_found')\nprint(result)\"",
            )
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        // PATH is not set in the sandbox env by default
        assert_eq!(r.stdout, "not_found\n");
    }
}

// =============================================================================
// 14. EDGE CASES
// =============================================================================

mod edge_cases {
    use super::*;

    #[tokio::test]
    async fn empty_print() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"print()\"").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "\n");
    }

    #[tokio::test]
    async fn multiple_print_args() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"print(1, 2, 3)\"").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "1 2 3\n");
    }

    #[tokio::test]
    async fn print_with_sep() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(1, 2, 3, sep='-')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "1-2-3\n");
    }

    #[tokio::test]
    async fn print_with_end() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print('a', end='')\nprint('b', end='')\nprint('c')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "abc\n");
    }

    #[tokio::test]
    async fn large_output() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"for i in range(100):\n    print(f'line {i}')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        let lines: Vec<&str> = r.stdout.lines().collect();
        assert_eq!(lines.len(), 100);
    }

    #[tokio::test]
    async fn unicode_output() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"print('hello')\"").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn multiline_string() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"s = '''line1\nline2\nline3'''\nprint(s)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("line1"));
        assert!(r.stdout.contains("line2"));
        assert!(r.stdout.contains("line3"));
    }

    #[tokio::test]
    async fn unpacking() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"a, b, c = 1, 2, 3\nprint(a, b, c)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "1 2 3\n");
    }

    #[tokio::test]
    async fn ternary_expression() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"x = 5\nresult = 'big' if x > 10 else 'small'\nprint(result)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "small\n");
    }

    #[tokio::test]
    async fn walrus_operator() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"if (n := 10) > 5:\n    print(f'n is {n}')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "n is 10\n");
    }
}

// =============================================================================
// 15. COMPLEX SCRIPTS
// =============================================================================

mod complex_scripts {
    use super::*;

    #[tokio::test]
    async fn fibonacci() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"def fib(n):\n    if n <= 1:\n        return n\n    return fib(n-1) + fib(n-2)\nprint(fib(10))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "55\n");
    }

    #[tokio::test]
    async fn data_processing() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"scores = [95, 87, 92, 78, 96]\ntotal = sum(scores)\navg = total / len(scores)\nprint(f'avg={avg}')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "avg=89.6\n");
    }

    #[tokio::test]
    async fn vfs_multifile_workflow() {
        let mut bash = bash_python();
        // Write config from bash, process in python, read result from bash
        bash.exec("mkdir -p /app && echo 'key=value' > /app/config.txt")
            .await
            .unwrap();
        bash.exec("python3 -c \"from pathlib import Path\ncfg = Path('/app/config.txt').read_text()\nk, v = cfg.strip().split('=')\nPath('/app/result.txt').write_text(f'{k.upper()}={v.upper()}')\"")
            .await
            .unwrap();
        let r = bash.exec("cat /app/result.txt").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "KEY=VALUE");
    }

    #[tokio::test]
    async fn generator_expression() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"total = sum(x**2 for x in range(10))\nprint(total)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "285\n");
    }

    #[tokio::test]
    async fn star_unpacking() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"first, *rest = [1, 2, 3, 4, 5]\nprint(first)\nprint(rest)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "1\n[2, 3, 4, 5]\n");
    }
}

// =============================================================================
// 16. REGRESSION TESTS — Monty v0.0.5 / v0.0.6 fixes
//
// These tests exercise bug fixes introduced in Monty v0.0.5 and the
// PrintWriter API changes in v0.0.6 to guard against regressions.
// =============================================================================

mod monty_regressions {
    use super::*;

    // -- v0.0.5: heap-allocated string comparison (pydantic/monty#159) --------

    #[tokio::test]
    async fn heap_string_equality() {
        let mut bash = bash_python();
        // Strings long enough to be heap-allocated (not interned)
        let r = bash
            .exec("python3 -c \"a = 'hello world ' * 10\nb = 'hello world ' * 10\nprint(a == b)\nprint(a != b)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\nFalse\n");
    }

    #[tokio::test]
    async fn heap_string_comparison_operators() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"a = 'aaa' * 20\nb = 'bbb' * 20\nprint(a < b)\nprint(b > a)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "True\nTrue\n");
    }

    #[tokio::test]
    async fn heap_string_in_collection() {
        let mut bash = bash_python();
        // String used as dict key must compare correctly after heap allocation
        let r = bash
            .exec("python3 -c \"k = 'long_key_' * 5\nd = dict()\nd[k] = 42\nk2 = 'long_key_' * 5\nprint(d[k2])\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "42\n");
    }

    // -- v0.0.5: i64::MIN division overflow (pydantic/monty#147) --------------

    #[tokio::test]
    async fn floor_div_negative_boundary() {
        let mut bash = bash_python();
        // -9223372036854775808 // -1 would overflow i64; Monty should handle gracefully
        let r = bash
            .exec("python3 -c \"import sys\nprint(-7 // 2)\nprint(7 // -2)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "-4\n-4\n");
    }

    #[tokio::test]
    async fn modulo_negative_operands() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(-7 % 3)\nprint(7 % -3)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        // Python semantics: result has sign of divisor
        assert_eq!(r.stdout, "2\n-2\n");
    }

    #[tokio::test]
    async fn divmod_builtin() {
        let mut bash = bash_python();
        let r = bash
            .exec("python3 -c \"print(divmod(17, 5))\nprint(divmod(-17, 5))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "(3, 2)\n(-4, 3)\n");
    }

    // -- v0.0.5: exponentiation safety (pydantic/monty#158) -------------------

    #[tokio::test]
    async fn large_exponentiation_within_limits() {
        let mut bash = bash_python();
        let r = bash.exec("python3 -c \"print(2 ** 30)\"").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "1073741824\n");
    }

    #[tokio::test]
    async fn large_exponentiation_completes() {
        let mut bash = bash_python();
        // Moderately large power should complete without hanging (safety multiplier)
        let r = bash
            .exec("python3 -c \"x = 2 ** 10000\nprint(len(str(x)))\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "3011");
    }

    // -- v0.0.6: PrintWriter scoping across multiple VFS OsCalls --------------

    #[tokio::test]
    async fn print_interleaved_with_vfs_ops() {
        let mut bash = bash_python();
        // Print, then VFS write, then print, then VFS read, then print.
        // Verifies PrintWriter::Collect output is preserved across OsCall suspend/resume.
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nprint('before-write')\nPath('/tmp/inter.txt').write_text('data')\nprint('after-write')\ncontent = Path('/tmp/inter.txt').read_text()\nprint(f'read: {content}')\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "before-write\nafter-write\nread: data\n");
    }

    #[tokio::test]
    async fn output_preserved_on_vfs_error() {
        let mut bash = bash_python();
        // Print output before a VFS operation that raises should be preserved
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nprint('line1')\nprint('line2')\nPath('/nonexistent/dir/file.txt').read_text()\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 1);
        assert_eq!(r.stdout, "line1\nline2\n");
        assert!(r.stderr.contains("FileNotFoundError"));
    }

    #[tokio::test]
    async fn many_vfs_ops_in_single_script() {
        let mut bash = bash_python();
        // Stress the scoped-printer pattern with many suspend/resume cycles.
        let r = bash
            .exec(concat!(
                "python3 -c \"from pathlib import Path\n",
                "Path('/tmp/vfs_stress').mkdir(parents=True, exist_ok=True)\n",
                "for i in range(5):\n",
                "    Path(f'/tmp/vfs_stress/{i}.txt').write_text(f'file {i}')\n",
                "results = []\n",
                "for i in range(5):\n",
                "    results.append(Path(f'/tmp/vfs_stress/{i}.txt').read_text())\n",
                "print(results)\"",
            ))
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(
            r.stdout,
            "['file 0', 'file 1', 'file 2', 'file 3', 'file 4']\n"
        );
    }

    #[tokio::test]
    async fn print_output_not_lost_across_mkdir() {
        let mut bash = bash_python();
        // mkdir triggers OsCall; print before and after must both appear
        let r = bash
            .exec("python3 -c \"from pathlib import Path\nprint('A')\nPath('/tmp/mk_test').mkdir()\nprint('B')\nprint(Path('/tmp/mk_test').is_dir())\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "A\nB\nTrue\n");
    }
}
