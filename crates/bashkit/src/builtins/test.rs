//! test builtin command ([ and test)
//!
//! An operand bash cannot use is not false, it is an error: `test 1 -eq abc`
//! writes `test: abc: integer expression expected` and exits 2. So the
//! evaluator returns `Result<bool, TestError>` rather than a bare bool, and
//! the two builtins turn the error into a diagnostic named after how they
//! were invoked (`test:` or `[:`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::fs::FileSystem;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;

/// An expression bash refuses to answer: it writes a diagnostic and exits 2.
struct TestError(String);

/// `Ok` is the expression's truth value, `Err` a usage error (exit 2).
type TestResult = std::result::Result<bool, TestError>;

/// `test: abc: integer expression expected`, named after the invocation.
fn usage_error(name: &str, e: TestError) -> ExecResult {
    ExecResult::err(format!("{name}: {}\n", e.0), 2)
}

/// The test builtin command.
pub struct Test;

#[async_trait]
impl Builtin for Test {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Handle empty args - returns false
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(String::new(), 1));
        }

        let cwd = ctx.cwd.clone();
        let set_vars = ctx.shell.as_ref().map_or(&[][..], |s| s.set_vars);
        let env = Env {
            fs: &ctx.fs,
            cwd: &cwd,
            variables: ctx.variables,
            set_vars,
        };
        // Parse and evaluate the expression
        match evaluate_expression(ctx.args, &env).await {
            Ok(true) => Ok(ExecResult::ok(String::new())),
            Ok(false) => Ok(ExecResult::err(String::new(), 1)),
            Err(e) => Ok(usage_error("test", e)),
        }
    }
}

/// The [ builtin (alias for test, but expects ] as last arg)
pub struct Bracket;

#[async_trait]
impl Builtin for Bracket {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Check for closing ]
        if ctx.args.is_empty() || ctx.args.last() != Some(&"]".to_string()) {
            return Ok(ExecResult::err("missing ]\n".to_string(), 2));
        }

        // Remove the trailing ]
        let args: Vec<String> = ctx.args[..ctx.args.len() - 1].to_vec();

        // Handle empty args - returns false
        if args.is_empty() {
            return Ok(ExecResult::err(String::new(), 1));
        }

        let cwd = ctx.cwd.clone();
        let set_vars = ctx.shell.as_ref().map_or(&[][..], |s| s.set_vars);
        let env = Env {
            fs: &ctx.fs,
            cwd: &cwd,
            variables: ctx.variables,
            set_vars,
        };
        // Parse and evaluate the expression
        match evaluate_expression(&args, &env).await {
            Ok(true) => Ok(ExecResult::ok(String::new())),
            Ok(false) => Ok(ExecResult::err(String::new(), 1)),
            Err(e) => Ok(usage_error("[", e)),
        }
    }
}

/// Resolve a file path against cwd (relative paths become absolute)
fn resolve_file_path(cwd: &Path, arg: &str) -> PathBuf {
    let p = Path::new(arg);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        vfs_join(cwd, p)
    }
}

/// What an expression can look at: the filesystem, the cwd, shell variables
/// (`set -o` state for `-o`) and the `-v` answers from the interpreter.
struct Env<'a> {
    fs: &'a Arc<dyn FileSystem>,
    cwd: &'a Path,
    variables: &'a HashMap<String, String>,
    set_vars: &'a [String],
}

/// A parsed `test` expression. Parsing follows bash's `test.c`: the POSIX
/// rules by argument count for up to four arguments, recursive descent
/// (`-o` below `-a` below `!`/`(`/primaries) beyond that.
enum Node {
    Const(bool),
    Unary(String, String),
    Binary(String, String, String),
    Not(Box<Node>),
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
}

/// bash `test_unop`: the unary primaries.
fn is_unop(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 2 && b[0] == b'-' && b"abcdefghknoprstuvwxzGLNORS".contains(&b[1])
}

/// bash `test_binop`: the binary primaries (`-a`/`-o` are connectives).
fn is_binop(s: &str) -> bool {
    matches!(
        s,
        "=" | "=="
            | "!="
            | "<"
            | ">"
            | "-nt"
            | "-ot"
            | "-ef"
            | "-eq"
            | "-ne"
            | "-lt"
            | "-le"
            | "-gt"
            | "-ge"
    )
}

struct Parser<'a> {
    argv: &'a [String],
    pos: usize,
}

fn syntax(msg: impl Into<String>) -> TestError {
    TestError(msg.into())
}

impl<'a> Parser<'a> {
    fn arg(&self, i: usize) -> &'a str {
        self.argv[i].as_str()
    }

    /// Step past the current word; `need` means another word must follow.
    fn advance(&mut self, need: bool) -> std::result::Result<(), TestError> {
        self.pos += 1;
        if need && self.pos >= self.argv.len() {
            return Err(syntax("argument expected"));
        }
        Ok(())
    }

    fn one_arg(&mut self) -> Node {
        let n = Node::Const(!self.arg(self.pos).is_empty());
        self.pos += 1;
        n
    }

    fn unary(&mut self) -> std::result::Result<Node, TestError> {
        let op = self.arg(self.pos).to_string();
        self.advance(true)?;
        let arg = self.arg(self.pos).to_string();
        self.pos += 1;
        Ok(Node::Unary(op, arg))
    }

    fn binary(&mut self) -> std::result::Result<Node, TestError> {
        let n = Node::Binary(
            self.arg(self.pos).to_string(),
            self.arg(self.pos + 1).to_string(),
            self.arg(self.pos + 2).to_string(),
        );
        self.pos += 3;
        Ok(n)
    }

    fn two_args(&mut self) -> std::result::Result<Node, TestError> {
        let first = self.arg(self.pos);
        if first == "!" {
            let n = Node::Const(self.arg(self.pos + 1).is_empty());
            self.pos += 2;
            Ok(n)
        } else if first.len() == 2 && first.starts_with('-') && is_unop(first) {
            self.unary()
        } else {
            Err(syntax(format!("{first}: unary operator expected")))
        }
    }

    fn three_args(&mut self) -> std::result::Result<Node, TestError> {
        let (a, b, c) = (
            self.arg(self.pos),
            self.arg(self.pos + 1),
            self.arg(self.pos + 2),
        );
        if is_binop(b) {
            self.binary()
        } else if b == "-a" || b == "-o" {
            let (l, r) = (
                Box::new(Node::Const(!a.is_empty())),
                Box::new(Node::Const(!c.is_empty())),
            );
            self.pos += 3;
            Ok(if b == "-a" {
                Node::And(l, r)
            } else {
                Node::Or(l, r)
            })
        } else if a == "!" {
            self.pos += 1;
            Ok(Node::Not(Box::new(self.two_args()?)))
        } else if a == "(" && c == ")" {
            let n = Node::Const(!b.is_empty());
            self.pos += 3;
            Ok(n)
        } else {
            Err(syntax(format!("{b}: binary operator expected")))
        }
    }

    fn expr(&mut self) -> std::result::Result<Node, TestError> {
        if self.pos >= self.argv.len() {
            return Err(syntax("argument expected"));
        }
        self.or()
    }

    fn or(&mut self) -> std::result::Result<Node, TestError> {
        let left = self.and()?;
        if self.pos < self.argv.len() && self.arg(self.pos) == "-o" {
            self.advance(false)?;
            let right = self.or()?;
            return Ok(Node::Or(Box::new(left), Box::new(right)));
        }
        Ok(left)
    }

    fn and(&mut self) -> std::result::Result<Node, TestError> {
        let left = self.term()?;
        if self.pos < self.argv.len() && self.arg(self.pos) == "-a" {
            self.advance(false)?;
            let right = self.and()?;
            return Ok(Node::And(Box::new(left), Box::new(right)));
        }
        Ok(left)
    }

    fn term(&mut self) -> std::result::Result<Node, TestError> {
        let argc = self.argv.len();
        if self.pos >= argc {
            return Err(syntax("argument expected"));
        }
        if self.arg(self.pos) == "!" {
            let mut negate = false;
            while self.pos < argc && self.arg(self.pos) == "!" {
                self.advance(true)?;
                negate = !negate;
            }
            let t = self.term()?;
            return Ok(if negate { Node::Not(Box::new(t)) } else { t });
        }
        if self.arg(self.pos) == "(" {
            self.advance(true)?;
            let value = self.expr()?;
            if self.pos >= argc {
                return Err(syntax("`)' expected"));
            }
            if self.arg(self.pos) != ")" {
                return Err(syntax(format!(
                    "`)' expected, found {}",
                    self.arg(self.pos)
                )));
            }
            self.advance(false)?;
            return Ok(value);
        }
        if self.pos + 3 <= argc && is_binop(self.arg(self.pos + 1)) {
            return self.binary();
        }
        if self.pos + 2 <= argc && is_unop(self.arg(self.pos)) {
            return self.unary();
        }
        Ok(self.one_arg())
    }

    /// bash `posixtest`: the argument-count rules, then `expr`.
    fn parse(&mut self) -> std::result::Result<Node, TestError> {
        let argc = self.argv.len();
        let node = match argc {
            0 => Node::Const(false),
            1 => self.one_arg(),
            2 => self.two_args()?,
            3 => self.three_args()?,
            4 if self.arg(0) == "!" => {
                self.pos = 1;
                Node::Not(Box::new(self.three_args()?))
            }
            4 if self.arg(0) == "(" && self.arg(3) == ")" => {
                self.pos = 1;
                let n = self.two_args()?;
                self.pos = argc;
                n
            }
            _ => self.expr()?,
        };
        if self.pos != argc {
            let next = self.arg(self.pos);
            return Err(if next.starts_with('-') {
                syntax(format!("syntax error: `{next}' unexpected"))
            } else {
                syntax("too many arguments")
            });
        }
        Ok(node)
    }
}

/// Evaluate a test expression
async fn evaluate_expression(args: &[String], env: &Env<'_>) -> TestResult {
    let node = Parser { argv: args, pos: 0 }.parse()?;
    eval_node(&node, env).await
}

/// Both sides of `-a`/`-o` are evaluated: bash's `test` connectives do not
/// short-circuit, so a bad operand on the right is an error even when the
/// left side already decides the answer.
fn eval_node<'a>(
    node: &'a Node,
    env: &'a Env<'a>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = TestResult> + Send + 'a>> {
    Box::pin(async move {
        match node {
            Node::Const(b) => Ok(*b),
            Node::Unary(op, arg) => evaluate_unary(op, arg, env).await,
            Node::Binary(l, op, r) => evaluate_binary(l, op, r, env.fs, env.cwd).await,
            Node::Not(n) => Ok(!eval_node(n, env).await?),
            Node::And(l, r) => {
                let (l, r) = (eval_node(l, env).await?, eval_node(r, env).await?);
                Ok(l && r)
            }
            Node::Or(l, r) => {
                let (l, r) = (eval_node(l, env).await?, eval_node(r, env).await?);
                Ok(l || r)
            }
        }
    })
}

/// Evaluate a unary test expression
async fn evaluate_unary(op: &str, arg: &str, env: &Env<'_>) -> TestResult {
    let (fs, cwd, variables) = (env.fs, env.cwd, env.variables);
    match op {
        // Shell state: a set variable, an enabled `set -o` option.
        "-v" => Ok(env.set_vars.iter().any(|v| v == arg)),
        "-o" => Ok(super::vars::set_o_option_on(variables, arg).unwrap_or(false)),
        // String tests
        "-z" => Ok(arg.is_empty()),
        "-n" => Ok(!arg.is_empty()),

        // File tests using the virtual filesystem
        "-e" | "-a" => {
            // file exists
            let path = resolve_file_path(cwd, arg);
            Ok(fs.exists(&path).await.unwrap_or(false))
        }
        "-f" => {
            // regular file
            let path = resolve_file_path(cwd, arg);
            Ok(match fs.stat(&path).await {
                Ok(meta) => meta.file_type.is_file(),
                Err(_) => false,
            })
        }
        "-d" => {
            // directory
            let path = resolve_file_path(cwd, arg);
            Ok(match fs.stat(&path).await {
                Ok(meta) => meta.file_type.is_dir(),
                Err(_) => false,
            })
        }
        "-r" => {
            // readable - in virtual fs, check if file exists
            // (permissions are stored but not enforced)
            let path = resolve_file_path(cwd, arg);
            Ok(fs.exists(&path).await.unwrap_or(false))
        }
        "-w" => {
            // writable - in virtual fs, check if file exists
            let path = resolve_file_path(cwd, arg);
            Ok(fs.exists(&path).await.unwrap_or(false))
        }
        "-x" => {
            // executable - in virtual fs, check if file exists and has executable permission
            let path = resolve_file_path(cwd, arg);
            Ok(match fs.stat(&path).await {
                // Check if any execute bit is set (u+x, g+x, o+x)
                Ok(meta) => (meta.mode & 0o111) != 0,
                Err(_) => false,
            })
        }
        "-s" => {
            // file exists and has size > 0
            let path = resolve_file_path(cwd, arg);
            Ok(match fs.stat(&path).await {
                Ok(meta) => meta.size > 0,
                Err(_) => false,
            })
        }
        "-L" | "-h" => {
            // symbolic link
            let path = resolve_file_path(cwd, arg);
            Ok(match fs.lstat(&path).await {
                Ok(meta) => meta.file_type.is_symlink(),
                Err(_) => false,
            })
        }
        "-p" => {
            // named pipe (FIFO)
            let path = resolve_file_path(cwd, arg);
            Ok(match fs.stat(&path).await {
                Ok(meta) => meta.file_type.is_fifo(),
                Err(_) => false,
            })
        }
        // Mode bits: setuid, setgid, sticky.
        "-u" | "-g" | "-k" => {
            let bit = match op {
                "-u" => 0o4000,
                "-g" => 0o2000,
                _ => 0o1000,
            };
            let path = resolve_file_path(cwd, arg);
            Ok(fs.stat(&path).await.is_ok_and(|meta| meta.mode & bit != 0))
        }
        // Owned by the effective user / group: the sandbox user owns every
        // file, so this is "exists".
        "-O" | "-G" => {
            let path = resolve_file_path(cwd, arg);
            Ok(fs.exists(&path).await.unwrap_or(false))
        }
        "-S" => Ok(false), // socket (not supported)
        "-b" => Ok(false), // block device (not supported)
        "-c" => {
            // The virtual character devices.
            let path = resolve_file_path(cwd, arg);
            Ok(matches!(
                path.to_str(),
                Some(
                    "/dev/null"
                        | "/dev/zero"
                        | "/dev/full"
                        | "/dev/random"
                        | "/dev/urandom"
                        | "/dev/tty"
                )
            ))
        }
        "-t" => {
            // file descriptor refers to a terminal
            // In VFS sandbox, defaults to false for all FDs.
            // Configurable via _TTY_N variables (e.g. _TTY_0=1 for stdin).
            let fd_key = format!("_TTY_{}", arg);
            Ok(variables.get(&fd_key).map(|v| v == "1").unwrap_or(false))
        }

        _ => Err(TestError(format!("{op}: unary operator expected"))),
    }
}

/// Evaluate a binary test expression
async fn evaluate_binary(
    left: &str,
    op: &str,
    right: &str,
    fs: &Arc<dyn FileSystem>,
    cwd: &Path,
) -> TestResult {
    match op {
        // String comparisons
        "=" | "==" => Ok(left == right),
        "!=" => Ok(left != right),
        "<" => Ok(left < right),
        ">" => Ok(left > right),

        // Numeric comparisons. An operand that is not an integer is a usage
        // error, not a false comparison: bash exits 2 and says so.
        "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" => {
            let (l, r) = (int_operand(left)?, int_operand(right)?);
            Ok(match op {
                "-eq" => l == r,
                "-ne" => l != r,
                "-lt" => l < r,
                "-le" => l <= r,
                "-gt" => l > r,
                _ => l >= r,
            })
        }

        // File comparisons
        "-nt" => {
            // file1 is newer than file2
            let left_meta = fs.stat(&resolve_file_path(cwd, left)).await;
            let right_meta = fs.stat(&resolve_file_path(cwd, right)).await;
            Ok(match (left_meta, right_meta) {
                (Ok(lm), Ok(rm)) => lm.modified > rm.modified,
                (Ok(_), Err(_)) => true, // left exists, right doesn't → left is newer
                _ => false,
            })
        }
        "-ot" => {
            // file1 is older than file2
            let left_meta = fs.stat(&resolve_file_path(cwd, left)).await;
            let right_meta = fs.stat(&resolve_file_path(cwd, right)).await;
            Ok(match (left_meta, right_meta) {
                (Ok(lm), Ok(rm)) => lm.modified < rm.modified,
                (Err(_), Ok(_)) => true, // left doesn't exist, right does → left is older
                _ => false,
            })
        }
        "-ef" => {
            // file1 and file2 refer to the same file (same path after resolution)
            // In VFS without inodes, compare canonical paths
            let left_path = super::resolve_path(cwd, left);
            let right_path = super::resolve_path(cwd, right);
            Ok(left_path == right_path)
        }

        _ => Err(TestError(format!("{op}: binary operator expected"))),
    }
}

/// An integer operand of `-eq` and friends. Bash accepts surrounding
/// whitespace and a sign but only decimal digits, so `0x10` and `1e3` are
/// errors, and so is a value that does not fit in a 64-bit integer.
fn int_operand(s: &str) -> std::result::Result<i64, TestError> {
    s.trim()
        .parse()
        .map_err(|_| TestError(format!("{s}: integer expression expected")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn setup() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();
        fs.mkdir(&cwd, true).await.unwrap();
        (fs, cwd, variables)
    }

    // ==================== test builtin ====================

    #[tokio::test]
    async fn test_empty_args_returns_false() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_nonempty_string_is_true() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["hello".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_z_empty_string() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-z".to_string(), "".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_z_nonempty_string() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-z".to_string(), "abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_n_nonempty_string() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-n".to_string(), "abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_n_empty_string() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-n".to_string(), "".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_string_equality() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string(), "=".to_string(), "abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_string_inequality() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string(), "!=".to_string(), "def".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_string_equality_fails() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string(), "=".to_string(), "xyz".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_eq_numeric() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["42".to_string(), "-eq".to_string(), "42".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_eq_numeric_fails() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["42".to_string(), "-eq".to_string(), "99".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_lt_numeric() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["5".to_string(), "-lt".to_string(), "10".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_lt_numeric_fails() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["10".to_string(), "-lt".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_gt_numeric() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["10".to_string(), "-gt".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_e_file_exists() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/file.txt"), b"hello")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-e".to_string(), "file.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_e_file_not_exists() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-e".to_string(), "nope.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_f_regular_file() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/file.txt"), b"data")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-f".to_string(), "file.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_f_directory_is_not_file() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.mkdir(Path::new("/home/user/subdir"), true)
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-f".to_string(), "subdir".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_d_directory() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.mkdir(Path::new("/home/user/subdir"), true)
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-d".to_string(), "subdir".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_d_file_is_not_dir() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/file.txt"), b"data")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-d".to_string(), "file.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_negation() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["!".to_string(), "-z".to_string(), "abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0); // ! -z "abc" => ! false => true
    }

    #[tokio::test]
    async fn test_negation_true_becomes_false() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["!".to_string(), "-n".to_string(), "abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1); // ! -n "abc" => ! true => false
    }

    // ==================== bracket builtin ====================

    #[tokio::test]
    async fn bracket_missing_closing() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-z".to_string(), "".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Bracket.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("missing ]"));
    }

    #[tokio::test]
    async fn bracket_with_closing() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-z".to_string(), "".to_string(), "]".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Bracket.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn bracket_empty_expression() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["]".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Bracket.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1); // empty expression => false
    }

    #[tokio::test]
    async fn bracket_empty_args() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Bracket.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
    }

    // ==================== additional numeric operators ====================

    #[tokio::test]
    async fn test_ne_numeric() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["1".to_string(), "-ne".to_string(), "2".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_ne_numeric_equal() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["5".to_string(), "-ne".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_le_numeric() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["5".to_string(), "-le".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_le_numeric_greater() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["10".to_string(), "-le".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_ge_numeric() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["5".to_string(), "-ge".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_ge_numeric_less() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["3".to_string(), "-ge".to_string(), "5".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    // ==================== string comparison operators ====================

    #[tokio::test]
    async fn test_double_eq_string() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["foo".to_string(), "==".to_string(), "foo".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_string_less_than() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string(), "<".to_string(), "def".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_string_greater_than() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["xyz".to_string(), ">".to_string(), "abc".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ==================== file tests: -s, -r, -w, -x ====================

    #[tokio::test]
    async fn test_s_file_has_size() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/nonempty.txt"), b"data")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-s".to_string(), "nonempty.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_s_empty_file() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/empty.txt"), b"")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-s".to_string(), "empty.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_r_readable_file() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/readable.txt"), b"x")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-r".to_string(), "readable.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_w_writable_file() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/home/user/writable.txt"), b"x")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-w".to_string(), "writable.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_r_nonexistent() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-r".to_string(), "nope".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    // ==================== absolute path handling ====================

    #[tokio::test]
    async fn test_e_absolute_path() {
        let (fs, mut cwd, mut variables) = setup().await;
        fs.write_file(Path::new("/tmp/abs.txt"), b"hi")
            .await
            .unwrap();
        let env = HashMap::new();
        let args = vec!["-e".to_string(), "/tmp/abs.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ==================== numeric parse edge cases ====================

    #[tokio::test]
    async fn test_eq_non_numeric_is_a_usage_error() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["abc".to_string(), "-eq".to_string(), "0".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert_eq!(
            result.stderr.to_string(),
            "test: abc: integer expression expected\n"
        );
    }

    #[tokio::test]
    async fn bracket_names_itself_in_a_usage_error() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["a".to_string(), "-lt".to_string(), "]".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Bracket.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert_eq!(result.stderr.to_string(), "[: a: unary operator expected\n");
    }

    #[tokio::test]
    async fn unknown_binary_operator_is_a_usage_error() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["a".to_string(), "foo".to_string(), "b".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert_eq!(
            result.stderr.to_string(),
            "test: foo: binary operator expected\n"
        );
    }

    #[tokio::test]
    async fn four_operands_are_too_many() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert_eq!(result.stderr.to_string(), "test: too many arguments\n");
    }

    /// `test`'s connectives do not short-circuit, so a bad operand on the
    /// side that cannot change the answer is still an error.
    #[tokio::test]
    async fn a_connective_evaluates_both_sides() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec![
            "1".to_string(),
            "-eq".to_string(),
            "2".to_string(),
            "-a".to_string(),
            "abc".to_string(),
            "-eq".to_string(),
            "1".to_string(),
        ];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert_eq!(
            result.stderr.to_string(),
            "test: abc: integer expression expected\n"
        );
    }

    /// Whitespace, a sign and leading zeros are fine; a base prefix,
    /// an exponent and an out-of-range value are not.
    #[tokio::test]
    async fn integer_operand_follows_bash() {
        for (operand, ok) in [
            (" 5 ", true),
            ("+5", true),
            ("010", true),
            ("0x10", false),
            ("1e3", false),
            ("99999999999999999999", false),
            ("", false),
        ] {
            let (fs, mut cwd, mut variables) = setup().await;
            let env = HashMap::new();
            let args = vec![operand.to_string(), "-ne".to_string(), "0".to_string()];
            let ctx =
                Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
            let result = Test.execute(ctx).await.unwrap();
            assert_eq!(result.exit_code == 2, !ok, "operand <{operand}>");
        }
    }

    #[tokio::test]
    async fn test_negative_numbers() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec!["-5".to_string(), "-lt".to_string(), "0".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ==================== bracket with binary ops ====================

    #[tokio::test]
    async fn bracket_numeric_eq() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec![
            "3".to_string(),
            "-eq".to_string(),
            "3".to_string(),
            "]".to_string(),
        ];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Bracket.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    // ==================== operator precedence ====================

    #[tokio::test]
    async fn test_or_and_precedence() {
        // [ true -o false -a false ] should be true (-a binds tighter than -o)
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec![
            "1".to_string(),
            "-eq".to_string(),
            "1".to_string(),
            "-o".to_string(),
            "1".to_string(),
            "-eq".to_string(),
            "2".to_string(),
            "-a".to_string(),
            "1".to_string(),
            "-eq".to_string(),
            "2".to_string(),
        ];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Test.execute(ctx).await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "-a should have higher precedence than -o"
        );
    }

    #[tokio::test]
    async fn bracket_string_neq() {
        let (fs, mut cwd, mut variables) = setup().await;
        let env = HashMap::new();
        let args = vec![
            "a".to_string(),
            "!=".to_string(),
            "b".to_string(),
            "]".to_string(),
        ];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let result = Bracket.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }
}
