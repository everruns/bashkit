# 002: Parser Design

## Status
Implemented (core features)

## Decision

BashKit uses a recursive descent parser with a context-aware lexer.

### Tokenization Flow

```
Input → Lexer → Tokens → Parser → AST
```

### Token Types

```rust
pub enum Token {
    // Literals
    Word(String),           // Unquoted or quoted words
    Number(i64),            // Integer literals

    // Operators
    Pipe,                   // |
    And,                    // &&
    Or,                     // ||
    Semicolon,              // ;
    Newline,                // \n
    Background,             // &

    // Redirections
    RedirectOut,            // >
    RedirectAppend,         // >>
    RedirectIn,             // <
    HereDoc,                // <<
    HereString,             // <<<
    ProcessSubIn,           // <(
    ProcessSubOut,          // >(

    // File descriptor redirections (future)
    // 2>&1, &>, etc.

    // Grouping
    LeftParen,              // (
    RightParen,             // )
    LeftBrace,              // {
    RightBrace,             // }
    DoubleLeftBracket,      // [[
    DoubleRightBracket,     // ]]

    // Keywords (detected after lexing)
    // if, then, else, elif, fi
    // for, while, until, do, done
    // case, esac, in
    // function
}
```

### AST Structure

```rust
pub struct Script {
    pub commands: Vec<Command>,
}

pub enum Command {
    Simple(SimpleCommand),      // ls -la
    Pipeline(Pipeline),         // cmd1 | cmd2 | cmd3
    List(CommandList),          // cmd1 && cmd2 || cmd3
    Compound(CompoundCommand),  // if, for, while, case, { }, ( )
    Function(FunctionDef),      // function foo() { }
}

pub struct SimpleCommand {
    pub name: Word,
    pub args: Vec<Word>,
    pub redirects: Vec<Redirect>,
    pub assignments: Vec<Assignment>,  // VAR=value cmd
}

pub struct Word {
    pub parts: Vec<WordPart>,
}

pub enum WordPart {
    Literal(String),
    Variable(String),           // $VAR
    CommandSub(Script),         // $(cmd) or `cmd`
    ArithmeticSub(String),      // $((expr))
    DoubleQuoted(Vec<WordPart>), // "text $var"
}
```

### Parser Rules (Simplified)

```
script        → command_list EOF
command_list  → pipeline (('&&' | '||' | ';' | '&') pipeline)*
pipeline      → command ('|' command)*
command       → simple_command | compound_command | function_def
simple_command → (assignment)* word (word | redirect)*
redirect      → ('>' | '>>' | '<' | '<<' | '<<<') word
               | NUMBER ('>' | '<') word
```

Note: The `&` operator marks the preceding command for background execution.
Currently, background commands run synchronously but are parsed correctly.

### Context-Aware Lexing

The lexer must handle bash's context-sensitivity:
- `$var` in double quotes: expand variable
- `$var` in single quotes: literal text
- Word splitting after expansion
- Glob patterns (*, ?, [])

### Error Recovery

Parser produces errors with:
- Line and column numbers
- Expected vs. found token
- Context (what was being parsed)

## Alternatives Considered

### PEG parser (pest, pom)
Rejected because:
- Bash grammar is context-sensitive
- PEG can't handle here-docs well
- Manual parser gives better error messages

### Tree-sitter
Rejected because:
- Designed for incremental parsing (overkill)
- Would add large dependency
- Harder to customize for our needs

## Verification

```rust
#[test]
fn test_parse_pipeline() {
    let parser = Parser::new("echo hello | cat");
    let script = parser.parse().unwrap();
    assert!(matches!(script.commands[0], Command::Pipeline(_)));
}

#[test]
fn test_parse_redirect() {
    let parser = Parser::new("echo hello > /tmp/out");
    let script = parser.parse().unwrap();
    if let Command::Simple(cmd) = &script.commands[0] {
        assert_eq!(cmd.redirects.len(), 1);
    }
}
```
