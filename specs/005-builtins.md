# 005: Builtin Commands

## Status
Implemented

## Decision

BashKit provides a comprehensive set of built-in commands for script execution
in a sandboxed environment. All builtins operate on the virtual filesystem.

### Builtin Categories

#### Core Shell Builtins
- `echo`, `printf` - Output text
- `true`, `false` - Exit status
- `exit`, `return` - Control flow
- `break`, `continue` - Loop control
- `cd`, `pwd` - Navigation
- `export`, `local`, `set`, `unset`, `shift` - Variable management
- `source`, `.` - Script sourcing
- `test`, `[` - Conditionals
- `read` - Input

#### File Operations
- `mkdir` - Create directories (`-p` for parents)
- `rm` - Remove files/directories (`-r`, `-f`)
- `cp` - Copy files (`-r` for directories)
- `mv` - Move/rename files
- `touch` - Create empty files
- `chmod` - Change permissions (octal mode)

#### Text Processing
- `cat` - Concatenate files
- `head`, `tail` - First/last N lines
- `grep` - Pattern matching (`-i`, `-v`, `-c`, `-n`, `-E`, `-q`)
- `sed` - Stream editing (s/pat/repl/, d, p)
- `awk` - Text processing (print, -F, variables)
- `jq` - JSON processing
- `sort` - Sort lines (`-r`, `-n`, `-u`)
- `uniq` - Filter duplicates (`-c`, `-d`, `-u`)
- `cut` - Extract fields (`-d`, `-f`)
- `tr` - Translate characters (`-d` for delete)
- `wc` - Count lines/words/bytes (`-l`, `-w`, `-c`)

#### Utilities
- `sleep` - Pause execution (max 60s for safety)
- `date` - Date/time formatting (`+FORMAT`, `-u`)
- `basename`, `dirname` - Path manipulation
- `wait` - Wait for background jobs

#### Network (Stubs)
- `curl` - HTTP client (requires network feature + allowlist)
- `wget` - Download files (requires network feature + allowlist)

### Builtin Trait

```rust
#[async_trait]
pub trait Builtin: Send + Sync {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult>;
}

pub struct Context<'a> {
    pub args: &'a [String],
    pub env: &'a HashMap<String, String>,
    pub variables: &'a mut HashMap<String, String>,
    pub cwd: &'a mut PathBuf,
    pub fs: Arc<dyn FileSystem>,
    pub stdin: Option<String>,
}
```

### Safety Constraints

1. **No real filesystem access** - All operations use virtual filesystem
2. **Resource limits** - `sleep` capped at 60s, execution limits enforced
3. **Network restrictions** - URL allowlist required for network builtins
4. **No process spawning** - All commands are internal implementations

### Implementation Notes

- Background execution (`&`) is parsed but runs synchronously
- Network builtins are stubs requiring explicit configuration
- File operations respect virtual filesystem permissions

## Verification

```bash
# All builtins work
cargo test --lib builtins

# Spec tests pass
cargo test --test spec_tests

# Full test suite
cargo test --all-features
```
