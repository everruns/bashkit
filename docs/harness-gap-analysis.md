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

## Status: All 98 harness patterns pass

All 14 issues filed during this analysis have been resolved on main.

### Issues filed and fixed

| Issue | Title | Category |
|-------|-------|----------|
| #791 | Pipe stdin to VFS script execution | Feature |
| #792 | Subprocess isolation for VFS script-by-path | Feature |
| #793 | Implement `set -a` (allexport) | Feature |
| #794 | `exec` with command argument — execute and don't return | Feature |
| #803 | Single-quoted strings inside `$(...)` lose double quotes | Bug |
| #804 | Nameref `+=` append to indexed array doesn't work | Bug |
| #805 | `export -p` produces no output | Bug |
| #806 | EXIT trap in `$(...)` — output escapes to parent stdout | Bug |
| #833 | `sort -n` doesn't extract leading numeric prefix from strings | Bug |
| #834 | Nameref expansion fails under `set -u` (nounset) | Bug |
| #846 | `${!ref[@]}` key enumeration empty via nameref to assoc array | Bug |
| #847 | `${var%$'\n'}` doesn't match newline in suffix removal pattern | Bug |
| #861 | Assoc array subscripts evaluated as arithmetic instead of literal strings | Bug |
| #862 | `$'\n'` not expanded when concatenated in function argument position | Bug |

### Validation pending

| Issue | Title |
|-------|-------|
| #801 | `local -n` nameref with associative arrays — extended harness patterns |

---

## Test Results (98 patterns, latest main)

**98 pass, 0 fail.**

### Feature tests (10/10)

| Feature | Test |
|---------|------|
| Stdin pipe | `echo data \| ./script.sh`, `read -r`, multi-stage jq pipeline |
| Subprocess isolation | child only sees exports, no side effects on parent |
| `set -a` | `set -a; source .env; set +a` exports, `set +a` stops |
| `exec` | runs target, stops execution, propagates exit code |

### Bash syntax tests (64/64)

| Category | Tests |
|----------|-------|
| Shell options | `set -euo pipefail` |
| Associative arrays | `declare -A`, `${!map[@]}`, key assignment, subscript as literal string |
| Indexed arrays | `+=`, `${#arr[@]}`, `${arr[*]}` |
| Parameter expansion | `:-`, `:+`, `:?`, `%`, `%%`, `#`, `##`, `/`, `//`, `%$'\n'` |
| Control flow | `case`, C-style `for`, reverse `for`, `while read` |
| Quoting | Single-quoted here-doc, here-string, ANSI-C `$'\n'`, concat in args |
| Arithmetic | `10#` base prefix, ternary `?:` |
| Regex | `[[ =~ ]]` with `BASH_REMATCH` |
| Boolean idiom | `${in_fm}` as command (true/false) |
| String ops | `+=` concat with `$'\n'`, `printf '%.0s'` repeat, trim trailing newline |
| Glob in `[[ ]]` | `[[ " $list " == *" $name "* ]]` |
| Functions | `local` scoping, return values |
| Process sub | `mapfile -t < <(cmd)`, `while read < <(cmd)` |
| Date | `date -Iseconds` ISO format |
| JSON (jq) | `-r`, `--argjson`, `-n --arg`, array build, `length` |
| Text tools | `sed -n s///p`, `awk` frontmatter, `nl -ba`, `sort -n` |
| File tools | `basename`, `ls -1`, `mkdir -p`, `mktemp`, `wc -c`, `grep -cF` |
| Misc | `readonly`, `command -v`, `export -p`, `trap EXIT`, brace groups |
| Namerefs | read/write, assoc read/assign, dual namerefs, `+=` append, key enum |
| Nested cmd sub | `$(basename "$(dirname ...)")` |

### Additional tests (24/24)

wc -l, sed, ls+sort, grep, wc -c, mktemp, nested cmd sub, awk, `${var/pat/rep}`,
nameref read/write/assoc/dual, trap EXIT in subshell, command -v, export -p,
arithmetic ternary, `${var:?}`, nl, single-quoted JSON in `$()`, jq via var.

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

**All 98 harness compatibility patterns pass on latest main.** Over 5 rounds
of testing, 14 issues were filed (4 features, 10 bugs) and all were resolved
upstream. Bashkit now supports every bash feature required by the harness
agent framework.

Remaining step: configure HTTP allowlist and TTY detection, then run the
actual harness codebase end-to-end on the VFS.
