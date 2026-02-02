# BashKit Compatibility Reference

> Dense reference for all bash features and builtins

**See also:**
- [API Documentation](https://docs.rs/bashkit) - Full API reference
- [Custom Builtins Guide](./custom_builtins.md) - Extending BashKit with custom commands

## Quick Status

| Category | Implemented | Planned | Total |
|----------|-------------|---------|-------|
| Shell Builtins | 44 | 4 | 48 |
| Text Processing | 9 | 3 | 12 |
| File Operations | 7 | 0 | 7 |
| Network | 2 | 0 | 2 |

---

## Builtins Reference

### Implemented

| Builtin | Flags/Features | Notes |
|---------|----------------|-------|
| `echo` | `-n`, `-e`, `-E` | Basic escape sequences |
| `printf` | `%s`, `%d`, `%x`, `%o`, `%f` | Format specifiers |
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
| `source` | `file` | Source script |
| `.` | `file` | Alias for source |
| `break` | `[N]` | Break from loop |
| `continue` | `[N]` | Continue loop |
| `return` | `[N]` | Return from function |
| `grep` | `-i`, `-v`, `-c`, `-n`, `-E`, `-q` | Pattern matching |
| `sed` | `s/pat/repl/[g]`, `d`, `p` | Stream editing |
| `awk` | `'{print}'`, `-F`, variables | Text processing |
| `jq` | `.field`, `.[n]`, pipes | JSON processing |
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
| `chmod` | `MODE` | Change permissions (octal) |
| `wc` | `-l`, `-w`, `-c` | Count lines/words/bytes |
| `sort` | `-r`, `-n`, `-u` | Sort lines |
| `uniq` | `-c`, `-d`, `-u` | Filter duplicate lines |
| `cut` | `-d DELIM`, `-f FIELDS` | Extract fields |
| `tr` | `-d`, character ranges | Translate/delete chars |
| `date` | `+FORMAT`, `-u` | Display/format date |
| `wait` | `[JOB_ID...]` | Wait for background jobs |
| `curl` | `-s`, `-o`, `-X`, `-d`, `-H` | HTTP client (stub) |
| `wget` | `-q`, `-O` | Download files (stub) |
| `timeout` | `DURATION COMMAND` | Run with time limit (stub) |

### Not Implemented

| Builtin | Priority | Status |
|---------|----------|--------|
| `xargs` | Low | Planned |
| `find` | Low | Planned |
| `diff` | Low | Planned |
| `tee` | Low | - |
| `ls` | Low | - |
| `ln` | Low | - |
| `chown` | Low | - |
| `kill` | Low | - |
| `eval` | Low | - |
| `exec` | Low | - |
| `type` | Low | - |
| `which` | Low | - |
| `command` | Low | - |
| `hash` | Low | - |
| `declare` | Low | - |
| `typeset` | Low | - |
| `readonly` | Low | - |
| `getopts` | Low | - |
| `trap` | High | - |

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
| `$!` | ❌ | Last background PID |
| `$$` | ✅ | Current PID |
| `$-` | ❌ | Current options |
| `$_` | ❌ | Last argument |
| `$RANDOM` | ✅ | Random number (0-32767) |
| `$LINENO` | ✅ | Current line number (placeholder) |

---

## Arrays

| Feature | Status | Example |
|---------|--------|---------|
| Declaration | ✅ | `arr=(a b c)` |
| Index access | ✅ | `${arr[0]}` |
| All elements | ✅ | `${arr[@]}` |
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
| Real filesystem | ❌ | Sandboxed by default |
| Symlinks | ✅ | Stored but not followed |
| Permissions | ✅ | Metadata stored, not enforced |

---

## Network

| Feature | Status | Notes |
|---------|--------|-------|
| HTTP client | ✅ | Infrastructure exists |
| URL allowlist | ✅ | Whitelist-based security |
| `curl` builtin | ❌ | Planned |
| `wget` builtin | ❌ | Planned |
| Raw sockets | ❌ | Not planned |

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
- [x] `basename`/`dirname` builtins
- [x] `date` builtin
- [x] Background execution (`&`, `wait`) - parsed, runs synchronously
- [x] Network (`curl`, `wget`) - stub, requires allowlist config
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

### Planned
- [ ] `trap` signal handling

### Not Planned
- Interactive features (history, job control UI)
- Process spawning (sandboxed environment)
- Raw filesystem access

---

## See Also

- [KNOWN_LIMITATIONS.md](../KNOWN_LIMITATIONS.md) - Detailed gap analysis
- [specs/](../specs/) - Design specifications
