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

    /// Pipe with stderr (|&), shorthand for `2>&1 |`
    PipeBoth,

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

    /// Here document on a numbered descriptor (`3<<EOF`, `3<<-EOF` when the
    /// flag is set)
    HereDocFd(i32, bool),

    /// Here string (<<<)
    HereString,

    /// Here string on a numbered descriptor (`3<<<word`)
    HereStringFd(i32),

    /// Open read-write (`<>`)
    RedirectReadWrite,

    /// Open read-write on a numbered descriptor (`4<>file`)
    RedirectFdReadWrite(i32),

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

    /// Append both stdout and stderr (&>>)
    RedirectBothAppend,

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

    /// `N>&` followed by a word that is not a digit or `-` (`2>&$fd`,
    /// `1>&file`): the word decides at expansion time.
    DupFdWord(i32),

    /// Close output fd (e.g., 4>&-)
    DupFdCloseOut(i32),

    /// Duplicate input fd to another (e.g., 4<&0)
    DupFdIn(i32, i32),

    /// Close input fd (e.g., 4<&-)
    DupFdClose(i32),

    /// Redirect input with file descriptor (e.g., 4<)
    RedirectFdIn(i32),

    /// Lexer error (e.g., unterminated string)
    Error(String),
}
