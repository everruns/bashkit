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
    Command, CommandList, CompoundCommand, ForCommand, IfCommand, ListOperator, Pipeline, Redirect,
    RedirectKind, Script, SimpleCommand, UntilCommand, WhileCommand, Word, WordPart,
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
                Command::Compound(compound) => self.execute_compound(compound).await,
                Command::Function(_) => {
                    // TODO: Implement function definition
                    Err(Error::Execution(
                        "function definitions not yet implemented".to_string(),
                    ))
                }
            }
        })
    }

    /// Execute a compound command (if, for, while, etc.)
    async fn execute_compound(&mut self, compound: &CompoundCommand) -> Result<ExecResult> {
        match compound {
            CompoundCommand::If(if_cmd) => self.execute_if(if_cmd).await,
            CompoundCommand::For(for_cmd) => self.execute_for(for_cmd).await,
            CompoundCommand::While(while_cmd) => self.execute_while(while_cmd).await,
            CompoundCommand::Until(until_cmd) => self.execute_until(until_cmd).await,
            CompoundCommand::Subshell(commands) => self.execute_command_sequence(commands).await,
            CompoundCommand::BraceGroup(commands) => self.execute_command_sequence(commands).await,
            CompoundCommand::Case(_) => Err(Error::Execution(
                "case statements not yet implemented".to_string(),
            )),
        }
    }

    /// Execute an if statement
    async fn execute_if(&mut self, if_cmd: &IfCommand) -> Result<ExecResult> {
        // Execute condition
        let condition_result = self.execute_command_sequence(&if_cmd.condition).await?;

        if condition_result.exit_code == 0 {
            // Condition succeeded, execute then branch
            return self.execute_command_sequence(&if_cmd.then_branch).await;
        }

        // Check elif branches
        for (elif_condition, elif_body) in &if_cmd.elif_branches {
            let elif_result = self.execute_command_sequence(elif_condition).await?;
            if elif_result.exit_code == 0 {
                return self.execute_command_sequence(elif_body).await;
            }
        }

        // Execute else branch if present
        if let Some(else_branch) = &if_cmd.else_branch {
            return self.execute_command_sequence(else_branch).await;
        }

        // No branch executed, return success
        Ok(ExecResult::ok(String::new()))
    }

    /// Execute a for loop
    async fn execute_for(&mut self, for_cmd: &ForCommand) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        // Get iteration values
        let values: Vec<String> = if let Some(words) = &for_cmd.words {
            words
                .iter()
                .map(|w| self.expand_word(w))
                .collect::<Result<_>>()?
        } else {
            // TODO: Use positional parameters
            Vec::new()
        };

        for value in values {
            // Set loop variable
            self.variables
                .insert(for_cmd.variable.clone(), value.clone());

            // Execute body
            let result = self.execute_command_sequence(&for_cmd.body).await?;
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
        })
    }

    /// Execute a while loop
    async fn execute_while(&mut self, while_cmd: &WhileCommand) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        loop {
            // Check condition
            let condition_result = self.execute_command_sequence(&while_cmd.condition).await?;
            if condition_result.exit_code != 0 {
                break;
            }

            // Execute body
            let result = self.execute_command_sequence(&while_cmd.body).await?;
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
        })
    }

    /// Execute an until loop
    async fn execute_until(&mut self, until_cmd: &UntilCommand) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        loop {
            // Check condition
            let condition_result = self.execute_command_sequence(&until_cmd.condition).await?;
            if condition_result.exit_code == 0 {
                break;
            }

            // Execute body
            let result = self.execute_command_sequence(&until_cmd.body).await?;
            stdout.push_str(&result.stdout);
            stderr.push_str(&result.stderr);
            exit_code = result.exit_code;
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
        })
    }

    /// Execute a sequence of commands
    async fn execute_command_sequence(&mut self, commands: &[Command]) -> Result<ExecResult> {
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut exit_code = 0;

        for command in commands {
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

        // Handle input redirections first
        let stdin = self
            .process_input_redirections(stdin, &command.redirects)
            .await?;

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

            // Handle output redirections
            return self.apply_redirections(result, &command.redirects).await;
        }

        // Check for external commands
        // TODO: Implement command lookup and execution
        Err(Error::CommandNotFound(name))
    }

    /// Process input redirections (< file, <<< string)
    async fn process_input_redirections(
        &self,
        existing_stdin: Option<String>,
        redirects: &[Redirect],
    ) -> Result<Option<String>> {
        let mut stdin = existing_stdin;

        for redirect in redirects {
            match redirect.kind {
                RedirectKind::Input => {
                    let target_path = self.expand_word(&redirect.target)?;
                    let path = self.resolve_path(&target_path);
                    let content = self.fs.read_file(&path).await?;
                    stdin = Some(String::from_utf8_lossy(&content).to_string());
                }
                RedirectKind::HereString => {
                    // <<< string - use the target as stdin content
                    let content = self.expand_word(&redirect.target)?;
                    stdin = Some(format!("{}\n", content));
                }
                _ => {
                    // Output redirections handled separately
                }
            }
        }

        Ok(stdin)
    }

    /// Apply output redirections to command output
    async fn apply_redirections(
        &self,
        mut result: ExecResult,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        for redirect in redirects {
            match redirect.kind {
                RedirectKind::Output => {
                    let target_path = self.expand_word(&redirect.target)?;
                    let path = self.resolve_path(&target_path);
                    // Write stdout to file
                    self.fs.write_file(&path, result.stdout.as_bytes()).await?;
                    result.stdout = String::new();
                }
                RedirectKind::Append => {
                    let target_path = self.expand_word(&redirect.target)?;
                    let path = self.resolve_path(&target_path);
                    // Append stdout to file
                    self.fs.append_file(&path, result.stdout.as_bytes()).await?;
                    result.stdout = String::new();
                }
                RedirectKind::Input | RedirectKind::HereString => {
                    // Input redirections handled in process_input_redirections
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
