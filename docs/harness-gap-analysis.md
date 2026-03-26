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

### Bugs (3 remaining)

| Issue | Title | Severity |
|-------|-------|----------|
| #846 | `${!ref[@]}` key enumeration empty via nameref to assoc array | Critical |
| #847 | `${var%$'\n'}` doesn't match newline in suffix removal pattern | Medium |
| #806 | EXIT trap in `$(...)` — output escapes to parent stdout | Low |

### Validation needed

| Issue | Title | Status |
|-------|-------|--------|
| #801 | `local -n` nameref with associative arrays — harness patterns | Needs testing |

### Fixed (closed on latest main)

| Issue | Title |
|-------|-------|
| #791 | ~~Pipe stdin to VFS script execution~~ |
| #792 | ~~Subprocess isolation for VFS script-by-path~~ |
| #793 | ~~Implement `set -a` (allexport)~~ |
| #794 | ~~`exec` with command argument — execute and don't return~~ |
| #803 | ~~Single-quoted strings inside `$(...)` lose double quotes~~ |
| #804 | ~~Nameref `+=` append to indexed array doesn't work~~ |
| #805 | ~~`export -p` produces no output~~ |
| #833 | ~~`sort -n` doesn't extract leading numeric prefix from strings~~ |
| #834 | ~~Nameref expansion fails under `set -u` (nounset)~~ |

---

## Test Results (74 patterns, latest main)

**71 pass, 3 fail** across bash syntax tests and feature verification.

### Feature tests (10/10 pass)

| Feature | Test | Status |
|---------|------|--------|
| #791 stdin pipe | `echo data \| ./script.sh` | Pass |
| #791 read stdin | `echo data \| ./reader.sh` (uses `read -r`) | Pass |
| #791 multi-stage | `echo {} \| ./a.sh \| ./b.sh` (jq pipeline) | Pass |
| #792 isolation | child doesn't see parent's non-exported vars | Pass |
| #792 no side effects | child's variable changes don't affect parent | Pass |
| #793 set -a | `set -a; source .env; set +a` exports vars | Pass |
| #793 set +a | variables after `set +a` are not exported | Pass |
| #794 exec runs | `exec ./target.sh` runs the target script | Pass |
| #794 exec stops | statements after `exec` are not reached | Pass |
| #794 exec exit code | exit code propagated from exec'd script | Pass |

### Bash syntax tests (61/64 pass)

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
| Text tools | `sed -n s///p`, `awk` frontmatter, `nl -ba`, `sort -n` | Pass |
| File tools | `basename`, `ls -1`, `mkdir -p`, `mktemp`, `wc -c` | Pass |
| Misc | `readonly`, `command -v`, `export -p`, `trap EXIT`, brace groups | Pass |
| Namerefs | Basic read/write, assoc array read/assign, dual namerefs | Pass |
| Nested cmd sub | `$(basename "$(dirname ...)")` | Pass |

### Failing (3/64)

| Test | Issue |
|------|-------|
| `${!ref[@]}` key enumeration through nameref | #846 |
| `${var%$'\n'}` suffix removal with ANSI-C pattern | #847 |
| EXIT trap output in `$(...)` escapes to parent | #806 |

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

After three rounds of rebasing on latest main, **71 of 74 harness patterns
pass**. The 4 critical feature gaps (stdin piping, subprocess isolation,
`set -a`, `exec` with command) and 4 bugs discovered in earlier runs have
all been fixed upstream.

**3 remaining bugs** to fix before harness can fully run:

1. **`${!ref[@]}` through nameref** (#846) — critical for tool/hook discovery
2. **`${var%$'\n'}`** (#847) — medium, used for message body trimming
3. **Trap in `$(...)`** (#806) — low priority, cleanup traps only
