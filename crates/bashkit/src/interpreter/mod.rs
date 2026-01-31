//! Interpreter for executing bash scripts

mod state;

pub use state::ExecResult;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::builtins::{self, Builtin};
use crate::error::{Error, Result};
use crate::fs::FileSystem;
use crate::parser::{Command, Script, SimpleCommand, Word, WordPart};

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

    async fn execute_command(&mut self, command: &Command) -> Result<ExecResult> {
        match command {
            Command::Simple(simple) => self.execute_simple_command(simple).await,
            Command::Pipeline(_) => {
                // TODO: Implement pipeline execution
                Err(Error::Execution(
                    "pipelines not yet implemented".to_string(),
                ))
            }
            Command::List(_) => {
                // TODO: Implement command list execution
                Err(Error::Execution(
                    "command lists not yet implemented".to_string(),
                ))
            }
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
    }

    async fn execute_simple_command(&mut self, command: &SimpleCommand) -> Result<ExecResult> {
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
            };

            return builtin.execute(ctx).await;
        }

        // Check for external commands
        // TODO: Implement command lookup and execution
        Err(Error::CommandNotFound(name))
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
