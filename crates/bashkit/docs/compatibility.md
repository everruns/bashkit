# Bashkit Compatibility Scorecard

> Feature parity tracking for bash and common tools

**See also:**
- [API Documentation](https://docs.rs/bashkit) - Full API reference
- [Custom Builtins Guide](./custom_builtins.md) - Extending Bashkit with custom commands
- [Threat Model](./threat-model.md) - Security considerations

**Legend:** ✅ Implemented | ⚠️ Partial | ❌ Not implemented | N/A Security exclusion

## POSIX Shell Compliance

Bashkit provides substantial compliance with IEEE Std 1003.1-2024 (POSIX.1-2024)
Shell Command Language. See [specs/008-posix-compliance.md](../specs/008-posix-compliance.md)
for detailed compliance status.

| POSIX Category | Status |
|----------------|--------|
| Reserved Words (16) | Full compliance |
| Special Parameters (8) | Full compliance |
| Special Built-ins (15) | 13/15 implemented |
| Word Expansions | Substantial compliance |
| Redirections | Full compliance |
| Compound Commands | Full compliance |

**Security Exclusions**: `exec` and `trap` are intentionally not implemented
for sandbox security reasons. See the compliance spec for details.

## Quick Status

| Category | Implemented | Planned | Total |
|----------|-------------|---------|-------|
| Shell Builtins | 85 | 0 | 85 |
| Text Processing | 14 | 0 | 14 |
| File Operations | 10 | 0 | 10 |
| Network | 2 | 0 | 2 |

---

## Builtins Reference

### Implemented

| Builtin | Flags/Features | Notes |
|---------|----------------|-------|
| `echo` | `-n`, `-e`, `-E` | Basic escape sequences |
| `printf` | `%s`, `%d`, `%x`, `%o`, `%f` | Format specifiers, repeats format for multiple args |
| `cat` | (none) | Concatenate files/stdin |
| `true` | - | Exit 0 |
| `false` | - | Exit 1 |
| `exit` | `[N]` | Exit with code |
| `cd` | `[dir]` | Change directory |
| `pwd` | - | Print working directory |
| `test` | `-f`, `-d`, `-e`, `-z`, `-n`, `-eq`, `-ne`, `-lt`, `-gt`, `-le`, `-ge` | Conditionals |
| `[` | (same as test) | Alias for test |
| `export` | `VAR=value` | Export variables |
| `read` | `VAR` | Read line into variable |
| `set` | `-e`, `+e`, positional | Set options and positional params |
| `unset` | `VAR` | Unset variable |
| `shift` | `[N]` | Shift positional params |
| `local` | `VAR=value` | Local variables |
| `source` | `file [args]` | Source script; loads functions/variables, PATH search, positional params |
| `.` | `file [args]` | Alias for source |
| `/path/to/script.sh` | `[args]` | Execute script by absolute/relative path (shebang stripped, call frame) |
| `$PATH` search | `cmd [args]` | Search `$PATH` dirs for executable scripts (after builtins) |
| `break` | `[N]` | Break from loop |
| `continue` | `[N]` | Continue loop |
| `return` | `[N]` | Return from function |
| `:` | - | POSIX null utility (no-op) |
| `eval` | `command...` | POSIX construct and execute command |
| `readonly` | `VAR[=value]`, `-p` | POSIX mark variable read-only |
| `times` | - | POSIX display process times |
| `grep` | `-i`, `-v`, `-c`, `-n`, `-E`, `-q` | Pattern matching |
| `sed` | `s///[g]`, `d`, `p`, `q`, `a`, `i`, `c`, `h/H/g/G/x`, `-E`, `-n`, `!` | Stream editing |
| `awk` | `'{print}'`, `-F`, `-v`, loops, arrays, increment, ternary | Text processing |
| `jq` | `.field`, `.[n]`, pipes, file args, `-r`, `-c`, `-n`, `-s`, `-S`, `-e`, `-j`, `--tab`, `--arg`, `--argjson`, `-V`, combined flags | JSON processing |
| `sleep` | `N`, `N.N` | Pause execution (max 60s) |
| `head` | `-n N`, `-N` | First N lines (default 10) |
| `tail` | `-n N`, `-N` | Last N lines (default 10) |
| `basename` | `NAME [SUFFIX]` | Strip directory from path |
| `dirname` | `NAME` | Strip last path component |
| `mkdir` | `-p` | Create directories |
| `rm` | `-rf` | Remove files/directories |
| `cp` | `-r` | Copy files |
| `mv` | - | Move/rename files |
| `touch` | - | Create empty files |
| `chmod` | `MODE` | Change permissions (octal only) |
| `wc` | `-l`, `-w`, `-c` | Count lines/words/bytes |
| `sort` | `-r`, `-n`, `-u` | Sort lines |
| `uniq` | `-c`, `-d`, `-u` | Filter duplicate lines |
| `cut` | `-d DELIM`, `-f FIELDS` | Extract fields |
| `tr` | `-d`, character ranges | Translate/delete chars |
| `date` | `+FORMAT`, `-u`, `-d`/`--date` (relative, compound, epoch) | Display/format date |
| `wait` | `[JOB_ID...]` | Wait for background jobs |
| `curl` | `-s`, `-o`, `-X`, `-d`, `-H`, `-I`, `-f`, `-L`, `-w`, `--compressed`, `-u`, `-A`, `-e`, `-v`, `-m` | HTTP client (requires http_client feature) |
| `wget` | `-q`, `-O`, `--spider`, `--header`, `-U`, `--post-data`, `-t` | Download files (requires http_client feature) |
| `timeout` | `DURATION COMMAND` | Run with time limit (stub) |
| `ls` | `-l`, `-a`, `-h`, `-1`, `-R` | List directory contents |
| `find` | `-name`, `-type`, `-maxdepth`, `-print` | Search for files |
| `rmdir` | `-p` | Remove empty directories |
| `xargs` | `-I`, `-n`, `-d` | Build commands from stdin |
| `tee` | `-a` | Write to files and stdout |
| `watch` | `INTERVAL COMMAND` | Execute periodically (virtual mode) |
| `file` | (none) | Detect file type via magic bytes |
| `less` | (none) | View file (behaves like cat in virtual mode) |
| `stat` | `-c FORMAT` | Display file metadata |
| `tar` | `-c`, `-x`, `-t`, `-v`, `-f`, `-z` | Archive operations |
| `gzip` | `-d`, `-k`, `-f` | Compress files |
| `gunzip` | `-k`, `-f` | Decompress files |
| `env` | `[VAR=val]` | Print/modify environment |
| `printenv` | `[VAR]` | Print environment variables |
| `history` | (none) | Command history (limited in virtual mode) |
| `hostname` | (none) | Display virtual hostname |
| `uname` | `-a`, `-s`, `-n`, `-r`, `-v`, `-m`, `-o` | System info |
| `whoami` | (none) | Display virtual username |
| `id` | `-u`, `-g`, `-n` | User/group IDs |
| `nl` | `-b`, `-n`, `-s`, `-i`, `-v`, `-w` | Number lines of files |
| `paste` | `-d`, `-s` | Merge lines of files |
| `column` | `-t`, `-s`, `-o` | Columnate lists |
| `comm` | `-1`, `-2`, `-3` | Compare two sorted files |
| `diff` | `-u`, `-q`/`--brief` | Compare files line by line |
| `strings` | `-n`, `-t`, `-a` | Find printable strings in binary data |
| `od` | `-A`, `-t`, `-N`, `-j` | Octal/hex dump |
| `xxd` | `-l`, `-s`, `-c`, `-g`, `-p` | Hex dump |
| `hexdump` | `-C`, `-n`, `-s` | Display file in hex+ASCII |

### Not Implemented

| Builtin | Priority | Status |
|---------|----------|--------|
| `ln` | Low | - |
| `chown` | Low | - |
| `kill` | Low | - |
| `exec` | N/A | Security: intentionally excluded |
| `trap` | N/A | Security: intentionally excluded |
| `type` | Low | - |
| `which` | Low | - |
| `command` | Medium | POSIX utility |
| `hash` | Low | - |
| `declare` | Low | Bash extension |
| `typeset` | Low | Bash extension |
| `getopts` | Medium | POSIX utility |

---

## Shell Syntax

### Operators

| Operator | Status | Example | Notes |
|----------|--------|---------|-------|
| `\|` | ✅ | `cmd1 \| cmd2` | Pipeline |
| `&&` | ✅ | `cmd1 && cmd2` | AND list |
| `\|\|` | ✅ | `cmd1 \|\| cmd2` | OR list |
| `;` | ✅ | `cmd1; cmd2` | Sequential |
| `&` | ⚠️ | `cmd &` | Parsed, async pending |
| `!` | ✅ | `! cmd` | Negate exit code |

### Redirections

| Redirect | Status | Example | Notes |
|----------|--------|---------|-------|
| `>` | ✅ | `cmd > file` | Output to file |
| `>>` | ✅ | `cmd >> file` | Append to file |
| `<` | ✅ | `cmd < file` | Input from file |
| `<<<` | ✅ | `cmd <<< "string"` | Here-string |
| `<<EOF` | ✅ | Heredoc | Multi-line input |
| `2>` | ✅ | `cmd 2> file` | Stderr redirect |
| `2>&1` | ✅ | `cmd 2>&1` | Stderr to stdout |
| `&>` | ✅ | `cmd &> file` | Both to file |

### Control Flow

| Feature | Status | Example |
|---------|--------|---------|
| `if/elif/else/fi` | ✅ | `if cmd; then ...; fi` |
| `for/do/done` | ✅ | `for i in a b c; do ...; done` |
| `while/do/done` | ✅ | `while cmd; do ...; done` |
| `until/do/done` | ✅ | `until cmd; do ...; done` |
| `case/esac` | ✅ | `case $x in pat) ...;; esac` |
| `{ ... }` | ✅ | Brace group |
| `( ... )` | ✅ | Subshell |
| `function name { }` | ✅ | Function definition |
| `name() { }` | ✅ | Function definition |

---

## Expansions

### Variable Expansion

| Syntax | Status | Example | Description |
|--------|--------|---------|-------------|
| `$var` | ✅ | `$HOME` | Simple expansion |
| `${var}` | ✅ | `${HOME}` | Braced expansion |
| `${var:-default}` | ✅ | `${X:-fallback}` | Use default if unset/empty |
| `${var:=default}` | ✅ | `${X:=value}` | Assign default if unset/empty |
| `${var:+alt}` | ✅ | `${X:+yes}` | Use alt if set |
| `${var:?error}` | ✅ | `${X:?missing}` | Error if unset/empty |
| `${#var}` | ✅ | `${#str}` | Length of value |
| `${var#pat}` | ✅ | `${f#*.}` | Remove shortest prefix |
| `${var##pat}` | ✅ | `${f##*/}` | Remove longest prefix |
| `${var%pat}` | ✅ | `${f%.*}` | Remove shortest suffix |
| `${var%%pat}` | ✅ | `${f%%/*}` | Remove longest suffix |
| `${var/pat/repl}` | ❌ | - | Substitute (not impl) |
| `${var^}` | ❌ | - | Uppercase first |
| `${var,}` | ❌ | - | Lowercase first |

### Prefix Environment Assignments

| Syntax | Status | Example | Description |
|--------|--------|---------|-------------|
| `VAR=val cmd` | ✅ | `TOKEN=abc printenv TOKEN` | Temporary env for command |
| Multiple prefix | ✅ | `A=1 B=2 cmd` | Multiple vars in one command |
| No persist | ✅ | `X=1 cmd; echo $X` | Var not set after command |
| Assignment-only | ✅ | `X=1` (no cmd) | Persists in shell variables |

### Command Substitution

| Syntax | Status | Example |
|--------|--------|---------|
| `$(cmd)` | ✅ | `x=$(pwd)` |
| `` `cmd` `` | ❌ | Backticks (deprecated) |

### Arithmetic

| Syntax | Status | Example |
|--------|--------|---------|
| `$((expr))` | ✅ | `$((1+2))` |
| `+`, `-`, `*`, `/`, `%` | ✅ | Basic ops |
| `==`, `!=`, `<`, `>`, `<=`, `>=` | ✅ | Comparisons |
| `&`, `\|` | ✅ | Bitwise |
| `&&`, `\|\|` | ✅ | Logical operators |
| `? :` | ✅ | Ternary |
| `=`, `+=`, etc. | ❌ | Assignment (not impl) |

### Other Expansions

| Syntax | Status | Example | Description |
|--------|--------|---------|-------------|
| `*`, `?` | ✅ | `*.txt` | Glob patterns |
| `[abc]` | ❌ | `[0-9]` | Bracket globs |
| `{a,b,c}` | ✅ | `{1..5}` | Brace expansion |
| `~` | ✅ | `~/file` | Tilde expansion |
| `<(cmd)` | ✅ | `diff <(a) <(b)` | Process substitution |

---

## Special Variables

| Variable | Status | Description |
|----------|--------|-------------|
| `$?` | ✅ | Last exit code |
| `$#` | ✅ | Number of positional params |
| `$@` | ✅ | All positional params (separate) |
| `$*` | ✅ | All positional params (joined) |
| `$0` | ✅ | Script/function name |
| `$1`-`$9` | ✅ | Positional parameters |
| `$!` | ✅ | Last background job ID (POSIX) |
| `$$` | ✅ | Current PID |
| `$-` | ✅ | Current option flags (POSIX) |
| `$_` | ❌ | Last argument |
| `$RANDOM` | ✅ | Random number (0-32767) |
| `$LINENO` | ✅ | Current line number |

---

## Arrays

| Feature | Status | Example |
|---------|--------|---------|
| Declaration | ✅ | `arr=(a b c)` |
| Index access | ✅ | `${arr[0]}` |
| All elements `@` | ✅ | `${arr[@]}` (separate args) |
| All elements `*` | ✅ | `${arr[*]}` (single arg when quoted) |
| Array length | ✅ | `${#arr[@]}` |
| Element length | ✅ | `${#arr[0]}` |
| Append | ✅ | `arr+=(d e)` |
| Slice | ❌ | `${arr[@]:1:2}` |
| Indices | ✅ | `${!arr[@]}` |
| Associative | ❌ | `declare -A` |

---

## Test Operators

### File Tests

| Operator | Status | Description |
|----------|--------|-------------|
| `-e file` | ✅ | Exists |
| `-f file` | ✅ | Is regular file |
| `-d file` | ✅ | Is directory |
| `-s file` | ✅ | Size > 0 |
| `-r file` | ✅ | Is readable (exists in virtual fs) |
| `-w file` | ✅ | Is writable (exists in virtual fs) |
| `-x file` | ✅ | Is executable (mode & 0o111) |
| `-L file` | ✅ | Is symlink |

### String Tests

| Operator | Status | Description |
|----------|--------|-------------|
| `-z str` | ✅ | Is empty |
| `-n str` | ✅ | Is non-empty |
| `str1 = str2` | ✅ | Equal |
| `str1 != str2` | ✅ | Not equal |
| `str1 < str2` | ✅ | Less than |
| `str1 > str2` | ✅ | Greater than |

### Numeric Tests

| Operator | Status | Description |
|----------|--------|-------------|
| `-eq` | ✅ | Equal |
| `-ne` | ✅ | Not equal |
| `-lt` | ✅ | Less than |
| `-gt` | ✅ | Greater than |
| `-le` | ✅ | Less or equal |
| `-ge` | ✅ | Greater or equal |

---

## Resource Limits

Default limits (configurable):

| Resource | Default | Notes |
|----------|---------|-------|
| Commands | 10,000 | Per execution |
| Loop iterations | 100,000 | Per loop |
| Function depth | 100 | Recursion limit |
| Output size | 10MB | Total stdout |
| Parser timeout | 5s | Prevents infinite parse |
| Parser operations | 100,000 | Fuel-based limit |
| Input size | 10MB | Max script size |
| AST depth | 100 | Nesting limit |

---

## Filesystem

| Feature | Status | Notes |
|---------|--------|-------|
| Virtual filesystem | ✅ | InMemoryFs, OverlayFs, MountableFs |
| Real filesystem | ❌ | Virtual by default |
| Symlinks | ✅ | Stored but not followed |
| Permissions | ✅ | Metadata stored, not enforced |
| `/dev/null` | ✅ | Interpreter-level handling (cannot be bypassed) |

---

## Network

| Feature | Status | Notes |
|---------|--------|-------|
| HTTP client | ✅ | Full implementation with security mitigations |
| URL allowlist | ✅ | Default-deny whitelist security model |
| `curl` builtin | ✅ | Full HTTP client with `-s`, `-o`, `-X`, `-d`, `-H`, `-I`, `-f`, `-L`, `-w`, `--compressed`, `-u`, `-A`, `-e`, `-v`, `-m` |
| `wget` builtin | ✅ | Full downloader with `-q`, `-O`, `--spider`, `--header`, `-U`, `--post-data`, `-t` |
| Response limits | ✅ | 10MB max response size, 30s timeout |
| Redirect security | ✅ | Redirects require explicit `-L` and allowlist check |
| Raw sockets | ❌ | Not planned |

### Network Configuration

```rust
use bashkit::{Bash, NetworkAllowlist};

// Enable network with URL allowlist
let bash = Bash::builder()
    .network(NetworkAllowlist::new()
        .allow("https://api.example.com")
        .allow("https://cdn.example.com/assets"))
    .build();
```

See [specs/006-threat-model.md](../specs/006-threat-model.md) for HTTP security details.

---

## Running Tests

```bash
# All tests
cargo test --all-features

# Spec tests only
cargo test --test spec_tests

# Compare with real bash
cargo test --test spec_tests -- bash_comparison_tests --ignored
```

---

## Roadmap

### Completed
- [x] `sleep` builtin
- [x] `head`/`tail` builtins
- [x] File operation builtins (`mkdir`, `rm`, `cp`, `mv`, `touch`, `chmod`)
- [x] `wc` builtin
- [x] Text processing (`sort`, `uniq`, `cut`, `tr`)
- [x] Text structure (`nl`, `paste`, `column`)
- [x] File comparison (`diff`, `comm`)
- [x] Byte inspection (`strings`, `od`, `xxd`, `hexdump`)
- [x] `basename`/`dirname` builtins
- [x] `date` builtin
- [x] Background execution (`&`, `wait`) - parsed, runs synchronously
- [x] Network (`curl`, `wget`) - full HTTP implementation with security mitigations
- [x] `timeout` builtin - stub, requires interpreter-level integration
- [x] Process substitution (`<(cmd)`, `>(cmd)`)
- [x] Here string edge cases tested
- [x] `set -e` (errexit) - exit on command failure
- [x] Tilde expansion (~) - expands to $HOME
- [x] Special variables ($$, $RANDOM, $LINENO)
- [x] File test operators (-r, -w, -x, -L)
- [x] Stderr redirections (2>, 2>&1, &>)
- [x] Arithmetic logical operators (&&, ||)
- [x] Brace expansion ({a,b,c}, {1..5})
- [x] String comparison operators (< >) in test
- [x] Array indices `${!arr[@]}`
- [x] `/dev/null` support (interpreter-level, cannot be bypassed by custom fs)

### Known LLM Compatibility Gaps

Identified from eval analysis — features frequently used by LLM-generated scripts:

**High Impact (commonly generated by LLMs):**
- [ ] `chmod +x` symbolic mode — LLMs almost always use `chmod +x` not octal
- [ ] `sed` ampersand (`&`) in replacement — very common pattern
- [ ] AWK `printf %x/%o/%c` format specifiers — hex/octal output
- [ ] AWK `match()` and `gensub()` functions — text extraction
- [ ] `sed` `\n` literal newline in replacement — line splitting

**Medium Impact:**
- [ ] AWK power operators (`^`, `**`) — math scripts
- [ ] AWK `exit` statement with code — error handling
- [ ] AWK negation `!$1` — filtering empty fields
- [ ] `sed` grouped commands `{cmd1;cmd2}` — complex transforms
- [ ] `sed` branch/label (`b`/`t`/`:label`) — advanced scripts
- [ ] AWK `ORS` variable — custom output formatting
- [ ] AWK `getline` — multi-file processing

**Low Impact:**
- [ ] `sed` `0~2` step addressing — even/odd line processing
- [ ] `sed` `Q` quiet quit command
- [ ] `sed` `0,/pattern/` first match addressing
- [ ] AWK `$0` modification with field re-splitting

### Not Planned
- Interactive features (history, job control UI)
- Process spawning (virtual environment)
- Raw filesystem access

---

## See Also

- [specs/009-implementation-status.md](../../../specs/009-implementation-status.md) - Detailed implementation status
- [specs/](../../../specs/) - Design specifications
