//! BashKit - Sandboxed bash interpreter for multi-tenant environments
//!
//! Part of the Everruns ecosystem.
//!
//! # Example
//!
//! ```rust
//! use bashkit::Bash;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let mut bash = Bash::new();
//!     let result = bash.exec("echo hello").await?;
//!     assert_eq!(result.stdout, "hello\n");
//!     assert_eq!(result.exit_code, 0);
//!     Ok(())
//! }
//! ```

mod builtins;
mod error;
mod fs;
mod interpreter;
mod parser;

pub use error::{Error, Result};
pub use interpreter::ExecResult;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use fs::{FileSystem, InMemoryFs};
use interpreter::Interpreter;
use parser::Parser;

/// Main entry point for BashKit.
///
/// Provides a sandboxed bash interpreter with an in-memory virtual filesystem.
pub struct Bash {
    #[allow(dead_code)] // Will be used for filesystem access methods
    fs: Arc<dyn FileSystem>,
    interpreter: Interpreter,
}

impl Default for Bash {
    fn default() -> Self {
        Self::new()
    }
}

impl Bash {
    /// Create a new Bash instance with default settings.
    pub fn new() -> Self {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let interpreter = Interpreter::new(Arc::clone(&fs));
        Self { fs, interpreter }
    }

    /// Create a new BashBuilder for customized configuration.
    pub fn builder() -> BashBuilder {
        BashBuilder::default()
    }

    /// Execute a bash script and return the result.
    pub async fn exec(&mut self, script: &str) -> Result<ExecResult> {
        let parser = Parser::new(script);
        let ast = parser.parse()?;
        self.interpreter.execute(&ast).await
    }
}

/// Builder for customized Bash configuration.
#[derive(Default)]
pub struct BashBuilder {
    fs: Option<Arc<dyn FileSystem>>,
    env: HashMap<String, String>,
    cwd: Option<PathBuf>,
}

impl BashBuilder {
    /// Set a custom filesystem.
    pub fn fs(mut self, fs: Arc<dyn FileSystem>) -> Self {
        self.fs = Some(fs);
        self
    }

    /// Set an environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set the current working directory.
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Build the Bash instance.
    pub fn build(self) -> Bash {
        let fs = self.fs.unwrap_or_else(|| Arc::new(InMemoryFs::new()));
        let mut interpreter = Interpreter::new(Arc::clone(&fs));

        for (key, value) in self.env {
            interpreter.set_env(&key, &value);
        }

        if let Some(cwd) = self.cwd {
            interpreter.set_cwd(cwd);
        }

        Bash { fs, interpreter }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo_hello() {
        let mut bash = Bash::new();
        let result = bash.exec("echo hello").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_echo_multiple_args() {
        let mut bash = Bash::new();
        let result = bash.exec("echo hello world").await.unwrap();
        assert_eq!(result.stdout, "hello world\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_variable_expansion() {
        let mut bash = Bash::builder().env("HOME", "/home/user").build();
        let result = bash.exec("echo $HOME").await.unwrap();
        assert_eq!(result.stdout, "/home/user\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_variable_brace_expansion() {
        let mut bash = Bash::builder().env("USER", "testuser").build();
        let result = bash.exec("echo ${USER}").await.unwrap();
        assert_eq!(result.stdout, "testuser\n");
    }

    #[tokio::test]
    async fn test_undefined_variable_expands_to_empty() {
        let mut bash = Bash::new();
        let result = bash.exec("echo $UNDEFINED_VAR").await.unwrap();
        assert_eq!(result.stdout, "\n");
    }

    #[tokio::test]
    async fn test_pipeline() {
        let mut bash = Bash::new();
        let result = bash.exec("echo hello | cat").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_pipeline_three_commands() {
        let mut bash = Bash::new();
        let result = bash.exec("echo hello | cat | cat").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_redirect_output() {
        let mut bash = Bash::new();
        let result = bash.exec("echo hello > /tmp/test.txt").await.unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 0);

        // Read the file back
        let result = bash.exec("cat /tmp/test.txt").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_redirect_append() {
        let mut bash = Bash::new();
        bash.exec("echo hello > /tmp/append.txt").await.unwrap();
        bash.exec("echo world >> /tmp/append.txt").await.unwrap();

        let result = bash.exec("cat /tmp/append.txt").await.unwrap();
        assert_eq!(result.stdout, "hello\nworld\n");
    }

    #[tokio::test]
    async fn test_command_list_and() {
        let mut bash = Bash::new();
        let result = bash.exec("true && echo success").await.unwrap();
        assert_eq!(result.stdout, "success\n");
    }

    #[tokio::test]
    async fn test_command_list_and_short_circuit() {
        let mut bash = Bash::new();
        let result = bash.exec("false && echo should_not_print").await.unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_command_list_or() {
        let mut bash = Bash::new();
        let result = bash.exec("false || echo fallback").await.unwrap();
        assert_eq!(result.stdout, "fallback\n");
    }

    #[tokio::test]
    async fn test_command_list_or_short_circuit() {
        let mut bash = Bash::new();
        let result = bash.exec("true || echo should_not_print").await.unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 0);
    }

    /// Phase 1 target test: `echo $HOME | cat > /tmp/out && cat /tmp/out`
    #[tokio::test]
    async fn test_phase1_target() {
        let mut bash = Bash::builder().env("HOME", "/home/testuser").build();

        let result = bash
            .exec("echo $HOME | cat > /tmp/out && cat /tmp/out")
            .await
            .unwrap();

        assert_eq!(result.stdout, "/home/testuser\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_redirect_input() {
        let mut bash = Bash::new();
        // Create a file first
        bash.exec("echo hello > /tmp/input.txt").await.unwrap();

        // Read it using input redirection
        let result = bash.exec("cat < /tmp/input.txt").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_here_string() {
        let mut bash = Bash::new();
        let result = bash.exec("cat <<< hello").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_if_true() {
        let mut bash = Bash::new();
        let result = bash.exec("if true; then echo yes; fi").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_if_false() {
        let mut bash = Bash::new();
        let result = bash.exec("if false; then echo yes; fi").await.unwrap();
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_if_else() {
        let mut bash = Bash::new();
        let result = bash
            .exec("if false; then echo yes; else echo no; fi")
            .await
            .unwrap();
        assert_eq!(result.stdout, "no\n");
    }

    #[tokio::test]
    async fn test_if_elif() {
        let mut bash = Bash::new();
        let result = bash
            .exec("if false; then echo one; elif true; then echo two; else echo three; fi")
            .await
            .unwrap();
        assert_eq!(result.stdout, "two\n");
    }

    #[tokio::test]
    async fn test_for_loop() {
        let mut bash = Bash::new();
        let result = bash.exec("for i in a b c; do echo $i; done").await.unwrap();
        assert_eq!(result.stdout, "a\nb\nc\n");
    }

    #[tokio::test]
    async fn test_while_loop() {
        let mut bash = Bash::new();
        // While with false condition - executes 0 times
        let result = bash.exec("while false; do echo loop; done").await.unwrap();
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_subshell() {
        let mut bash = Bash::new();
        let result = bash.exec("(echo hello)").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_brace_group() {
        let mut bash = Bash::new();
        let result = bash.exec("{ echo hello; }").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_function_keyword() {
        let mut bash = Bash::new();
        let result = bash
            .exec("function greet { echo hello; }; greet")
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_function_posix() {
        let mut bash = Bash::new();
        let result = bash.exec("greet() { echo hello; }; greet").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_function_args() {
        let mut bash = Bash::new();
        let result = bash
            .exec("greet() { echo $1 $2; }; greet world foo")
            .await
            .unwrap();
        assert_eq!(result.stdout, "world foo\n");
    }

    #[tokio::test]
    async fn test_function_arg_count() {
        let mut bash = Bash::new();
        let result = bash
            .exec("count() { echo $#; }; count a b c")
            .await
            .unwrap();
        assert_eq!(result.stdout, "3\n");
    }

    #[tokio::test]
    async fn test_case_literal() {
        let mut bash = Bash::new();
        let result = bash
            .exec("case foo in foo) echo matched ;; esac")
            .await
            .unwrap();
        assert_eq!(result.stdout, "matched\n");
    }

    #[tokio::test]
    async fn test_case_wildcard() {
        let mut bash = Bash::new();
        let result = bash
            .exec("case bar in *) echo default ;; esac")
            .await
            .unwrap();
        assert_eq!(result.stdout, "default\n");
    }

    #[tokio::test]
    async fn test_case_no_match() {
        let mut bash = Bash::new();
        let result = bash.exec("case foo in bar) echo no ;; esac").await.unwrap();
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_case_multiple_patterns() {
        let mut bash = Bash::new();
        let result = bash
            .exec("case foo in bar|foo|baz) echo matched ;; esac")
            .await
            .unwrap();
        assert_eq!(result.stdout, "matched\n");
    }

    #[tokio::test]
    async fn test_break_as_command() {
        let mut bash = Bash::new();
        // Just run break alone - should not error
        let result = bash.exec("break").await.unwrap();
        // break outside of loop returns success with no output
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_for_one_item() {
        let mut bash = Bash::new();
        // Simple for loop with one item
        let result = bash.exec("for i in a; do echo $i; done").await.unwrap();
        assert_eq!(result.stdout, "a\n");
    }

    #[tokio::test]
    async fn test_for_with_break() {
        let mut bash = Bash::new();
        // For loop with break
        let result = bash.exec("for i in a; do break; done").await.unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_for_echo_break() {
        let mut bash = Bash::new();
        // For loop with echo then break - tests the semicolon command list in body
        let result = bash
            .exec("for i in a b c; do echo $i; break; done")
            .await
            .unwrap();
        assert_eq!(result.stdout, "a\n");
    }

    #[tokio::test]
    async fn test_test_string_empty() {
        let mut bash = Bash::new();
        let result = bash.exec("test -z '' && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_test_string_not_empty() {
        let mut bash = Bash::new();
        let result = bash.exec("test -n 'hello' && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_test_string_equal() {
        let mut bash = Bash::new();
        let result = bash.exec("test foo = foo && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_test_string_not_equal() {
        let mut bash = Bash::new();
        let result = bash.exec("test foo != bar && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_test_numeric_equal() {
        let mut bash = Bash::new();
        let result = bash.exec("test 5 -eq 5 && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_test_numeric_less_than() {
        let mut bash = Bash::new();
        let result = bash.exec("test 3 -lt 5 && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_bracket_form() {
        let mut bash = Bash::new();
        let result = bash.exec("[ foo = foo ] && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_if_with_test() {
        let mut bash = Bash::new();
        let result = bash
            .exec("if [ 5 -gt 3 ]; then echo bigger; fi")
            .await
            .unwrap();
        assert_eq!(result.stdout, "bigger\n");
    }

    #[tokio::test]
    async fn test_variable_assignment() {
        let mut bash = Bash::new();
        let result = bash.exec("FOO=bar; echo $FOO").await.unwrap();
        assert_eq!(result.stdout, "bar\n");
    }

    #[tokio::test]
    async fn test_variable_assignment_inline() {
        let mut bash = Bash::new();
        // Assignment before command
        let result = bash.exec("MSG=hello; echo $MSG world").await.unwrap();
        assert_eq!(result.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_variable_assignment_only() {
        let mut bash = Bash::new();
        // Assignment without command should succeed silently
        let result = bash.exec("FOO=bar").await.unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 0);

        // Verify the variable was set
        let result = bash.exec("echo $FOO").await.unwrap();
        assert_eq!(result.stdout, "bar\n");
    }

    #[tokio::test]
    async fn test_multiple_assignments() {
        let mut bash = Bash::new();
        let result = bash.exec("A=1; B=2; C=3; echo $A $B $C").await.unwrap();
        assert_eq!(result.stdout, "1 2 3\n");
    }

    #[tokio::test]
    async fn test_printf_string() {
        let mut bash = Bash::new();
        let result = bash.exec("printf '%s' hello").await.unwrap();
        assert_eq!(result.stdout, "hello");
    }

    #[tokio::test]
    async fn test_printf_newline() {
        let mut bash = Bash::new();
        let result = bash.exec("printf 'hello\\n'").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_printf_multiple_args() {
        let mut bash = Bash::new();
        let result = bash.exec("printf '%s %s\\n' hello world").await.unwrap();
        assert_eq!(result.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_printf_integer() {
        let mut bash = Bash::new();
        let result = bash.exec("printf '%d' 42").await.unwrap();
        assert_eq!(result.stdout, "42");
    }

    #[tokio::test]
    async fn test_export() {
        let mut bash = Bash::new();
        let result = bash.exec("export FOO=bar; echo $FOO").await.unwrap();
        assert_eq!(result.stdout, "bar\n");
    }

    #[tokio::test]
    async fn test_read_basic() {
        let mut bash = Bash::new();
        let result = bash.exec("echo hello | read VAR; echo $VAR").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_read_multiple_vars() {
        let mut bash = Bash::new();
        let result = bash
            .exec("echo 'a b c' | read X Y Z; echo $X $Y $Z")
            .await
            .unwrap();
        assert_eq!(result.stdout, "a b c\n");
    }

    #[tokio::test]
    async fn test_glob_star() {
        let mut bash = Bash::new();
        // Create some files
        bash.exec("echo a > /tmp/file1.txt").await.unwrap();
        bash.exec("echo b > /tmp/file2.txt").await.unwrap();
        bash.exec("echo c > /tmp/other.log").await.unwrap();

        // Glob for *.txt files
        let result = bash.exec("echo /tmp/*.txt").await.unwrap();
        assert_eq!(result.stdout, "/tmp/file1.txt /tmp/file2.txt\n");
    }

    #[tokio::test]
    async fn test_glob_question_mark() {
        let mut bash = Bash::new();
        // Create some files
        bash.exec("echo a > /tmp/a1.txt").await.unwrap();
        bash.exec("echo b > /tmp/a2.txt").await.unwrap();
        bash.exec("echo c > /tmp/a10.txt").await.unwrap();

        // Glob for a?.txt (single character)
        let result = bash.exec("echo /tmp/a?.txt").await.unwrap();
        assert_eq!(result.stdout, "/tmp/a1.txt /tmp/a2.txt\n");
    }

    #[tokio::test]
    async fn test_glob_no_match() {
        let mut bash = Bash::new();
        // Glob that doesn't match anything should return the pattern
        let result = bash.exec("echo /nonexistent/*.xyz").await.unwrap();
        assert_eq!(result.stdout, "/nonexistent/*.xyz\n");
    }

    #[tokio::test]
    async fn test_command_substitution() {
        let mut bash = Bash::new();
        let result = bash.exec("echo $(echo hello)").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_command_substitution_in_string() {
        let mut bash = Bash::new();
        let result = bash.exec("echo \"result: $(echo 42)\"").await.unwrap();
        assert_eq!(result.stdout, "result: 42\n");
    }

    #[tokio::test]
    async fn test_command_substitution_pipeline() {
        let mut bash = Bash::new();
        let result = bash.exec("echo $(echo hello | cat)").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_command_substitution_variable() {
        let mut bash = Bash::new();
        let result = bash.exec("VAR=$(echo test); echo $VAR").await.unwrap();
        assert_eq!(result.stdout, "test\n");
    }

    #[tokio::test]
    async fn test_arithmetic_simple() {
        let mut bash = Bash::new();
        let result = bash.exec("echo $((1 + 2))").await.unwrap();
        assert_eq!(result.stdout, "3\n");
    }

    #[tokio::test]
    async fn test_arithmetic_multiply() {
        let mut bash = Bash::new();
        let result = bash.exec("echo $((3 * 4))").await.unwrap();
        assert_eq!(result.stdout, "12\n");
    }

    #[tokio::test]
    async fn test_arithmetic_with_variable() {
        let mut bash = Bash::new();
        let result = bash.exec("X=5; echo $((X + 3))").await.unwrap();
        assert_eq!(result.stdout, "8\n");
    }

    #[tokio::test]
    async fn test_arithmetic_complex() {
        let mut bash = Bash::new();
        let result = bash.exec("echo $((2 + 3 * 4))").await.unwrap();
        assert_eq!(result.stdout, "14\n");
    }

    #[tokio::test]
    async fn test_heredoc_simple() {
        let mut bash = Bash::new();
        let result = bash.exec("cat <<EOF\nhello\nworld\nEOF").await.unwrap();
        assert_eq!(result.stdout, "hello\nworld\n");
    }

    #[tokio::test]
    async fn test_heredoc_single_line() {
        let mut bash = Bash::new();
        let result = bash.exec("cat <<END\ntest\nEND").await.unwrap();
        assert_eq!(result.stdout, "test\n");
    }

    #[tokio::test]
    async fn test_unset() {
        let mut bash = Bash::new();
        let result = bash
            .exec("FOO=bar; unset FOO; echo \"x${FOO}y\"")
            .await
            .unwrap();
        assert_eq!(result.stdout, "xy\n");
    }

    #[tokio::test]
    async fn test_local_basic() {
        let mut bash = Bash::new();
        // Test that local command runs without error
        let result = bash.exec("local X=test; echo $X").await.unwrap();
        assert_eq!(result.stdout, "test\n");
    }

    #[tokio::test]
    async fn test_set_option() {
        let mut bash = Bash::new();
        let result = bash.exec("set -e; echo ok").await.unwrap();
        assert_eq!(result.stdout, "ok\n");
    }

    #[tokio::test]
    async fn test_param_default() {
        let mut bash = Bash::new();
        // ${var:-default} when unset
        let result = bash.exec("echo ${UNSET:-default}").await.unwrap();
        assert_eq!(result.stdout, "default\n");

        // ${var:-default} when set
        let result = bash.exec("X=value; echo ${X:-default}").await.unwrap();
        assert_eq!(result.stdout, "value\n");
    }

    #[tokio::test]
    async fn test_param_assign_default() {
        let mut bash = Bash::new();
        // ${var:=default} assigns when unset
        let result = bash.exec("echo ${NEW:=assigned}; echo $NEW").await.unwrap();
        assert_eq!(result.stdout, "assigned\nassigned\n");
    }

    #[tokio::test]
    async fn test_param_length() {
        let mut bash = Bash::new();
        let result = bash.exec("X=hello; echo ${#X}").await.unwrap();
        assert_eq!(result.stdout, "5\n");
    }

    #[tokio::test]
    async fn test_param_remove_prefix() {
        let mut bash = Bash::new();
        // ${var#pattern} - remove shortest prefix
        let result = bash.exec("X=hello.world.txt; echo ${X#*.}").await.unwrap();
        assert_eq!(result.stdout, "world.txt\n");
    }

    #[tokio::test]
    async fn test_param_remove_suffix() {
        let mut bash = Bash::new();
        // ${var%pattern} - remove shortest suffix
        let result = bash.exec("X=file.tar.gz; echo ${X%.*}").await.unwrap();
        assert_eq!(result.stdout, "file.tar\n");
    }
}
