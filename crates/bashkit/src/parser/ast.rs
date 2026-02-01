//! AST types for parsed bash scripts
//!
//! These types define the abstract syntax tree for bash scripts.
//! Many types are not yet used but are defined for future implementation phases.

#![allow(dead_code)]

use std::fmt;

/// A complete bash script.
#[derive(Debug, Clone)]
pub struct Script {
    pub commands: Vec<Command>,
}

/// A single command in the script.
#[derive(Debug, Clone)]
pub enum Command {
    /// A simple command (e.g., `echo hello`)
    Simple(SimpleCommand),

    /// A pipeline (e.g., `ls | grep foo`)
    Pipeline(Pipeline),

    /// A command list (e.g., `a && b || c`)
    List(CommandList),

    /// A compound command (if, for, while, case, etc.)
    Compound(CompoundCommand),

    /// A function definition
    Function(FunctionDef),
}

/// A simple command with arguments and redirections.
#[derive(Debug, Clone)]
pub struct SimpleCommand {
    /// Command name
    pub name: Word,
    /// Command arguments
    pub args: Vec<Word>,
    /// Redirections
    pub redirects: Vec<Redirect>,
    /// Variable assignments before the command
    pub assignments: Vec<Assignment>,
}

/// A pipeline of commands.
#[derive(Debug, Clone)]
pub struct Pipeline {
    /// Whether the pipeline is negated (!)
    pub negated: bool,
    /// Commands in the pipeline
    pub commands: Vec<Command>,
}

/// A list of commands with operators.
#[derive(Debug, Clone)]
pub struct CommandList {
    /// First command
    pub first: Box<Command>,
    /// Remaining commands with their operators
    pub rest: Vec<(ListOperator, Command)>,
}

/// Operators for command lists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ListOperator {
    /// && - execute next if previous succeeded
    And,
    /// || - execute next if previous failed
    Or,
    /// ; - execute next unconditionally
    Semicolon,
    /// & - execute in background
    Background,
}

/// Compound commands (control structures).
#[derive(Debug, Clone)]
pub enum CompoundCommand {
    /// If statement
    If(IfCommand),
    /// For loop
    For(ForCommand),
    /// While loop
    While(WhileCommand),
    /// Until loop
    Until(UntilCommand),
    /// Case statement
    Case(CaseCommand),
    /// Subshell (commands in parentheses)
    Subshell(Vec<Command>),
    /// Brace group
    BraceGroup(Vec<Command>),
}

/// If statement.
#[derive(Debug, Clone)]
pub struct IfCommand {
    pub condition: Vec<Command>,
    pub then_branch: Vec<Command>,
    pub elif_branches: Vec<(Vec<Command>, Vec<Command>)>,
    pub else_branch: Option<Vec<Command>>,
}

/// For loop.
#[derive(Debug, Clone)]
pub struct ForCommand {
    pub variable: String,
    pub words: Option<Vec<Word>>,
    pub body: Vec<Command>,
}

/// While loop.
#[derive(Debug, Clone)]
pub struct WhileCommand {
    pub condition: Vec<Command>,
    pub body: Vec<Command>,
}

/// Until loop.
#[derive(Debug, Clone)]
pub struct UntilCommand {
    pub condition: Vec<Command>,
    pub body: Vec<Command>,
}

/// Case statement.
#[derive(Debug, Clone)]
pub struct CaseCommand {
    pub word: Word,
    pub cases: Vec<CaseItem>,
}

/// A single case item.
#[derive(Debug, Clone)]
pub struct CaseItem {
    pub patterns: Vec<Word>,
    pub commands: Vec<Command>,
}

/// Function definition.
#[derive(Debug, Clone)]
pub struct FunctionDef {
    pub name: String,
    pub body: Box<Command>,
}

/// A word (potentially with expansions).
#[derive(Debug, Clone)]
pub struct Word {
    pub parts: Vec<WordPart>,
}

impl Word {
    /// Create a simple literal word.
    pub fn literal(s: impl Into<String>) -> Self {
        Self {
            parts: vec![WordPart::Literal(s.into())],
        }
    }
}

impl fmt::Display for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for part in &self.parts {
            match part {
                WordPart::Literal(s) => write!(f, "{}", s)?,
                WordPart::Variable(name) => write!(f, "${}", name)?,
                WordPart::CommandSubstitution(cmd) => write!(f, "$({:?})", cmd)?,
                WordPart::ArithmeticExpansion(expr) => write!(f, "$(({}))", expr)?,
                WordPart::ParameterExpansion {
                    name,
                    operator,
                    operand,
                } => {
                    let op_str = match operator {
                        ParameterOp::UseDefault => ":-",
                        ParameterOp::AssignDefault => ":=",
                        ParameterOp::UseReplacement => ":+",
                        ParameterOp::Error => ":?",
                        ParameterOp::RemovePrefixShort => "#",
                        ParameterOp::RemovePrefixLong => "##",
                        ParameterOp::RemoveSuffixShort => "%",
                        ParameterOp::RemoveSuffixLong => "%%",
                    };
                    write!(f, "${{{}{}{}}}", name, op_str, operand)?
                }
                WordPart::Length(name) => write!(f, "${{#{}}}", name)?,
                WordPart::ArrayAccess { name, index } => write!(f, "${{{}[{}]}}", name, index)?,
                WordPart::ArrayLength(name) => write!(f, "${{#{}[@]}}", name)?,
            }
        }
        Ok(())
    }
}

/// Parts of a word.
#[derive(Debug, Clone)]
pub enum WordPart {
    /// Literal text
    Literal(String),
    /// Variable expansion ($VAR or ${VAR})
    Variable(String),
    /// Command substitution ($(...))
    CommandSubstitution(Vec<Command>),
    /// Arithmetic expansion ($((...)))
    ArithmeticExpansion(String),
    /// Parameter expansion with operator ${var:-default}, ${var:=default}, etc.
    ParameterExpansion {
        name: String,
        operator: ParameterOp,
        operand: String,
    },
    /// Length expansion ${#var}
    Length(String),
    /// Array element access ${arr[index]} or ${arr[@]} or ${arr[*]}
    ArrayAccess { name: String, index: String },
    /// Array length ${#arr[@]} or ${#arr[*]}
    ArrayLength(String),
}

/// Parameter expansion operators
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterOp {
    /// :- use default if unset/empty
    UseDefault,
    /// := assign default if unset/empty
    AssignDefault,
    /// :+ use replacement if set
    UseReplacement,
    /// :? error if unset/empty
    Error,
    /// # remove prefix (shortest)
    RemovePrefixShort,
    /// ## remove prefix (longest)
    RemovePrefixLong,
    /// % remove suffix (shortest)
    RemoveSuffixShort,
    /// %% remove suffix (longest)
    RemoveSuffixLong,
}

/// I/O redirection.
#[derive(Debug, Clone)]
pub struct Redirect {
    /// File descriptor (default: 1 for output, 0 for input)
    pub fd: Option<i32>,
    /// Type of redirection
    pub kind: RedirectKind,
    /// Target (file, fd, or heredoc content)
    pub target: Word,
}

/// Types of redirections.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RedirectKind {
    /// > - redirect output
    Output,
    /// >> - append output
    Append,
    /// < - redirect input
    Input,
    /// << - here document
    HereDoc,
    /// <<< - here string
    HereString,
    /// >& - duplicate output fd
    DupOutput,
    /// <& - duplicate input fd
    DupInput,
    /// &> - redirect both stdout and stderr
    OutputBoth,
}

/// Variable assignment.
#[derive(Debug, Clone)]
pub struct Assignment {
    pub name: String,
    /// Optional array index for indexed assignments like arr[0]=value
    pub index: Option<String>,
    pub value: AssignmentValue,
    /// Whether this is an append assignment (+=)
    pub append: bool,
}

/// Value in an assignment - scalar or array
#[derive(Debug, Clone)]
pub enum AssignmentValue {
    /// Scalar value: VAR=value
    Scalar(Word),
    /// Array value: VAR=(a b c)
    Array(Vec<Word>),
}
