//! Token types for the lexer
//!
//! Many token types are defined for future implementation phases.

#![allow(dead_code)]

/// Token types produced by the lexer.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// A word (command name, argument, etc.) - may contain variable expansions
    Word(String),

    /// A literal word (single-quoted) - no variable expansion
    LiteralWord(String),

    /// Newline character
    Newline,

    /// Semicolon (;)
    Semicolon,

    /// Pipe (|)
    Pipe,

    /// And (&&)
    And,

    /// Or (||)
    Or,

    /// Background (&)
    Background,

    /// Redirect output (>)
    RedirectOut,

    /// Redirect output append (>>)
    RedirectAppend,

    /// Redirect input (<)
    RedirectIn,

    /// Here document (<<)
    HereDoc,

    /// Here string (<<<)
    HereString,

    /// Left parenthesis (()
    LeftParen,

    /// Right parenthesis ())
    RightParen,

    /// Left brace ({)
    LeftBrace,

    /// Right brace (})
    RightBrace,

    /// Double left bracket ([[)
    DoubleLeftBracket,

    /// Double right bracket (]])
    DoubleRightBracket,

    /// Assignment (=)
    Assignment,

    /// Process substitution input <(cmd)
    ProcessSubIn,

    /// Process substitution output >(cmd)
    ProcessSubOut,
}
