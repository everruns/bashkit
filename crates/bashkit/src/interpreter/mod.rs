//! Interpreter for executing bash scripts

mod state;

pub use state::ExecResult;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::builtins::{self, Builtin};
use crate::error::{Error, Result};
use crate::fs::FileSystem;
use crate::parser::{
    Command, CommandList, ListOperator, Pipeline, Redirect, RedirectKind, Script, SimpleCommand,
    Word, WordPart,
};

/// Interpreter state.
pub struct Interpreter {
    fs: Arc<dyn FileSystem>,
    env: HashMap<String, String>,
    variables: HashMap<String, String>,
    cwd: PathBuf,
    last_exit_code: i32,
    builtins: HashMap<&'static str, Box<dyn Builtin>>,
}

impl Interpreter {
    /// Create a new interpreter with the given filesystem.
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        let mut builtins: HashMap<&'static str, Box<dyn Builtin>> = HashMap::new();

        // Register builtins
        builtins.insert("echo", Box::new(builtins::Echo));
        builtins.insert("true", Box::new(builtins::True));
        builtins.insert("false", Box::new(builtins::False));
        builtins.insert("exit", Box::new(builtins::Exit));
        builtins.insert("cd", Box::new(builtins::Cd));
        builtins.insert("pwd", Box::new(builtins::Pwd));
        builtins.insert("cat", Box::new(builtins::Cat));

        Self {
            fs,
            env: HashMap::new(),
            variables: HashMap::new(),
            cwd: PathBuf::from("/home/user"),
            last_exit_code: 0,
            builtins,
        }
    }

    /// Set an environment variable.
    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env.insert(key.to_string(), value.to_string());
    }

    /// Set the current working directory.
    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
    }

    /// Execute a script.
    pub async fn execute(&mut self, script: &Script) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        for command in &script.commands {
            let result = self.execute_command(command).await?;
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;
            self.last_exit_code = exit_code;
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
        })
    }

    fn execute_command<'a>(
        &'a mut self,
        command: &'a Command,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>> {
        Box::pin(async move {
            match command {
                Command::Simple(simple) => self.execute_simple_command(simple, None).await,
                Command::Pipeline(pipeline) => self.execute_pipeline(pipeline).await,
                Command::List(list) => self.execute_list(list).await,
                Command::Compound(_) => {
                    // TODO: Implement compound command execution
                    Err(Error::Execution(
                        "compound commands not yet implemented".to_string(),
                    ))
                }
                Command::Function(_) => {
                    // TODO: Implement function definition
                    Err(Error::Execution(
                        "function definitions not yet implemented".to_string(),
                    ))
                }
            }
        })
    }

    /// Execute a pipeline (cmd1 | cmd2 | cmd3)
    async fn execute_pipeline(&mut self, pipeline: &Pipeline) -> Result<ExecResult> {
        let mut stdin_data: Option<String> = None;
        let mut last_result = ExecResult::ok(String::new());

        for (i, command) in pipeline.commands.iter().enumerate() {
            let is_last = i == pipeline.commands.len() - 1;

            match command {
                Command::Simple(simple) => {
                    let result = self
                        .execute_simple_command(simple, stdin_data.take())
                        .await?;

                    if is_last {
                        last_result = result;
                    } else {
                        // Pass stdout to next command's stdin
                        stdin_data = Some(result.stdout);
                    }
                }
                _ => {
                    return Err(Error::Execution(
                        "only simple commands supported in pipeline".to_string(),
                    ))
                }
            }
        }

        // Handle negation
        if pipeline.negated {
            last_result.exit_code = if last_result.exit_code == 0 { 1 } else { 0 };
        }

        Ok(last_result)
    }

    /// Execute a command list (cmd1 && cmd2 || cmd3)
    async fn execute_list(&mut self, list: &CommandList) -> Result<ExecResult> {
        let mut result = self.execute_command(&list.first).await?;

        for (op, cmd) in &list.rest {
            let should_execute = match op {
                ListOperator::And => result.exit_code == 0,
                ListOperator::Or => result.exit_code != 0,
                ListOperator::Semicolon => true,
                ListOperator::Background => {
                    // TODO: Implement background execution
                    true
                }
            };

            if should_execute {
                result = self.execute_command(cmd).await?;
            }
        }

        Ok(result)
    }

    async fn execute_simple_command(
        &mut self,
        command: &SimpleCommand,
        stdin: Option<String>,
    ) -> Result<ExecResult> {
        let name = self.expand_word(&command.name)?;
        let args: Vec<String> = command
            .args
            .iter()
            .map(|w| self.expand_word(w))
            .collect::<Result<_>>()?;

        // Check for builtins
        if let Some(builtin) = self.builtins.get(name.as_str()) {
            let ctx = builtins::Context {
                args: &args,
                env: &self.env,
                variables: &mut self.variables,
                cwd: &mut self.cwd,
                fs: Arc::clone(&self.fs),
                stdin: stdin.as_deref(),
            };

            let result = builtin.execute(ctx).await?;

            // Handle redirections
            return self.apply_redirections(result, &command.redirects).await;
        }

        // Check for external commands
        // TODO: Implement command lookup and execution
        Err(Error::CommandNotFound(name))
    }

    /// Apply redirections to command output
    async fn apply_redirections(
        &self,
        mut result: ExecResult,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        for redirect in redirects {
            let target_path = self.expand_word(&redirect.target)?;
            let path = self.resolve_path(&target_path);

            match redirect.kind {
                RedirectKind::Output => {
                    // Write stdout to file
                    self.fs.write_file(&path, result.stdout.as_bytes()).await?;
                    result.stdout = String::new();
                }
                RedirectKind::Append => {
                    // Append stdout to file
                    self.fs.append_file(&path, result.stdout.as_bytes()).await?;
                    result.stdout = String::new();
                }
                RedirectKind::Input => {
                    // Read file to stdin (handled in command execution)
                    // This is handled before command execution in execute_simple_command
                }
                RedirectKind::HereString => {
                    // Here string is handled as stdin
                    // Already handled in parsing/command setup
                }
                _ => {
                    // TODO: Handle other redirect types (HereDoc, DupOutput, DupInput, OutputBoth)
                }
            }
        }

        Ok(result)
    }

    /// Resolve a path relative to cwd
    fn resolve_path(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.cwd.join(p)
        }
    }

    fn expand_word(&self, word: &Word) -> Result<String> {
        let mut result = String::new();

        for part in &word.parts {
            match part {
                WordPart::Literal(s) => result.push_str(s),
                WordPart::Variable(name) => {
                    // Check shell variables first, then environment
                    if let Some(value) = self.variables.get(name) {
                        result.push_str(value);
                    } else if let Some(value) = self.env.get(name) {
                        result.push_str(value);
                    }
                    // If not found, expand to empty string (bash behavior)
                }
                WordPart::CommandSubstitution(_) => {
                    // TODO: Implement command substitution
                }
                WordPart::ArithmeticExpansion(_) => {
                    // TODO: Implement arithmetic expansion
                }
            }
        }

        Ok(result)
    }
}
