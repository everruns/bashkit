# 001: Architecture

## Status
Implemented

## Decision

Bashkit uses a Cargo workspace with multiple crates:

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
├── limits.rs             # Execution limits
├── tool.rs               # LLM tool contract (Tool, ToolBuilder)
├── parser/               # Lexer + Parser + AST
│   ├── mod.rs           # Parser implementation
│   ├── lexer.rs         # Tokenization
│   ├── tokens.rs        # Token types
│   └── ast.rs           # AST node types
├── interpreter/          # Execution engine
│   ├── mod.rs           # Interpreter implementation
│   ├── state.rs         # ExecResult and state types
│   └── jobs.rs          # Job table for background execution
├── fs/                   # Virtual filesystem
│   ├── mod.rs           # Module exports
│   ├── traits.rs        # FileSystem trait
│   └── memory.rs        # InMemoryFs implementation
├── network/              # Network access (optional)
│   ├── mod.rs           # Module exports
│   ├── allowlist.rs     # URL allowlist
│   └── client.rs        # HTTP client
└── builtins/            # Built-in commands
    ├── mod.rs           # Builtin trait + Context
    ├── echo.rs          # echo, printf
    ├── flow.rs          # true, false, exit, break, continue, return
    ├── navigation.rs    # cd, pwd
    ├── fileops.rs       # mkdir, rm, cp, mv, touch, chmod
    ├── headtail.rs      # head, tail
    ├── sortuniq.rs      # sort, uniq
    ├── cuttr.rs         # cut, tr
    ├── wc.rs            # wc
    ├── date.rs          # date
    ├── sleep.rs         # sleep
    ├── wait.rs          # wait
    ├── curl.rs          # curl, wget
    └── ...              # grep, sed, awk, jq, etc.
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

// LLM Tool Contract
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn short_description(&self) -> &str;
    fn description(&self) -> String;          // Dynamic, includes custom builtins
    fn llmtext(&self) -> String;              // Full docs for LLMs
    fn system_prompt(&self) -> String;        // Token-efficient for sysprompt
    fn input_schema(&self) -> serde_json::Value;
    fn output_schema(&self) -> serde_json::Value;
    fn version(&self) -> &str;
    async fn execute(&mut self, req: ToolRequest) -> ToolResponse;
    async fn execute_with_status(...) -> ToolResponse;
}

pub struct BashTool { /* sandboxed bash implementing Tool */ }
pub struct BashToolBuilder { /* builder pattern */ }
pub struct ToolRequest { commands: String }   // Like bash -c
pub struct ToolResponse { stdout, stderr, exit_code, error }

impl BashTool {
    pub fn builder() -> BashToolBuilder;
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
