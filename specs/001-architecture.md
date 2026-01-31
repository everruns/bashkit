# 001: Architecture

## Status
Implemented

## Decision

BashKit uses a Cargo workspace with multiple crates:

```
bashkit/
├── crates/
│   ├── bashkit/           # Core library
│   └── bashkit-cli/       # CLI binary (future)
├── specs/                 # Design specifications
├── tests/                 # Integration tests (future)
└── Cargo.toml            # Workspace root
```

### Core Library Structure (`crates/bashkit/`)

```
src/
├── lib.rs                # Public API: Bash struct
├── error.rs              # Error types
├── parser/               # Lexer + Parser + AST
│   ├── mod.rs           # Parser implementation
│   ├── lexer.rs         # Tokenization
│   ├── tokens.rs        # Token types
│   └── ast.rs           # AST node types
├── interpreter/          # Execution engine
│   ├── mod.rs           # Interpreter implementation
│   └── state.rs         # ExecResult and state types
├── fs/                   # Virtual filesystem
│   ├── mod.rs           # Module exports
│   ├── traits.rs        # FileSystem trait
│   └── memory.rs        # InMemoryFs implementation
└── builtins/            # Built-in commands
    ├── mod.rs           # Builtin trait + Context
    ├── echo.rs          # echo command
    ├── flow.rs          # true, false, exit
    └── navigation.rs    # cd, pwd
```

### Public API

```rust
// Main entry point
pub struct Bash {
    fs: Arc<dyn FileSystem>,
    interpreter: Interpreter,
}

impl Bash {
    pub fn new() -> Self;
    pub fn builder() -> BashBuilder;
    pub async fn exec(&mut self, script: &str) -> Result<ExecResult>;
}

pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}
```

### Design Principles

1. **Async-first**: All filesystem and execution is async (tokio)
2. **Sandboxed**: No real filesystem access by default
3. **Multi-tenant safe**: Isolated state per Bash instance
4. **Trait-based**: FileSystem and Builtin traits for extensibility

## Alternatives Considered

### Single crate vs workspace
Rejected single crate because:
- CLI binary would bloat the library
- Future Python bindings need separate crate
- Cleaner separation of concerns

### Sync vs async filesystem
Rejected sync because:
- just-bash is fully async
- Future network operations need async
- tokio is already a dependency

## Verification

```bash
# Build succeeds
cargo build

# Tests pass including e2e
cargo test

# Basic usage works
cargo test --lib -- tests::test_echo_hello
```
