# Limitations

## Status
Living document (updated as limitations are added/lifted)

## Summary

The negative spec: what Bashkit deliberately does NOT do (and why), plus
known partial implementations. Absences can't be recovered from code, so
they're recorded here; everything positive is generated or tested instead:

- **Builtin inventory**: generated `specs/status/builtins.json`
  (`just regen-builtins`, drift-checked by `builtins-drift.yml`)
- **Test counts / pass rates**: CI (`bash_spec_tests` job); spec cases in
  `crates/bashkit/tests/spec_cases/`
- **Resource limit defaults**: `crates/bashkit/src/limits.rs`
- **Hook/binding API surface**: rustdoc + binding type stubs

Intentional-limitation IDs (`L-<AREA>-<NNN>`) are stable: code comments
and docs reference them (like TM-* threat IDs). Never renumber; mark
lifted limitations as removed in the PR that lifts them.
`limitations_doc_format` in `crates/bashkit/tests/integration/` lints the
table format and ID uniqueness.

## Intentional Limitations

By design — these conflict with the sandboxed, virtual, stateless
execution model. Evidence is a threat-model ID, a test, or `stance`
(untestable position).

| ID | Limitation | Why | Evidence |
|----|------------|-----|----------|
| L-PROC-001 | `exec` does not replace the process; `exec cmd` runs cmd then stops execution (fd redirects work) | True process replace would break sandbox containment | TM-ESC-005 |
| L-PROC-002 | No job control (`bg`, `fg`, `jobs`) | Requires process state; interactive-only feature | stance |
| L-PROC-003 | No process spawning; external commands run as builtins | Core sandbox model: no fork/exec escape surface | stance |
| L-FS-001 | Symlinks stored but never followed in path resolution (`ln -s` works, `read_link()` returns targets, traversal blocked) | Prevents symlink loops and link-based sandbox escapes | TM-DOS-011 |
| L-FS-002 | No file permission enforcement in the VFS | Single-tenant virtual FS; permissions would be theater | stance |
| L-NET-001 | No raw network sockets; HTTP only via `curl`/`wget`/`http` builtins | Allowlist-mediated egress is the only network surface | stance |
| L-NET-002 | No DNS resolution; hosts must appear in the allowlist | Resolution would bypass allowlist intent | stance |
| L-SIG-001 | `trap` stores INT/TERM handlers but no signal delivery in virtual mode (EXIT, ERR fire) | No host signals exist inside the sandbox | stance |

### Design Rationale

**Stateless execution model**: scripts run in isolated, stateless
contexts; each command completes before the next begins. Prevents
resource leaks from orphaned work, simplifies limit enforcement, keeps
agent runs deterministic. (`&` background execution + `wait` are
supported within an exec call.)

**bash/sh as virtual re-invocation**: `bash script.sh` / `bash -c` /
`bash -n` re-enter the Bashkit interpreter — same virtual environment,
shared state and limits, never an external process. `bash --version`
reports Bashkit. Security analysis: TM-ESC-015 in
[threat-model.md](threat-model.md).

## POSIX Compliance Stance

Target: IEEE 1003.1-2024 Shell Command Language.

| Category | Status | Notes |
|----------|--------|-------|
| Reserved words, special parameters | Full | All 16 / all 8 |
| Special built-in utilities | Substantial | 14/15; `exec` partial (L-PROC-001); `times` returns zeros; `trap` per L-SIG-001 |
| Quoting, redirections, compound commands, functions | Full | |
| Word expansions | Substantial | Most expansions supported |
| Pipelines and lists | Full | `\|`, `&&`, `\|\|`, `;`, `&`+`wait`, `!` |

## Shell Features

### Not Yet Implemented

| Feature | Priority | Notes |
|---------|----------|-------|
| History expansion | Out of scope | Interactive only |

### Partially Implemented

| Feature | What Works | What's Missing |
|---------|------------|----------------|
| Prefix env assignments | `VAR=val cmd` temporarily sets env for cmd | Array prefix assignments not in env |
| `local` | Declaration | Proper scoping in nested functions |
| `return` | Basic usage | Return value propagation |
| `time` | Wall-clock timing | User/sys CPU time (always 0) |
| `timeout` | Basic usage | `-k` kill timeout |
| `bash`/`sh` | `-c`, `-n`, `-e`, `-x`, `-u`, `-f`, `-o option`, script files, stdin, `--version`, `--help` | Login shell |

## Builtins

Inventory is generated — see [status/builtins.json](status/builtins.json)
and the [builtins spec](builtins.md). No unimplemented builtins currently
tracked.

## Text Processing

What each tool does is covered by its spec tests (all unskipped tests
pass in CI); only divergences and boundaries are recorded here.

| ID | Tool | Limitation | Evidence |
|----|------|------------|----------|
| L-AWK-001 | awk | Some complex regex patterns unsupported (engine shared with sed/grep, size-limited) | stance |
| L-JQ-001 | jq | Alternative `//`: jaq errors on `.foo` applied to null instead of returning null (upstream jaq divergence) | 1 skipped spec test |
| L-GREP-001 | grep | `--color`/`--colour`, `--line-buffered` accepted as no-ops | stance |
| L-CURL-001 | curl | Spec-test coverage for methods/headers/payloads/auth/redirects not ported (needs `http_client` + allowlist in harness); behavior covered by integration tests | stance |

Safety boundaries (enforced, not bugs): printf width/precision caps,
output buffer caps, getline file-cache cap, shared regex size limit,
curl/wget timeouts clamped to [1, 600] s, multipart field-name
sanitization, redirect handling hardened against credential leaks.

## Parser

- Single-quoted strings are completely literal (correct behavior)
- Some complex nested structures may hit the parser timeout
- Very long pipelines may cause stack issues
- Bounded by configurable limits: timeout, fuel, input size, AST depth

## Lifting a Limitation / Adding One

1. Add a spec test demonstrating it, marked `### skip: reason`
   (or an expected-fail differential test)
2. Add a row here — with an `L-*` ID if it's an intentional decision
3. When lifting: un-skip the test, delete the row, update referencing
   code comments in the same PR
