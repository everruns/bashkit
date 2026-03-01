# Bashkit Security Audit Report

**Date:** 2026-03-01
**Scope:** Full codebase (Rust core + Python bindings)
**Methodology:** Manual code review of all security-sensitive components

---

## Executive Summary

Bashkit demonstrates a strong security architecture overall. The virtual filesystem provides genuine isolation, the network allowlist is default-deny, resource limits are comprehensive, and there is zero `unsafe` Rust code. The threat model (`specs/006-threat-model.md`) is thorough and most documented threats are properly mitigated.

However, this audit identified **27 security findings** across 5 severity levels:

| Severity | Count | Key Themes |
|----------|-------|------------|
| CRITICAL | 2 | Arithmetic panic/DoS, VFS limit bypass via public API |
| HIGH | 8 | Internal variable namespace injection, parser limit bypass, extglob DoS, process env pollution, shell injection in Python wrappers |
| MEDIUM | 10 | TOCTOU in VFS, tar path traversal, OverlayFs limit inconsistencies, GIL deadlock risk |
| LOW | 7 | PID leak, error message info leaks, integer truncation on 32-bit |

---

## CRITICAL Findings

### C-1: Integer Overflow / Panic in Arithmetic Exponentiation

**File:** `crates/bashkit/src/interpreter/mod.rs:7444`

```rust
return left.pow(right as u32);
```

`right` is `i64`. Casting to `u32` silently wraps negatives (e.g., `-1` becomes `4294967295`). Calling `i64::pow()` with a large exponent causes a panic in debug builds or massive CPU hang in release. Even `$(( 2 ** 63 ))` overflows `i64`.

**Attack:** `$(( 2 ** -1 ))` or `$(( 2 ** 999999999999 ))` crashes or hangs the interpreter.

**Related:** Shift operators at lines 7343/7353 (`left << right`, `left >> right`) panic if `right >= 64` or `right < 0`. All standard arithmetic (`+`, `-`, `*`) at lines 7379-7407 panics on overflow in debug. Division `i64::MIN / -1` at line 7413 panics (not caught by the `right != 0` check).

**Fix:**
```rust
// Exponentiation
let exp = right.clamp(0, 63) as u32;
return left.wrapping_pow(exp);

// Shifts
let shift = right.clamp(0, 63) as u32;
return left.wrapping_shl(shift);

// All arithmetic: use wrapping_add, wrapping_sub, wrapping_mul
// Division: check left == i64::MIN && right == -1
```

### C-2: `add_file()` and `restore()` Bypass All VFS Limits

**File:** `crates/bashkit/src/fs/memory.rs:658-698` (add_file), `549-603` (restore)

`InMemoryFs::add_file()` is a `pub` method that:
- Does NOT call `validate_path()` (no path depth/length/unicode checks)
- Does NOT call `check_write_limits()` (no file size, total bytes, or file count checks)

Any code with access to `InMemoryFs` (including via `OverlayFs::upper()`) can bypass all filesystem limits.

Similarly, `restore()` deserializes a `VfsSnapshot` and inserts all entries without any validation or limit checks.

**Compounding factor:** `OverlayFs::upper()` (line 241) returns `&InMemoryFs` with `FsLimits::unlimited()`, so calling `overlay.upper().add_file(...)` or `overlay.upper().write_file(...)` trivially bypasses all OverlayFs-level limits.

**Fix:** `add_file()` should call `validate_path()` and `check_write_limits()`. Alternatively, make it `pub(crate)` or document it is only safe during construction. `OverlayFs::upper()` should not be public, or should return a limited view.

---

## HIGH Findings

### H-1: Internal Variable Namespace Injection

**File:** `crates/bashkit/src/interpreter/mod.rs` (various locations)

The interpreter uses magic variable prefixes as internal control signals:
- `_NAMEREF_<name>` (line 7549): nameref resolution
- `_READONLY_<name>` (line 5333): readonly enforcement
- `_SHIFT_COUNT` (line 4255): positional parameter shifting
- `_SET_POSITIONAL` (line 4268): positional parameter replacement
- `_UPPER_<name>`, `_LOWER_<name>` (line 7517): case conversion

A user script can set these directly:
```bash
_READONLY_PATH=""          # Bypass readonly protection
_NAMEREF_x="PATH"         # Create unauthorized nameref
_SHIFT_COUNT=100           # Manipulate builtins
```

`${!_NAMEREF_*}` (PrefixMatch at line 6034) also exposes all internal variables.

**Fix:** Use a separate `HashMap` for internal state, or reject user assignments to names starting with `_NAMEREF_`, `_READONLY_`, etc. Filter internal names from `${!prefix*}` results.

### H-2: Parser Limits Bypassed via `eval`, `source`, Traps, Aliases

**File:** `crates/bashkit/src/interpreter/mod.rs`

Multiple code paths create `Parser::new()` which ignores the interpreter's configured limits:
- `source` command (line 4548): `Parser::new(&content)`
- `eval` command (line 4613): `Parser::new(&cmd)`
- Alias expansion (line 3627): `Parser::new(&expanded_cmd)`
- EXIT trap (line 697): `Parser::new(&trap_cmd).parse()`
- ERR trap (line 7795): `Parser::new(&trap_cmd).parse()`

If the interpreter was configured with stricter `max_ast_depth` or `max_parser_operations`, these paths silently use defaults.

**Fix:** Use `Parser::with_limits()` everywhere, propagating `self.limits.max_ast_depth` and `self.limits.max_parser_operations`.

### H-3: Unbounded Recursion in ExtGlob Matching

**File:** `crates/bashkit/src/interpreter/mod.rs:3043-3092`

The `+(...)` and `*(...)` extglob handlers recursively call `glob_match_impl` without any depth limit. For each split point in the string, the function recurses with a reconstructed pattern, creating O(n!) time complexity.

**Attack:** Pattern `+(a|aa)` against a long string of `a`s causes exponential time/stack overflow.

**Fix:** Add a depth parameter to `glob_match_impl` and `match_extglob`, and bail when exceeded.

### H-4: Process Environment Pollution in `jq` Builtin (Thread-Unsafe)

**File:** `crates/bashkit/src/builtins/jq.rs:414-421`

```rust
std::env::set_var(k, v);  // Modifies real process env
```

The jq builtin calls `std::env::set_var()` to expose shell variables to jaq's `env` function. This is:
1. **Thread-unsafe:** `set_var` is unsound in multi-threaded contexts (flagged `unsafe` in Rust 2024 edition)
2. **Info leak:** Host process env vars (API keys, tokens) become visible via jaq's `env`
3. **Race condition:** Concurrent jq calls corrupt each other's env state

**Fix:** Provide a custom `env` implementation to jaq that reads from `ctx.env`/`ctx.variables`, or add a mutex around the env block.

### H-5: Shell Injection in Python `BashkitBackend` (deepagents.py)

**File:** `crates/bashkit-python/bashkit/deepagents.py` (lines 187, 198, 206, 230, 258, 278, 302)

Multiple methods construct shell commands via f-string interpolation of user-supplied paths and content:

```python
result = self._bash.execute_sync(f"cat {file_path}")          # path injection
cmd = f"cat > {file_path} << 'BASHKIT_EOF'\n{content}\nBASHKIT_EOF"  # heredoc escape
cmd = f"grep -rn '{pattern}' {path}"                          # pattern injection
```

A path like `"/dev/null; echo pwned > /important/file"` executes injected commands within the VFS. Content containing the literal `BASHKIT_EOF` delimiter terminates the heredoc early, causing remaining text to execute as shell commands.

**Fix:** Use `shlex.quote()` for all interpolated values, or expose direct VFS methods from Rust that bypass shell parsing.

### H-6: Heredoc Content Injection in `BashkitBackend.write()`

**File:** `crates/bashkit-python/bashkit/deepagents.py:198`

Specific variant of H-5. Content containing `BASHKIT_EOF` on its own line terminates the heredoc. Everything after becomes shell commands within the VFS.

**Fix:** Use a random delimiter suffix or expose a direct write API.

### H-7: GIL Deadlock Risk in `execute_sync`

**File:** `crates/bashkit-python/src/lib.rs:510-527`

`execute_sync()` calls `rt.block_on()` without releasing the GIL. Inside that, tool callbacks call `Python::attach()` to reacquire the GIL. While PyO3 handles same-thread reentrance, this can deadlock in multi-threaded Python scenarios.

**Fix:** Wrap `rt.block_on()` with `py.allow_threads(|| { ... })`.

### H-8: Tokio Runtime Created Per Sync Call (Resource Exhaustion)

**File:** `crates/bashkit-python/src/lib.rs:237-258, 261-271, 510-527`

Each `execute_sync()` and `reset()` creates a new `tokio::runtime::Runtime`, spawning OS threads. Under rapid-fire calls (e.g., from an LLM agent loop), this exhausts OS thread/fd limits.

**Fix:** Use a shared runtime stored in the struct, or use `Builder::new_current_thread()`.

---

## MEDIUM Findings

### M-1: TOCTOU Race in `InMemoryFs::append_file()`

**File:** `crates/bashkit/src/fs/memory.rs:816-896`

`append_file()` reads file state under a read lock, drops it, checks limits with stale data, then acquires a write lock. Another thread can modify the file between locks, making size checks inaccurate.

**Fix:** Use a single write lock for the entire operation, or re-check size after acquiring the write lock.

### M-2: Tar Path Traversal Within VFS

**File:** `crates/bashkit/src/builtins/archive.rs:538, 560`

Tar entry names like `../../../etc/passwd` are passed to `resolve_path()` which normalizes `..` but still writes to arbitrary VFS locations outside the extraction directory.

**Fix:** Validate that the resolved path starts with `extract_base`. Reject entries with `..` or leading `/`.

### M-3: `OverlayFs::chmod` Copy-on-Write Bypasses Limits

**File:** `crates/bashkit/src/fs/overlay.rs:610-638`

When `chmod` triggers copy-on-write from the lower layer, it writes directly to `self.upper` (unlimited InMemoryFs), bypassing `OverlayFs::check_write_limits()`.

### M-4: `OverlayFs` Usage Double-Counts Files

**File:** `crates/bashkit/src/fs/overlay.rs:246-259`

`compute_usage()` sums upper + lower layer usage without deducting overwritten or whited-out files. This makes `usage()` inaccurate and can cause premature limit rejections.

### M-5: `OverlayFs::check_write_limits` Checks Only Upper Layer

**File:** `crates/bashkit/src/fs/overlay.rs:263-293`

Total bytes checked against upper layer only. If the lower layer has 80MB and the combined limit is 100MB, the upper layer is allowed another full 100MB (total: 180MB).

### M-6: Incomplete Recursive Delete Whiteout in `OverlayFs`

**File:** `crates/bashkit/src/fs/overlay.rs:456-484`

`rm -r /dir` only whiteouts the directory path, not child files. `is_whiteout()` uses exact path matching, so `/dir/file.txt` remains visible after deleting `/dir`.

### M-7: Missing Path Validation Across Multiple VFS Methods

**Files:** `crates/bashkit/src/fs/memory.rs`, `overlay.rs`, `mountable.rs`, `posix.rs`

`validate_path()` is only called in `read_file`, `write_file`, `append_file`, and `mkdir` within `InMemoryFs`. Missing from: `remove`, `stat`, `read_dir`, `exists`, `rename`, `copy`, `symlink`, `read_link`, `chmod`.

`copy()` is particularly notable: it creates a new entry without checking write limits, enabling file count and total bytes bypass.

`PosixFs` and `MountableFs` never call `validate_path()` at all.

### M-8: `BashTool::create_bash()` Loses Custom Builtins After First Call

**File:** `crates/bashkit/src/tool.rs:456`

```rust
for (name, builtin) in std::mem::take(&mut self.builtins) {
```

`std::mem::take` empties `self.builtins`. The first `execute()` gets all custom builtins; subsequent calls get none. If custom builtins enforce security wrappers, those are silently removed.

### M-9: Git Branch Name Path Injection

**File:** `crates/bashkit/src/git/client.rs:1035, 1080, 1119`

Branch names are used directly in `Path::join()` without validation. `branch_create(name="../../config")` could overwrite `.git/config` within the VFS.

**Fix:** Validate branch names against git's ref name rules (no `..`, no control chars, no trailing `.lock`).

### M-10: `reset()` Discards Security Configuration

**File:** `crates/bashkit-python/src/lib.rs:260-271`

`BashTool.reset()` creates a new `Bash` with bare `Bash::builder()`, discarding `max_commands`, `max_loop_iterations`, `username`, and `hostname` configuration. After `reset()`, DoS protections are removed.

**Fix:** Store original builder config and reapply on reset.

---

## LOW Findings

### L-1: `$$` Leaks Real Host Process ID

**File:** `crates/bashkit/src/interpreter/mod.rs:7615`

```rust
return std::process::id().to_string();
```

Violates sandbox isolation by returning the real OS PID. **Fix:** Return a fixed or random value.

### L-2: Cyclic Nameref Silently Resolves to Wrong Variable

**File:** `crates/bashkit/src/interpreter/mod.rs:7547-7560`

Cyclic namerefs (a->b->a) silently resolve to whatever variable is current after 10 iterations instead of producing an error.

### L-3: `py_to_json` / `json_to_py` Unbounded Recursion

**File:** `crates/bashkit-python/src/lib.rs:58-92`

Deeply nested Python dicts/lists cause stack overflow in the recursive JSON conversion functions.

**Fix:** Add a depth counter; fail beyond 64 levels.

### L-4: Error Messages May Leak Internal State

**Files:** `crates/bashkit/src/error.rs:38` (`Io` wraps `std::io::Error`), `network/client.rs:224` (reqwest errors), `git/client.rs` (VFS paths and remote URLs), `scripted_tool/execute.rs:323` (Debug-formatted errors)

Internal details (host paths, resolved IPs, TLS info) can leak through error messages. The `scripted_tool` uses `{:?}` (Debug format) while `BashTool` uses `error_kind()` -- inconsistent.

### L-5: URL Credentials Leaked in Blocked Error Messages

**File:** `crates/bashkit/src/network/allowlist.rs:144`

Full URL (including potential `user:pass@` in query strings or authority) is echoed in "URL not in allowlist" errors.

### L-6: Integer Truncation on 32-bit Platforms

**Files:** `crates/bashkit/src/network/client.rs:236,419` (`content_length as usize`), `crates/bashkit-python/src/lib.rs:197,200` (`u64 as usize` for limits)

On 32-bit platforms, large values silently truncate, potentially bypassing size checks.

### L-7: No Limit on AWK Loop Iterations

**File:** `crates/bashkit/src/builtins/awk.rs`

AWK `while`/`for` loops have no iteration limit. `BEGIN { while(1){} }` loops until the bash-level timeout fires (30s default), consuming CPU.

---

## Positive Security Observations

The following aspects of the security design are well-implemented:

1. **Zero `unsafe` Rust** across the entire codebase
2. **Virtual filesystem** provides genuine process-level isolation -- no real host FS access
3. **Default-deny network** -- empty allowlist blocks all outbound requests
4. **No auto-redirect following** in HTTP client prevents allowlist bypass
5. **Compression bomb protection** -- auto-decompression disabled, ratio limits on archive extraction
6. **Rust `regex` crate** guarantees linear-time matching (no ReDoS)
7. **Builtin panic recovery** via `catch_unwind` with sanitized error messages
8. **Symlinks not followed** prevents symlink-based escape attacks
9. **HTTPS-only git remotes** with no external process execution
10. **Comprehensive resource limits** with per-`exec()` counter reset
11. **Log redaction** for secrets, URLs, API keys by default
12. **Python (Monty) sandbox** with no `os.system`, `subprocess`, `socket`, or `open()`
13. **Path normalization** consistently applied to collapse `..` traversal
14. **System builtins return virtual values** (hostname, whoami, uname, id)
15. **Fail-point injection testing** infrastructure for systematic security verification

---

## Threat Model Gaps

The existing threat model (`specs/006-threat-model.md`) does not cover:

| Gap | Related Findings |
|-----|-----------------|
| Internal variable namespace injection | H-1 |
| Arithmetic overflow/panic | C-1 |
| ExtGlob exponential blowup | H-3 |
| VFS limit bypass via public API (`add_file`, `upper()`) | C-2 |
| Cross-layer limit accounting in OverlayFs | M-3, M-4, M-5 |
| `jq` process env pollution | H-4 |
| Python binding shell injection | H-5, H-6 |
| Parser limit bypass via eval/source/trap | H-2 |

---

## Prioritized Remediation

### Immediate (blocks production use)
1. **C-1**: Use wrapping/checked arithmetic in all arithmetic operations
2. **H-4**: Stop mutating `std::env` in jq builtin
3. **H-1**: Isolate internal variable namespace from user scripts

### Short-term (next release)
4. **C-2**: Add limit checks to `add_file()`, restrict `upper()` visibility
5. **H-2**: Propagate parser limits to all `Parser::new()` call sites
6. **H-3**: Add depth limit to extglob matching
7. **H-5/H-6**: Fix shell injection in deepagents.py
8. **M-2**: Validate tar extraction paths stay within target directory
9. **M-8**: Fix `BashTool` custom builtin loss after first execution

### Medium-term (hardening)
10. **M-1**: Fix TOCTOU in append_file
11. **M-3/M-4/M-5/M-6**: Fix OverlayFs limit accounting and whiteout propagation
12. **M-7**: Add `validate_path()` to all VFS methods
13. **M-9**: Validate git branch names
14. **M-10**: Preserve config on reset()
15. **H-7/H-8**: Fix Python binding GIL handling and runtime management
