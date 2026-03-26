# Gap Analysis: Running wedow/harness on Bashkit

Analysis of what bashkit features are missing or incomplete for running
[wedow/harness](https://github.com/wedow/harness), a ~500-line bash agent
framework with a plugin-based architecture.

## What is harness?

A minimal agent loop in bash. Core state machine:
`start -> assemble -> send -> receive -> (tool_exec -> tool_done -> assemble) -> done`.

Dependencies: bash 4+, jq, curl.

Architecture: everything is an external executable (tools, hooks, providers,
commands) discovered from `.harness/` directories and plugin packs.

---

## Critical Blockers

### 1. External process execution

**Status: BLOCKED**

Harness's entire plugin architecture runs external executables: tool plugins,
hook scripts, provider scripts, and CLI commands. Every plugin is a standalone
bash script discovered from disk and invoked as a subprocess.

Bashkit is a sandboxed virtual interpreter -- commands that aren't builtins
produce "command not found". There is no mechanism to spawn arbitrary external
scripts as subprocesses.

**Impact**: The entire plugin system (tools, hooks, providers, commands) cannot
function. This is the fundamental architectural mismatch.

**Examples from harness**:
```bash
# Hook pipeline: chain external scripts
echo "${current}" | "${hook}" 2>>"${HARNESS_LOG}"

# Tool execution
cd "${workdir}" && echo "${input}" | "${tool_bin}" --exec 2>&1

# Provider invocation
echo "${payload}" | "${provider_path}" 2>"${err_file}"

# Command dispatch
exec "${cmd_path}" "$@"
```

### 2. `exec` with command replacement

**Status: NOT IMPLEMENTED**

Harness uses `exec "${cmd_path}" "$@"` to dispatch CLI subcommands, replacing
the current process. Bashkit's `exec` builtin only supports FD redirections --
when given a command argument it returns exit code 127 ("command not found").
This is intentionally excluded for sandbox safety.

**Impact**: Main CLI command dispatch fails entirely.

### 3. `set -a` / `set +a` (allexport)

**Status: NOT IMPLEMENTED**

Harness uses this pattern to load session environment:
```bash
set -a
source "${HARNESS_SESSION}/.env"
set +a
```

Bashkit's `set` builtin stores `SHOPT_a` as a variable when `set -a` is used,
but:
- `option_name_to_var()` doesn't map `"allexport"` at all
- Nothing checks `SHOPT_a` during variable assignment to auto-export

**Impact**: Session environment variables (HARNESS_CWD, etc.) won't propagate
to child commands/hooks.

### 4. Real filesystem access

**Status: REQUIRES CONFIGURATION**

Harness reads/writes session files, creates directories, and walks the
filesystem upward to discover `.harness/` plugin directories. Bashkit uses a
VFS by default. The `RealFs` feature exists but is opt-in and has path
traversal prevention.

**Impact**: Session persistence, plugin discovery from disk, and `.env` file
sourcing need `RealFs` enabled.

### 5. Real HTTP via curl

**Status: REQUIRES CONFIGURATION**

Provider plugins use `curl` with specific headers, JSON payloads, and
timeouts to call Anthropic/OpenAI APIs:
```bash
curl -s "${ANTHROPIC_API_URL}" \
  -H "x-api-key: ${ANTHROPIC_API_KEY}" \
  -H "anthropic-version: ${ANTHROPIC_API_VERSION}" \
  -H "content-type: application/json" \
  --max-time 300 \
  -d "${body}"
```

Bashkit's `curl` builtin requires the `http_client` feature and a URL
allowlist. Each API endpoint must be explicitly allowed.

**Impact**: All provider API calls fail unless allowlist is configured.

---

## Medium-Priority Gaps

### 6. `local -n` (nameref) with associative arrays

**Status: PARTIALLY WORKING**

Harness uses nameref extensively for passing associative arrays by reference:
```bash
_collect_hooks_from() {
  local dir="$1"
  local -n map_ref="$2"
  local -n order_ref="$3"
  # ...
  map_ref["${base}"]="${f}"
  order_ref+=("${base}")
}
```

Bashkit has nameref support but with 14 skipped spec tests. The complex
pattern of assigning through a nameref to an associative array key may not
work correctly.

### 7. `command -v` with `&>/dev/null`

**Status: UNKNOWN**

Harness checks command availability with:
```bash
_require() { command -v "$1" &>/dev/null || _die "…"; }
```

Bashkit has `command` builtin but `command -v` behavior with non-builtins
(checking for executables on PATH) may not match expectations in a sandboxed
environment.

### 8. `readlink -f` for symlink resolution

**Status: IMPLEMENTED (VFS limitation)**

Bashkit implements `readlink -f` but symlink following is intentionally
blocked in the VFS. Harness uses `readlink -f "${BASH_SOURCE[0]}"` for
self-location, which may not resolve correctly.

### 9. `[[ ! -t 0 ]]` (terminal detection)

**Status: LIKELY INCOMPATIBLE**

Harness checks if stdin is a terminal for interactive vs piped input
detection. In bashkit's virtual environment there's no TTY concept, so this
always behaves as non-interactive.

### 10. `trap 'cmd' EXIT` for cleanup

**Status: IMPLEMENTED**

Harness uses EXIT traps for temp file cleanup:
```bash
trap 'rm -f "${err_file}"' EXIT
```

Bashkit implements EXIT trap execution. This should work.

### 11. `env` command with variable overrides

**Status: NEEDS VERIFICATION**

The subagent tool uses:
```bash
env "${env_args[@]+"${env_args[@]}"}" "${HARNESS_ROOT}/bin/harness" run ...
```

Bashkit has an `env` builtin but running an external command through `env`
hits the external process execution blocker.

---

## Likely Working Features

These harness features are supported in bashkit:

| Feature | Status |
|---------|--------|
| `set -euo pipefail` | Working |
| `declare -A` (associative arrays) | Working |
| `${!assoc[@]}` (iterate keys) | Working |
| Indexed arrays, `+=` append | Working |
| `${var:-default}`, `${var:?error}` | Working |
| `${var%pattern}`, `${var%%pattern}`, `${var#prefix}` | Working |
| `case ... esac` with patterns | Working |
| `[[ ]]` conditionals (-d, -f, -x, -z, -n, ==, !=) | Working |
| `[[ =~ ]]` regex with `BASH_REMATCH` | Working |
| C-style `for (( i=0; i<n; i++ ))` loops | Working |
| `while IFS= read -r line` | Working |
| `IFS=':' read -ra array <<< "$str"` | Working |
| `mapfile -t` with process substitution | Basic support |
| `printf '%04d'`, `printf '%s\n'` | Working |
| `$'\n'` ANSI-C quoting | Working |
| Here-documents `<<EOF` and `<<'EOF'` | Working |
| Here-strings `<<<` | Working |
| `< <(cmd)` process substitution | Basic support |
| `date -Iseconds`, `date +FORMAT` | Working |
| `jq` (JSON processing) | Working (120/121 tests) |
| `sed`, `sort`, `basename`, `dirname` | Working |
| `readlink -f` (in VFS context) | Working |
| `source` / `.` | Working |
| `readonly` | Working |
| Functions, `local` variables | Working |
| `$(( 10#${var} + 1 ))` arithmetic | Working |
| `trap 'cmd' EXIT` | Working |
| `cat`, `ls`, `head`, `tail`, `wc` | Working |
| `mkdir -p` | Working |
| `mktemp` | Working |
| `grep -cF` | Working |
| `awk` | Working |
| `seq` | Working |
| Boolean-as-command idiom (`${in_fm}`) | Working |
| `echo -e` (ANSI escapes) | Working |
| `$$` (PID), `$?` (exit code) | Working |
| `BASH_SOURCE[0]` | Working |
| `unset` | Working |
| `shift` | Working |
| `break`, `continue` | Working |

---

## Summary

**Harness cannot run in bashkit today.** The root cause is architectural:
harness orchestrates external executables while bashkit deliberately prevents
external process execution.

### Required changes (ordered by priority)

1. **External process execution mode** -- ability to spawn real subprocesses
   from VFS-resident or real-fs scripts. This is the single biggest gap and
   conflicts with bashkit's core sandbox guarantee.

2. **`exec` with command replacement** -- or a safe equivalent that runs a
   command and exits with its status.

3. **`set -a` / allexport** -- implement the auto-export behavior, not just
   flag storage. Map `"allexport"` in `option_name_to_var()` and check
   `SHOPT_a` on every variable assignment.

4. **Real filesystem mode** -- enable and configure `RealFs` for session
   persistence and plugin discovery.

5. **HTTP allowlist** -- configure for Anthropic/OpenAI API endpoints.

6. **Robust nameref + associative arrays** -- fix remaining edge cases
   with `local -n` and associative array assignment through namerefs.

### Alternative approach

Instead of adding real process execution, harness could be adapted to run
inside bashkit by:
- Converting plugin scripts to bashkit builtins or `source`-able VFS files
- Replacing external command invocation with function calls
- Pre-loading all plugin code into the interpreter

This would preserve bashkit's sandbox guarantees but require significant
harness modifications.
