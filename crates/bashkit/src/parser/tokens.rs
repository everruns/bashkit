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

    /// A double-quoted word - may contain variable expansions inside,
    /// but is marked as quoted (affects heredoc delimiter semantics)
    QuotedWord(String),

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

    /// Here document with tab stripping (<<-)
    HereDocStrip,

    /// Here string (<<<)
    HereString,

    /// Left parenthesis (()
    LeftParen,

    /// Right parenthesis ())
    RightParen,

    /// Double left parenthesis ((()
    DoubleLeftParen,

    /// Double right parenthesis ()))
    DoubleRightParen,

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

    /// Redirect both stdout and stderr (&>)
    RedirectBoth,

    /// Duplicate output file descriptor (>&)
    DupOutput,

    /// Redirect with file descriptor (e.g., 2>)
    RedirectFd(i32),

    /// Redirect and append with file descriptor (e.g., 2>>)
    RedirectFdAppend(i32),

    /// Duplicate fd to another (e.g., 2>&1)
    DupFd(i32, i32),
}
