# 005: Builtin Commands

## Status
Implemented

## Decision

Bashkit provides a comprehensive set of built-in commands for script execution
in a virtual environment. All builtins operate on the virtual filesystem.

### Builtin Categories

#### Core Shell Builtins
- `echo`, `printf` - Output text
- `true`, `false` - Exit status
- `exit`, `return` - Control flow
- `break`, `continue` - Loop control
- `cd`, `pwd` - Navigation
- `export`, `local`, `set`, `unset`, `shift` - Variable management
- `source`, `.` - Script sourcing (functions, variables, PATH search, positional params)
- `test`, `[` - Conditionals (see Test Operators below)
- `read` - Input
- `alias`, `unalias` - Alias management (gated by `shopt -s expand_aliases`, parser-time first-word expansion, trailing-space chaining, recursion guard)

#### Script Execution by Path

Commands containing `/` (absolute or relative paths) are resolved against the
VFS. Commands without `/` are searched in `$PATH` directories for executable
files. The dispatch order is: functions → special commands → builtins → path
execution → $PATH search → "command not found".

- Absolute: `/path/to/script.sh` — resolved directly
- Relative: `./script.sh` — resolved relative to cwd
- $PATH search: `myscript` — searches each `$PATH` directory for executable file
- Shebang (`#!/bin/bash`) stripped; content executed as bash
- `$0` = script name, `$1..N` = arguments
- Exit 127: file not found; Exit 126: not executable or is a directory

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
- `chown` - Change ownership (no-op in VFS, validates file existence)
- `ln` - Create links (`-s` symbolic, `-f` force)
- `kill` - Send signals (no-op in VFS, `-l` lists signals)

#### Text Processing
- `cat` - Concatenate files (`-v`, `-n`, `-e`, `-t`)
- `nl` - Number lines (`-b`, `-n`, `-s`, `-i`, `-v`, `-w`)
- `head`, `tail` - First/last N lines
- `grep` - Pattern matching (`-i`, `-v`, `-c`, `-n`, `-o`, `-l`, `-w`, `-E`, `-F`, `-P`, `-q`, `-m`, `-x`, `-A`, `-B`, `-C`, `-e`, `-f`, `-H`, `-h`, `-b`, `-a`, `-z`, `-r`)
- `sed` - Stream editing (s/pat/repl/, d, p, a, i; `-E`, `-e`, `-i`, `-n`; nth occurrence, `!` negation)
- `awk` - Text processing (print, -F, variables, `--csv`/`-k`, `\u` Unicode escapes)
- `jq` - JSON processing (file arguments, `-s`, `-r`, `-c`, `-n`, `-S`, `-e`, `--tab`, `-j`, `--arg`, `--argjson`, `-V`/`--version`, combined short flags)
- `sort` - Sort lines (`-r`, `-n`, `-u`)
- `uniq` - Filter duplicates (`-c`, `-d`, `-u`)
- `cut` - Extract fields (`-d`, `-f`)
- `tr` - Translate characters (`-d` for delete)
- `wc` - Count lines/words/bytes (`-l`, `-w`, `-c`)
- `paste` - Merge lines of files (`-d`, `-s`)
- `column` - Columnate lists (`-t`, `-s`, `-o`)
- `diff` - Compare files line by line (`-u`, `-q`)
- `comm` - Compare two sorted files (`-1`, `-2`, `-3`)

#### Byte Inspection
- `od` - Octal dump (`-A`, `-t`, `-N`, `-j`)
- `xxd` - Hex dump (`-l`, `-s`, `-c`, `-g`, `-p`, `-r`)
- `hexdump` - Hex display (`-C`, `-n`, `-s`)
- `strings` - Extract printable strings (`-n`, `-t`)

#### Utilities
- `sleep` - Pause execution (max 60s for safety)
- `date` - Date/time formatting (`+FORMAT`, `-u`)
- `basename`, `dirname` - Path manipulation
- `wait` - Wait for background jobs
- `timeout` - Run command with time limit (stub, max 300s)

#### System Information
- `hostname` - Display virtual hostname (configurable, default: "bashkit-sandbox")
- `uname` - System info (`-a`, `-s`, `-n`, `-r`, `-v`, `-m`, `-o`)
- `whoami` - Display virtual username (configurable, default: "sandbox")
- `id` - User/group IDs (`-u`, `-g`, `-n`)

These builtins return configurable virtual values to prevent host information disclosure.
Configure via `BashBuilder`:

```rust
Bash::builder()
    .username("deploy")      // Sets whoami, id, and $USER
    .hostname("my-server")   // Sets hostname, uname -n
    .build();
```

#### Directory Listing and Search
- `ls` - List directory contents (`-l`, `-a`, `-h`, `-1`, `-R`, `-t`)
- `find` - Search for files (`-name PATTERN`, `-type f|d|l`, `-maxdepth N`, `-mindepth N`, `-print`)
- `rmdir` - Remove empty directories (`-p` for parents)

#### File Inspection
- `less` - View file contents (virtual mode: behaves like `cat`, no interactive paging)
- `file` - Detect file type via magic bytes (text, binary, PNG, JPEG, gzip, etc.)
- `stat` - Display file metadata (`-c FORMAT` with %n, %s, %F, %a, %U, %G, %Y, %Z)

#### Archive Operations
- `tar` - Create/extract tar archives (`-c`, `-x`, `-t`, `-v`, `-f`, `-z` for gzip)
- `gzip` - Compress files (`-d` decompress, `-k` keep, `-f` force)
- `gunzip` - Decompress files (`-k` keep, `-f` force)

#### Environment
- `env` - Print environment or run command with modified environment
- `printenv` - Print environment variable values
- `history` - Command history (virtual mode: limited, no persistent history)

#### Prefix Environment Assignments

Bash supports `VAR=value command` syntax where the assignment is temporary and
scoped to the command's environment. Bashkit implements this: prefix assignments
are injected into `ctx.env` for the command's duration, then both `env` and
`variables` are restored. Assignment-only commands (`VAR=value` with no command)
persist in shell variables as usual.

#### Pipeline Control
- `xargs` - Build commands from stdin (`-I REPL`, `-n MAX`, `-d DELIM`)
- `tee` - Write to files and stdout (`-a` append)
- `watch` - Execute command periodically (virtual mode: shows command info, no continuous execution)

#### Network
- `curl` - HTTP client (requires http_client feature + allowlist)
  - Options: `-s/--silent`, `-o FILE`, `-X METHOD`, `-d DATA` (supports `@-` for stdin, `@file` for VFS file), `-H HEADER`, `-I/--head`, `-f/--fail`, `-L/--location`, `-w FORMAT`, `--compressed`, `-u/--user`, `-A/--user-agent`, `-e/--referer`, `-v/--verbose`, `-m/--max-time`, `--connect-timeout`
  - Security: URL allowlist enforced, 10MB response limit, timeouts clamped to [1s, 10min], zip bomb protection via size-limited decompression
- `wget` - Download files (requires http_client feature + allowlist)
  - Options: `-q/--quiet`, `-O FILE`, `--spider`, `--header`, `-U/--user-agent`, `--post-data`, `-t/--tries`, `-T/--timeout`, `--connect-timeout`
  - Security: URL allowlist enforced, 10MB response limit, timeouts clamped to [1s, 10min]
- `http` - HTTPie-style HTTP client (requires http_client feature + allowlist)
  - Syntax: `http [OPTIONS] [METHOD] URL [ITEMS...]` where items are `key=value` (JSON string), `key:=value` (JSON raw), `Header:value`, `key==value` (query param)
  - Options: `--json/-j`, `--form/-f`, `-v/--verbose`, `-h/--headers`, `-b/--body`, `-o FILE`
  - Security: URL allowlist enforced, JSON/form injection prevention, query parameter encoding

**Request Signing**: When the `bot-auth` feature is enabled and configured, all outbound HTTP requests from curl, wget, and http builtins are transparently signed with Ed25519 per RFC 9421. See `specs/017-request-signing.md`.

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

    /// Return an execution plan for sub-command execution.
    /// Default: Ok(None) — normal execute() is used.
    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        Ok(None)
    }
}

pub struct Context<'a> {
    pub args: &'a [String],
    pub env: &'a HashMap<String, String>,
    pub variables: &'a mut HashMap<String, String>,
    pub cwd: &'a mut PathBuf,
    pub fs: Arc<dyn FileSystem>,
    pub stdin: Option<&'a str>,
    #[cfg(feature = "http_client")]
    pub http_client: Option<&'a HttpClient>,
    #[cfg(feature = "git")]
    pub git_client: Option<&'a GitClient>,
    /// Internal builtins only — None for custom builtins.
    pub(crate) shell: Option<ShellRef<'a>>,
}
```

### Shell State Access (ShellRef)

Internal builtins that need interpreter state receive it via `Context.shell`:

```rust
pub(crate) struct ShellRef<'a> {
    // Direct mutable access (simple HashMap state, no invariants)
    pub(crate) aliases: &'a mut HashMap<String, String>,
    pub(crate) traps: &'a mut HashMap<String, String>,
    // Read-only introspection (accessed via methods)
    // has_builtin(), has_function(), is_keyword(),
    // call_stack_depth(), call_stack_frame_name(),
    // history_entries(), jobs()
}
```

**Design rationale:**
- **Direct mutation** for aliases/traps — simple HashMaps with no invariants
- **Side effects** for arrays (budget checks), positional params (call stack),
  history (VFS persistence) — state with invariants the interpreter must enforce
- **Read-only methods** for introspection (functions, builtins, keywords,
  call stack, history, jobs) — builtins shouldn't mutate these
- `pub(crate)` keeps ShellRef out of the public API; custom builtins get `None`
- No dynamic dispatch — concrete struct, not trait

**Builtins using ShellRef:**
- `type`, `which` — read-only: check builtin/function/keyword names
- `hash` — no-op (no PATH cache in sandbox)
- `alias`, `unalias` — direct mutation of `shell.aliases`
- `trap` — direct mutation of `shell.traps`
- `caller` — read call stack depth/frame names
- `history` — read history entries, clear via `ClearHistory` side effect
- `wait` — read job table, set exit code via `SetLastExitCode` side effect
- `mapfile`/`readarray` — set arrays via `SetIndexedArray` side effect

**Builtins still in interpreter dispatch chain** (fundamentally need interpreter):
- `exec` — redirect management, VFS I/O
- `local` — call frame locals mutation
- `source`/`.`, `eval` — parse and execute in current context
- `bash`/`sh` — script execution
- `command` — dispatch to builtins/functions
- `declare`/`typeset` — arrays, assoc arrays, variable attributes
- `unset` — functions, arrays, namerefs, call stack locals
- `let` — arithmetic evaluation with assignment
- `getopts` — complex variable + call stack interaction

### Execution Plans (Sub-Command Delegation)

Builtins cannot access the interpreter directly. When a builtin needs to run
other commands (e.g. `timeout`, `xargs`, `find -exec`), it returns a declarative
`ExecutionPlan` from `execution_plan()`. The interpreter checks this method
before `execute()` — when it returns `Some(plan)`, the interpreter fulfills the
plan instead of using the `execute()` result.

```rust
pub struct SubCommand {
    pub name: String,
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

pub enum ExecutionPlan {
    /// Run a single command with a timeout.
    Timeout {
        duration: Duration,
        preserve_status: bool,
        command: SubCommand,
    },
    /// Run a sequence of commands, collecting output.
    Batch {
        commands: Vec<SubCommand>,
    },
}
```

**Current users:**
- `timeout` → `ExecutionPlan::Timeout` — wraps a sub-command with a time limit
- `xargs` → `ExecutionPlan::Batch` — builds commands from stdin lines
- `find -exec` → `ExecutionPlan::Batch` — runs commands on matched files

**Adding new execution plans:** Add a variant to `ExecutionPlan` and handle it
in the interpreter's plan fulfillment code (`interpreter/mod.rs`). Custom
builtins can also override `execution_plan()` to request sub-command execution.

### Adding Internal Builtins

Simple builtins (zero-arg unit structs) are registered via the `register_builtins!`
macro in `interpreter/mod.rs`. To add a new one:

1. Create the builtin module in `crates/bashkit/src/builtins/` (implement `Builtin` trait)
2. Add `mod mycommand;` and `pub use mycommand::MyCommand;` in `builtins/mod.rs`
3. Add one line to the `register_builtins!` table i