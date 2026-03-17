# Technical Debt Analysis

Date: 2026-03-17

Deep analysis of shortcuts, hacks, duplications, lazy abstractions, and over-complications.

---

## 1. God Module: `interpreter/mod.rs` (11,991 lines)

**Severity: HIGH | Type: Shortcut / Accumulated Debt**

The interpreter is a single 12K-line file containing ~50 `execute_*` methods, variable expansion, arithmetic evaluation, glob expansion, IFS splitting, redirection handling, `find`/`xargs`/`timeout` execution — all in one `impl Interpreter` block.

This is the classic "everything goes into the interpreter" shortcut. Rather than designing a proper command dispatch architecture, each new feature was added as another method on `Interpreter`. The file has grown organically and is now unmaintainable without grep.

**Evidence:**
- 50+ `async fn execute_*` methods in a single impl block
- `find_collect`, `find_printf_format`, `execute_xargs`, `execute_timeout` — these are command implementations that live in the interpreter instead of as builtins
- Arithmetic evaluation (`evaluate_arithmetic_with_assign`, 110+ lines) inlined in the interpreter
- Glob expansion (~200 lines) inlined in the interpreter
- IFS splitting (~80 lines) inlined in the interpreter

**Proper fix:** Extract into submodules — `interpreter/expansion.rs`, `interpreter/arithmetic.rs`, `interpreter/glob.rs`, `interpreter/redirection.rs`, `interpreter/dispatch.rs`. Move `find`/`xargs`/`timeout` to builtins with proper interpreter hooks.

---

## 2. Magic Variable Hack: `_NAMEREF_`, `_READONLY_`, `_SHIFT_COUNT`, etc.

**Severity: HIGH | Type: Hack / Architectural Shortcut**

Builtins communicate side effects back to the interpreter via magic-prefixed variables smuggled through the `variables` HashMap. The `Builtin` trait's `Context` only gives `&mut HashMap<String, String>` for variables, so builtins that need to affect interpreter state (shift positional params, set arrays, mark readonly) write sentinel values like `_SHIFT_COUNT`, `_SET_POSITIONAL`, `_ARRAY_READ_*`, `_READONLY_*` into the variables map. The interpreter then post-processes these after every builtin call.

**Evidence (interpreter/mod.rs:5004-5056):**
```rust
// Post-process: read -a populates array from marker variable
let markers: Vec<(String, String)> = self.variables.iter()
    .filter(|(k, _)| k.starts_with("_ARRAY_READ_"))
    .map(|(k, v)| (k.clone(), v.clone()))
    .collect();

// Post-process: shift builtin updates positional parameters
if let Some(shift_str) = self.variables.remove("_SHIFT_COUNT") { ... }

// Post-process: `set --` replaces positional parameters
if let Some(encoded) = self.variables.remove("_SET_POSITIONAL") { ... }
```

This is a classic in-band signaling hack. The `_READONLY_*` prefix is particularly fragile — readonly is tracked by checking if `_READONLY_varname` exists in the variables map, meaning metadata about variables is stored alongside the variables themselves using naming conventions.

A security function `is_internal_variable()` exists solely to prevent user scripts from setting these magic markers — a band-aid for the band-aid.

**Proper fix:** Extend `ExecResult` or the `Context` trait with a structured side-effect channel (e.g., `enum BuiltinSideEffect { ShiftPositional(usize), SetArray(String, HashMap<usize, String>), MarkReadonly(String), ... }`). Or give builtins a richer `Context` with methods like `ctx.set_array()`, `ctx.shift_positional()`.

---

## 3. Duplicated `is_valid_var_name()` — 3 Identical Copies

**Severity: MEDIUM | Type: Copy-Paste Duplication**

The exact same function appears in three places:
- `interpreter/mod.rs:8426`
- `builtins/vars.rs:12`
- `builtins/export.rs:10`

All three are identical:
```rust
fn is_valid_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
```

**Proper fix:** Make it `pub(crate)` in one location (e.g., `interpreter/mod.rs` where it already exists) and import it elsewhere.

---

## 4. `grep` and `rg` Builtins: Duplicated Matching Logic

**Severity: MEDIUM | Type: Duplication to Reduce Work**

`builtins/grep.rs` (1,468 lines) and `builtins/rg.rs` (291 lines) implement overlapping regex-based file searching. Both:
- Parse similar option sets (`-i`, `-v`, `-n`, `-c`, `-l`, `-F`, `-w`, `-m`)
- Build regex with `RegexBuilder` using the same pattern
- Walk directories recursively with the same logic
- Format output with the same filename:lineno:match pattern

`rg` is essentially a simplified copy of `grep` with different defaults (line numbers on by default, recursive by default). Rather than having `rg` delegate to `grep` with different defaults, the matching/output logic was duplicated.

**Proper fix:** Extract shared matching engine (regex building, line matching, output formatting) into a shared module. Have both `grep` and `rg` configure and call it.

---

## 5. `find` and `xargs` Live in the Interpreter, Not as Builtins

**Severity: MEDIUM | Type: Shortcut / Lazy Architecture**

Unlike all other commands, `find` and `xargs` are implemented as private methods on `Interpreter` rather than as `Builtin` trait implementations. This is because they need deep interpreter access:
- `find` needs recursive filesystem access (which builtins also have via `ctx.fs`)
- `xargs` needs to execute commands (which requires interpreter state)

The lazy shortcut was to just implement them directly in the interpreter. But `timeout` (also in the interpreter) shows the same pattern, and `parallel` (a builtin) somehow manages to execute commands without being in the interpreter.

**Evidence:**
- `interpreter/mod.rs:2642` — `execute_find` (350+ lines)
- `interpreter/mod.rs:2485` — `execute_xargs` (150+ lines)
- `interpreter/mod.rs:2327` — `execute_timeout` (150+ lines)
- Meanwhile `builtins/parallel.rs` handles command execution as a proper builtin

---

## 6. Interpreter-Level Builtin Dispatch Chain: `if name == "..."` x 20

**Severity: MEDIUM | Type: Shortcut / Missing Abstraction**

Before reaching the `Builtin` trait dispatch, the interpreter has ~20 hardcoded `if name == "type"`, `if name == "trap"`, `if name == "declare"` checks (interpreter/mod.rs:4900-4966). These are commands that need interpreter-level access (call stack, aliases, arrays, traps) and couldn't fit the `Builtin` trait's limited `Context`.

This is the natural consequence of hack #2 (magic variables). Commands that need more than `Context` provides get special-cased in the interpreter dispatch chain instead.

**Evidence (interpreter/mod.rs:4900-4966):**
```rust
if name == "type" { return self.execute_type_builtin(...).await; }
if name == "which" { return self.execute_which_builtin(...).await; }
if name == "hash" { ... }
if name == "trap" { return self.execute_trap_builtin(...).await; }
if name == "declare" || name == "typeset" { return self.execute_declare_builtin(...).await; }
if name == "let" { return self.execute_let_builtin(...).await; }
if name == "unset" { return self.execute_unset_builtin(...).await; }
if name == "getopts" { return self.execute_getopts(...).await; }
if name == "caller" { return self.execute_caller_builtin(...).await; }
if name == "wait" { return self.execute_wait_builtin(...).await; }
if name == "mapfile" || name == "readarray" { return self.execute_mapfile(...).await; }
if name == "alias" { return self.execute_alias_builtin(...).await; }
if name == "unalias" { return self.execute_unalias_builtin(...).await; }
```

These 20 commands bypass the `Builtin` trait entirely. Each new command that needs interpreter state gets another `if name == "..."` line rather than enriching the abstraction.

---

## 7. Manual Builtin Registration: 120+ Hardcoded `insert` Calls

**Severity: LOW-MEDIUM | Type: Boilerplate / Missing Abstraction**

Builtin registration is a 170-line block of repetitive `builtins.insert("name".to_string(), Box::new(builtins::Name))` calls (interpreter/mod.rs:382-553). No macro, no inventory, no auto-registration. Every new builtin requires adding a line here AND the `mod`/`pub use` in `builtins/mod.rs`.

This is tedious but functional. A `#[builtin("name")]` attribute macro or an `inventory` crate registration would eliminate the boilerplate.

---

## 8. `declare` and `local` Duplicate Compound Assignment Parsing

**Severity: LOW-MEDIUM | Type: Copy-Paste Duplication**

Both `execute_declare_builtin` (interpreter/mod.rs:6821-6846) and `execute_local_builtin` (interpreter/mod.rs:5541-5561) contain identical "reconstruct compound assignments" logic:

```rust
// Both contain this identical block:
let mut merged: Vec<String> = Vec::new();
let mut pending: Option<String> = None;
for arg in &var_args {
    if let Some(ref mut p) = pending {
        p.push(' ');
        p.push_str(arg);
        if arg.ends_with(')') {
            merged.push(p.clone());
            pending = None;
        }
    } else if arg.contains("=(") && !arg.ends_with(')') {
        pending = Some(arg.to_string());
    } else {
        merged.push(arg.to_string());
    }
}
```

Also, both `declare` and `local` duplicate flag parsing (`-n`, `-a`, `-A`, `-i`, `-r`, `-x`).

---

## 9. VFS Missing `delete_file` — Documented WTF

**Severity: LOW | Type: Acknowledged Hack**

`builtins/patch.rs:352-353`:
```rust
// WTF: VFS doesn't have a delete_file, using write with empty content as a workaround.
// Real deletion would need fs.remove().
```

The `FileSystem` trait has no `remove()` or `delete_file()` method. The `rm` builtin exists but uses `Rm` struct's own logic. When `patch` needs to delete a file, it writes empty content instead of actually deleting it. This means patched-as-deleted files still appear in `ls` and `find` as 0-byte files.

---

## 10. Shell Options: Split Brain Between `ShellOptions` Struct and `SHOPT_*` Variables

**Severity: MEDIUM | Type: Inconsistent Design / Accumulated Debt**

Shell options exist in two places:
1. `ShellOptions` struct on `Interpreter` (errexit, xtrace, pipefail) — checked by the interpreter directly
2. `SHOPT_*` variables in the variables HashMap (e.g., `SHOPT_e`, `SHOPT_u`, `SHOPT_pipefail`) — set by the `set`/`shopt` builtins

The builtins can only modify variables (via `Context`), so they write `SHOPT_*` magic variables. The interpreter then reads both its own `ShellOptions` struct AND checks for `SHOPT_*` variables. This creates a split-brain where the same option is tracked in two places.

**Evidence:** `set -e` writes `SHOPT_e=1` to variables. But `options.errexit` on the interpreter struct is what actually controls execution. The interpreter has to sync between these.

---

## 11. Blanket `#[allow(clippy::unwrap_used)]` on Test Modules (90+ instances)

**Severity: LOW | Type: Shortcut**

Nearly every test module has `#[allow(clippy::unwrap_used)]` at the top, and the interpreter module blanket-allows it with a comment. This effectively disables an important lint across 90% of the codebase. The interpreter's justification comment says "safe because we check for non-empty strings" — but that's a fragile invariant that isn't enforced by the type system.

---

## 12. `git show rev:path` Ignores the Revision

**Severity: LOW | Type: Incomplete Implementation**

`git/client.rs:1286`:
```rust
let _ = rev; // TODO: resolve rev to actual snapshot
```

`git show HEAD~1:file.txt` silently shows the current file content instead of the historical version. No error, no warning — just wrong output.

---

## Summary by Priority

| # | Issue | Severity | Type | Effort |
|---|-------|----------|------|--------|
| 1 | God module interpreter/mod.rs | HIGH | Accumulated debt | Large |
| 2 | Magic variable hack for builtin side-effects | HIGH | Architectural hack | Large |
| 6 | if name == "..." dispatch chain (consequence of #2) | MEDIUM | Missing abstraction | Medium |
| 10 | Shell options split brain | MEDIUM | Inconsistent design | Medium |
| 3 | Triplicated `is_valid_var_name` | MEDIUM | Copy-paste | Trivial |
| 4 | grep/rg duplicated logic | MEDIUM | Duplication | Medium |
| 5 | find/xargs/timeout in interpreter | MEDIUM | Lazy architecture | Medium |
| 8 | declare/local duplicate parsing | LOW-MED | Copy-paste | Small |
| 7 | Manual builtin registration | LOW-MED | Boilerplate | Medium |
| 9 | VFS missing delete_file | LOW | Known hack | Small |
| 11 | Blanket clippy::unwrap_used allows | LOW | Shortcut | Small |
| 12 | git show ignores revision | LOW | Incomplete | Small |

### Root Cause Pattern

Issues #2, #6, and #10 share a root cause: **the `Builtin` trait's `Context` is too narrow**. Builtins only get `&mut HashMap<String, String>` for variables and can't affect interpreter state (positional params, arrays, assoc arrays, traps, aliases, call stack, shell options). This forced two compensating patterns:
1. Magic marker variables for simple side effects (#2)
2. Interpreter-level special-casing for complex commands (#6)

Enriching `Context` (or providing a callback/channel for side effects) would eliminate both hacks and the shell options split brain (#10) simultaneously.
