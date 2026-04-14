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

    /// A word that mixes quoted and unquoted segments where the unquoted
    /// portion contains glob metacharacters (*, ?, [).  Semantically
    /// equivalent to QuotedWord for IFS splitting (suppressed), but the
    /// interpreter must still perform glob expansion on the result.
    /// Example: `"$var"*.ext` or `./"$dir"/*.log`
    QuotedGlobWord(String),

    /// Newline character
    Newline,

    /// Semicolon (;)
    Semicolon,

    /// Double semicolon (;;) — case break
    DoubleSemicolon,

    /// Case fallthrough (;&)
    SemiAmp,

    /// Case continue-matching (;;&)
    DoubleSemiAmp,

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

    /// Clobber redirect (>|) - force overwrite even with noclobber
    Clobber,

    /// Duplicate output file descriptor (>&)
    DupOutput,

    /// Duplicate input file descriptor (<&)
    DupInput,

    /// Redirect with file descriptor (e.g., 2>)
    RedirectFd(i32),

    /// Redirect and append with file descriptor (e.g., 2>>)
    RedirectFdAppend(i32),

    /// Duplicate fd to another (e.g., 2>&1)
    DupFd(i32, i32),

    /// Duplicate input fd to another (e.g., 4<&0)
    DupFdIn(i32, i32),

    /// Close fd (e.g., 4<&- or 4>&-)
    DupFdClose(i32),

    /// Redirect input with file descriptor (e.g., 4<)
    RedirectFdIn(i32),

    /// Lexer error (e.g., unterminated string)
    Error(String),
}
