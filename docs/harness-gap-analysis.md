# Gap Analysis: Running wedow/harness on Bashkit

Analysis of what bashkit features are missing or incomplete for running
[wedow/harness](https://github.com/wedow/harness), a ~500-line bash agent
framework with a plugin-based architecture.

## What is harness?

A minimal agent loop in bash. Core state machine:
`start -> assemble -> send -> receive -> (tool_exec -> tool_done -> assemble) -> done`.

Dependencies: bash 4+, jq, curl.

Architecture: everything is an external executable (tools, hooks, providers,
commands) discovered from `.harness/` directories and plugin packs. Harness
will run inside bashkit against the VFS (no real filesystem needed).

---

## Open Issues

### Feature gaps

| Issue | Title | Severity |
|-------|-------|----------|
| #791 | Pipe stdin to VFS script execution | Critical |
| #792 | Subprocess isolation for VFS script-by-path | Critical |
| #793 | Implement `set -a` (allexport) | Critical |
| #794 | `exec` with command argument — execute and don't return | Critical |
| #801 | `local -n` nameref with associative arrays — harness patterns | Medium |

### Bugs discovered by running harness patterns

| Issue | Title | Severity |
|-------|-------|----------|
| #833 | `sort -n` doesn't extract leading numeric prefix from strings | Critical |
| #834 | Nameref expansion fails under `set -u` (nounset) | Critical |

### Fixed (closed on latest main)

| Issue | Title |
|-------|-------|
| #803 | ~~Single-quoted strings inside `$(...)` lose double quotes~~ |
| #804 | ~~Nameref `+=` append to indexed array doesn't work~~ |
| #805 | ~~`export -p` produces no output~~ |
| #806 | ~~EXIT trap in command substitution subshell doesn't fire~~ |

---

## Test Results (50 patterns, latest main)

**48 pass, 2 fail** on the harness compatibility test suite.

### Passing (48/50)

| Category | Tests | Status |
|----------|-------|--------|
| Shell options | `set -euo pipefail` | Pass |
| Associative arrays | `declare -A`, `${!map[@]}`, key assignment | Pass |
| Indexed arrays | `+=`, `${#arr[@]}`, `${arr[*]}` | Pass |
| Parameter expansion | `:-`, `:+`, `:?`, `%`, `%%`, `#`, `##`, `/`, `//` | Pass |
| Control flow | `case`, C-style `for`, reverse `for`, `while read` | Pass |
| Quoting | Single-quoted here-doc, here-string, ANSI-C `$'\n'` | Pass |
| Arithmetic | `10#` base prefix, ternary `?:` | Pass |
| Regex | `[[ =~ ]]` with `BASH_REMATCH` | Pass |
| Boolean idiom | `${in_fm}` as command (true/false) | Pass |
| String ops | `+=` concat with `$'\n'`, `printf '%.0s'` repeat | Pass |
| Glob in `[[ ]]` | `[[ " $list " == *" $name "* ]]` | Pass |
| Functions | `local` scoping, return values | Pass |
| Process sub | `mapfile -t < <(cmd)`, `while read < <(cmd)` | Pass |
| Date | `date -Iseconds` ISO format | Pass |
| JSON (jq) | `-r`, `--argjson`, `-n --arg`, array build, `length` | Pass |
| Text tools | `sed -n s///p`, `awk` frontmatter, `nl -ba` | Pass |
| File tools | `basename`, `ls -1`, `mkdir -p`, `mktemp`, `wc -c` | Pass |
| Misc | `readonly`, `command -v`, `export -p`, `trap EXIT`, brace groups | Pass |
| Namerefs | Basic read/write, assoc array read/assign, dual namerefs | Pass |
| Nested cmd sub | `$(basename "$(dirname ...)")` | Pass |

### Failing (2/50)

| Test | Issue |
|------|-------|
| `sort -n` with string prefixes (`0003-msg.md`) | #833 |
| `declare -n` under `set -u` | #834 |

---

## Configuration needed (no code changes)

### HTTP allowlist

Bashkit's `curl` supports all flags harness needs (`-s`, `-H`, `-d`,
`--max-time`). Just configure the allowlist:

```rust
NetworkAllowlist::new()
    .allow("https://api.anthropic.com")
    .allow("https://api.openai.com")
```

### TTY detection

Harness uses `[[ ! -t 0 ]]` — bashkit defaults to non-interactive.
Use `BashBuilder::tty()` (added in #830) to configure if needed.

---

## Summary

After rebasing on latest main, **48 of 50 harness bash patterns pass**.
The 4 previously-filed bugs (#803-#806) were all fixed upstream.

Remaining work to run harness on bashkit:

1. **VFS script execution** (#791, #792) — pipe stdin to scripts, subprocess isolation
2. **`exec` with command** (#794) — execute and exit
3. **`set -a`** (#793) — auto-export variables
4. **`sort -n`** (#833) — numeric prefix extraction
5. **Nameref + nounset** (#834) — `set -u` compatibility
6. **Nameref edge cases** (#801) — complex harness patterns
