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
- `test`, `[` - Conditionals (see Test Operators below)
- `read` - Input

#### Test Operators (`test` / `[`)

**String tests:**
- `-z string` - True if string is empty
- `-n string` - True if string is non-empty
- `s1 = s2`, `s1 == s2` - String equality
- `s1 != s2` - String inequality
- `s1 < s2`, `s1 > s2` - String comparison (lexicographic)

**Numeric tests:**
- `-eq`, `-ne`, `-lt`, `-le`, `-gt`, `-ge` - Integer comparisons

**File tests:**
- `-e file` - File exists
- `-f file` - Regular file
- `-d file` - Directory
- `-r file` - Readable
- `-w file` - Writable
- `-x file` - Executable
- `-s file` - Non-empty file
- `-L file`, `-h file` - Symbolic link

**Logical:**
- `! expr` - Negation
- `expr1 -a expr2` - AND
- `expr1 -o expr2` - OR

#### Shell Options (`set`)
- `set -e` / `set -o errexit` - Exit on error
- `set +e` - Disable errexit
- errexit respects conditionals (if, while, &&, ||)

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
- `grep` - Pattern matching (`-i`, `-v`, `-c`, `-n`, `-o`, `-l`, `-w`, `-E`, `-F`, `-q`, `-m`, `-x`, `-A`, `-B`, `-C`, `-e`)
- `sed` - Stream editing (s/pat/repl/, d, p, a, i; `-E`, `-e`, `-i`, `-n`; nth occurrence, `!` negation)
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
- `timeout` - Run command with time limit (stub, max 300s)

#### System Information
- `hostname` - Display sandbox hostname (configurable, default: "bashkit-sandbox")
- `uname` - System info (`-a`, `-s`, `-n`, `-r`, `-v`, `-m`, `-o`)
- `whoami` - Display sandbox username (configurable, default: "sandbox")
- `id` - User/group IDs (`-u`, `-g`, `-n`)

These builtins return configurable sandbox values to prevent host information disclosure.
Configure via `BashBuilder`:

```rust
Bash::builder()
    .username("deploy")      // Sets whoami, id, and $USER
    .hostname("my-server")   // Sets hostname, uname -n
    .build();
```

#### Directory Listing and Search
- `ls` - List directory contents (`-l`, `-a`, `-h`, `-1`, `-R`)
- `find` - Search for files (`-name PATTERN`, `-type f|d|l`, `-maxdepth N`, `-print`)
- `rmdir` - Remove empty directories (`-p` for parents)

#### File Inspection
- `less` - View file contents (sandbox: behaves like `cat`, no interactive paging)
- `file` - Detect file type via magic bytes (text, binary, PNG, JPEG, gzip, etc.)
- `stat` - Display file metadata (`-c FORMAT` with %n, %s, %F, %a, %U, %G, %Y, %Z)

#### Archive Operations
- `tar` - Create/extract tar archives (`-c`, `-x`, `-t`, `-v`, `-f`, `-z` for gzip)
- `gzip` - Compress files (`-d` decompress, `-k` keep, `-f` force)
- `gunzip` - Decompress files (`-k` keep, `-f` force)

#### Environment
- `env` - Print environment or run command with modified environment
- `printenv` - Print environment variable values
- `history` - Command history (sandbox: limited, no persistent history)

#### Pipeline Control
- `xargs` - Build commands from stdin (`-I REPL`, `-n MAX`, `-d DELIM`)
- `tee` - Write to files and stdout (`-a` append)
- `watch` - Execute command periodically (sandbox: shows command info, no continuous execution)

#### Network
- `curl` - HTTP client (requires http_client feature + allowlist)
  - Options: `-s/--silent`, `-o FILE`, `-X METHOD`, `-d DATA`, `-H HEADER`, `-I/--head`, `-f/--fail`, `-L/--location`, `-w FORMAT`, `--compressed`, `-u/--user`, `-A/--user-agent`, `-e/--referer`, `-v/--verbose`, `-m/--max-time`
  - Security: URL allowlist enforced, 10MB response limit, 30s timeout, zip bomb protection via size-limited decompression
- `wget` - Download files (requires http_client feature + allowlist)
  - Options: `-q/--quiet`, `-O FILE`, `--spider`, `--header`, `-U/--user-agent`, `--post-data`, `-t/--tries`
  - Security: URL allowlist enforced, 10MB response limit, 30s timeout

**Network Configuration**:
```rust
use bashkit::{Bash, NetworkAllowlist};

let bash = Bash::builder()
    .network(NetworkAllowlist::new()
        .allow("https://api.example.com")
        .allow("https://cdn.example.com"))
    .build();
```

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
    pub stdin: Option<&'a str>,
    // Only available with http_client feature:
    #[cfg(feature = "http_client")]
    pub http_client: Option<&'a HttpClient>,
}
```

### Custom Builtins

BashKit supports registering custom builtins via `BashBuilder`:

```rust
use bashkit::{Bash, Builtin, BuiltinContext, ExecResult, async_trait};

struct MyCommand;

#[async_trait]
impl Builtin for MyCommand {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        Ok(ExecResult::ok("Hello!\n".to_string()))
    }
}

let bash = Bash::builder()
    .builtin("mycommand", Box::new(MyCommand))
    .build();
```

Custom builtins:
- Have full access to execution context (args, env, fs, stdin)
- Can override default builtins if registered with the same name
- Must implement `Send + Sync` for async safety
- Integrate seamlessly with pipelines, conditionals, and loops

See `crates/bashkit/docs/custom_builtins.md` for detailed documentation.

### Safety Constraints

1. **No real filesystem access** - All operations use virtual filesystem
2. **Resource limits** - `sleep` capped at 60s, execution limits enforced
3. **Network restrictions** - URL allowlist required for network builtins
4. **No process spawning** - All commands are internal implementations

### Implementation Notes

- Background execution (`&`) is parsed but runs synchronously
- Network builtins require explicit allowlist configuration for security
- File operations respect virtual filesystem permissions
- Network responses are limited to 10MB by default to prevent memory exhaustion

## Verification

```bash
# All builtins work
cargo test --lib builtins

# Spec tests pass
cargo test --test spec_tests

# Full test suite
cargo test --all-features
```
