# Coreutils argument-surface port

## Status
Active (`cat`, `tac`, `ls`, `shuf`, `readlink`, `truncate`)

## Decision

Reuse uutils/coreutils' clap argument definitions in bashkit by **port-time
codegen**, not by depending on `uu_*` crates at runtime.

`crates/bashkit-coreutils-port/` is a small standalone binary that:

1. Parses `<uutils>/src/uu/<util>/src/<util>.rs` with `syn`, falling back to
   sibling `.rs` files (e.g. `ls/src/config.rs`) when `mod options` lives
   next to `<util>.rs`.
2. Reads `<uutils>/src/uu/<util>/locales/en-US.ftl` for help/about strings.
3. Rewrites the `uu_app()` AST in place:
   - `translate!("k")` → `String::from("<value from ftl>")`
   - `uucore::crate_version!()` → `env!("CARGO_PKG_VERSION")`
   - `uucore::format_usage(x)` → local `format_usage` shim
   - `.help_template(uucore::localized_help_template(...))` and
     `uucore::clap_localization::configure_localized_command(cmd)` → elided
   - `ShortcutValueParser::new([…])` → `clap::builder::PossibleValuesParser::new([…])`
     (loses uucore's unambiguous-abbreviation behaviour; documented divergence)
   - `Arg::…env("FOO")…` → chain step elided AND harvested into a sidecar
     table (TM-INF-024). uutils attaches `.env(...)` to options like
     `TABSIZE`/`TIME_STYLE` so they pick up host process state; bashkit
     sandboxes scripts inside `ctx.env`, so the generated `<util>_command()`
     only consults argv. To preserve uutils' UX, codegen records each
     stripped `.env(...)` into `pub static <UTIL>_ENV_DEFAULTS:
     &[clap_env::EnvDefault]` next to the command builder — rows of
     `(arg_id, long, env_var, kind ∈ {Single, Bool, Multi})`. The shim
     `crate::builtins::clap_env::apply_env_defaults` reads the table plus
     the caller's `ctx.env` and synthesises `--<long> <value>` (or
     `--<long>` for `Bool`) into argv before `try_get_matches_from`,
     emulating clap's "argv > env > default" precedence — sourced from the
     sandbox, never `std::env`. Defence-in-depth: the workspace `clap` dep
     drops the `env` cargo feature,
     `builtins::tests::no_clap_env_in_generated_parsers` statically forbids
     runtime `.env(` calls in `generated/*.rs`, and
     `every_generated_parser_emits_env_defaults_table` enforces the uniform
     sidecar surface (every util emits the table, possibly empty).
     Per-builtin opt-in: a builtin chooses whether to wire through the shim —
     if it does, every uutils env-default auto-lights as that option's
     bashkit support lands.
4. Validates the rewritten `uu_app()` before emission: args mode accepts
   either a single tail clap `Command` builder expression, or the
   two-statement shape `let <ident> = Command::new(...)<chain>;
   <ident>.<method>(...)<chain>` where the `let` initializer is itself a
   Command::new chain and the tail's innermost receiver is the let-bound
   identifier (the shape that emerges after folding
   `configure_localized_command(cmd)` to `cmd`). Anything else — additional
   prefix statements, block expressions, loops, matches, async blocks,
   unsafe blocks, or a destructuring/let-else binding — is rejected before
   any generated Rust is written. This keeps third-party uutils source from
   smuggling arbitrary executable statements into `<util>_command()`
   (TM-INF-025).
5. Emits `crates/bashkit/src/builtins/generated/<util>_args.rs` with a clean
   `pub fn <util>_command() -> clap::Command`.

bashkit's `Builtin::execute` calls `<util>_command().try_get_matches_from(...)`
and implements behaviour against the VFS. `clap` is an unconditional
dependency — no feature flag for the ported path or the `ClapBuiltin` trait.
Help template is overridden in the calling builtin (e.g. `cat.rs`) to put the
`Usage:` line first, matching GNU layout.

## Rationale

The uu_* crates expose `uu_app()` as their canonical clap definition, but
they hardcode `std::fs` / `io::stdin()` / `io::stdout()` (incompatible with
VFS), are sync (incompatible with tokio-async builtins), resolve help
strings through Fluent at runtime, and pull `rustix` / `winapi-util`
(hostile to WASM).

A runtime dep would force Fluent init and locale bundles into bashkit. A
`build.rs` would either vendor uutils as a submodule or fetch during every
clean build — both violate bashkit norms (build does not fetch; generated
artifacts are not in `target/`). Codegen via a binary, with output
committed, gives reviewability, grep-ability, and predictable build times —
at the cost of re-running the recipe on every uutils bump (drift CI below).

## Verification

Regenerate with `just regen-coreutils-args` (clones/updates the uutils
checkout, checks out the pin, regenerates every ported util). Spec tests:
`tests/spec_cases/bash/cat.test.sh`, `textrev.test.sh`, `help-flag.test.sh`.

## Scaling

Per new utility:

1. `just regen-coreutils-args` (extend the recipe's for-loop with the util).
2. Add `pub mod <util>_args;` to `crates/bashkit/src/builtins/generated/mod.rs`.
3. In the matching builtin, replace handwritten parsing with
   `<util>_command().try_get_matches_from(...)`.

The tool handles every uutils utility whose `uu_app()` follows the common
shape (`Command::new(...)` chain, `mod options`, flat `en-US.ftl`). Ports
needing bespoke transforms (no `mod options`, Fluent placables/selectors)
fail with an `unresolved translate!()` error rather than emitting
silently-wrong code.

## Verification — Differential tests

The args workflow only catches **flag-signature drift**; it cannot see
**body drift** (semantic divergence inside `cat.rs` / `textrev.rs` vs
GNU/uutils). `crates/bashkit/tests/integration/coreutils_differential_tests.rs`
closes that gap: per fixture row it runs the same `<util> <args>` (same
stdin, same input files) through bashkit and the matching uutils binary,
asserting byte-equal stdout + exit-code parity. Key properties:

- **Opt-in**: skips unless `BASHKIT_RUN_COREUTILS_DIFF=1` — body divergences
  are *expected*; the harness surfaces them, it does not gate the regular
  workspace test run. Also skips gracefully when neither `uu_<util>` nor a
  `coreutils` multicall binary is on `$PATH`.
- One `DiffFixture` per row (util, argv, stdin, files, optional
  `diff_reason` for documented divergences); adding a port is ~10 lines.
- Files materialize to a host tempdir for uutils and mount at the same
  virtual path in bashkit; `LC_ALL=C` on the host side (bashkit does not
  localize).

CI: `.github/workflows/ci.yml`'s `Test` job pre-installs the uutils
multicall (cached, `continue-on-error`) but does **not** set the env gate —
install is purely caching. The drift workflow (below) builds the multicall
from the *pinned* clone and runs the harness with the gate set, so body
drift surfaces in the same auto-PR as flag drift.

## Module mode

Args mode rewrites a single function (`uu_app()`). For library code worth
reusing wholesale — e.g. uucore's `format/` parser, which `printf.rs` would
otherwise reimplement — a second mode vendors entire uucore modules at port
time.

### When to use module mode vs args mode

| Need | Mode | Tool invocation |
|---|---|---|
| Reuse a uutils utility's flag surface (`uu_app()`) | args | `bashkit-coreutils-port <UUTILS_DIR> <UTIL> [<REV>]` |
| Reuse a platform-clean uucore library (e.g. `format/`) | module | `bashkit-coreutils-port port-module <UUTILS_DIR> <MODULE> [<REV>]` |

Module mode fits small, platform-clean modules whose imports are mostly
`std` + a few published crates, plus a bounded set of uucore-internal types.
A runtime dependency on `uucore` was rejected — ~98 s of cold build time,
breaks the WASM target (`uucore → rustix → errno`), and forces Fluent into
bashkit's runtime.

### Manifest

Vendored modules are declared in
`crates/bashkit-coreutils-port/vendored.toml` (the tool owns the manifest,
the drift workflow reads it). Each `[[modules]]` stanza declares one target:

```toml
[[modules]]
name = "format"                                # CLI lookup id
source = "src/uucore/src/lib/features/format"  # under <UUTILS_DIR>
out = "format"                                 # under generated/

[[modules.substitutions]]
prefix = "uucore::error::UError"
action = "error"
```

`out` is relative to `crates/bashkit/src/builtins/generated/`. `source` may
be a single `.rs` file or a directory (walked recursively, structure
mirrored).

### Substitution model

`port-module` walks every top-level `use` in each ported file, flattens
nested groups (`use a::{b, c}`), and classifies each path:

- **External or module-local** (not rooted at `uucore`/`crate`) passes
  through — `std`, `bigdecimal`, etc. resolve at bashkit compile time;
  `self::`/`super::` stay inside the vendored tree.
- **Fluent boundary** — `use fluent::*;`, `use uucore::translate;`,
  `uucore::i18n::*` are hard errors regardless of manifest: the module is
  not safely vendorable without code changes.
- **uucore-internal** must match a `[[modules.substitutions]]` prefix.
  Unmatched internal references abort the port — silent emission of a broken
  `use uucore::...` is rejected since it would not compile against bashkit's
  dep graph.

Substitution `action`s (all implemented):

| Action | Behaviour |
|---|---|
| `error` | Abort the port at this import (uucore type that should not be vendored). |
| `replace_with` | Rewrite the matched prefix in every `use` path to `target`; if the final segment changes, insert `as <orig>` so call sites compile unchanged. |
| `inline` | Vendor the file at `inline_source` next to the module's output dir (`<out_base>/<leaf>.rs`, `<leaf>` = prefix's final segment) and rewrite matching `use` paths to `crate::builtins::generated::<leaf>::…` so imports work from any nesting depth. The inlined file goes through the same enforce + rewrite pipeline, so transitive uucore references either substitute or surface explicitly. |

Output goes through `prettyplease::unparse` whenever any `replace_with` or
`inline` substitution is in scope, so use-groups may flatten into individual
`use` items. `use module::{self, Item}` normalizes to `use module;` +
`use module::Item;` so flattened relative imports remain valid Rust.
Top-level upstream `#[cfg(test)]` items and rustdoc attributes are stripped:
bashkit tests/docs cover the integrated behavior; upstream tests assume the
original uucore topology.

### Vendored Modules

| Module | uutils source | Output | Substitution decisions |
|---|---|---|---|
| `format` | `src/uucore/src/lib/features/format` | `crates/bashkit/src/builtins/generated/format/` plus `extendedbigdecimal.rs` and `num_parser.rs` siblings | `crate::format` self-refs rewrite to `crate::builtins::generated::format`; `extendedbigdecimal` and `parser::num_parser` are inlined; `NonUtf8OsStrError`, `os_str_as_bytes`, `UError`, `set_exit_code`, `quoting_style`, `show_error`, and `show_warning` rewrite to bashkit-local `format_support` shims. |

### Output banner

Module-mode files carry the same banner shape as args mode: `GENERATED by
bashkit-coreutils-port. DO NOT EDIT.`, source `uutils/coreutils@<rev>` +
relative path, regenerate command, MIT license pointer
(`THIRD_PARTY_LICENSES`).

## Source-of-truth uutils revision pin

`crates/bashkit/src/builtins/generated/mod.rs` declares
`pub const UUTILS_REVISION: &str = "<short-rev>"` — single source of truth
shared by the codegen tool, the body-drift harness (multicall built from the
same rev), and `just regen-coreutils-args` (checks out the local clone at
the pin). A static test
(`builtins/mod.rs::tests::generated_args_headers_match_pinned_uutils_revision`)
asserts every `<util>_args.rs` header references the same rev as the
constant, so partial regenerations that forget or mis-bump the pin fail in
CI. The drift workflow always resolves upstream HEAD to a concrete commit,
checks it out detached, and bumps `UUTILS_REVISION` together with the
regenerated files in one PR — the two never diverge across an auto-PR
boundary.

## CI guard

`.github/workflows/coreutils-args-drift.yml` runs weekly (Mondays 05:00 UTC)
and on `workflow_dispatch`:

1. Read-only job (checkout credential persistence disabled) checks out
   bashkit + uutils side-by-side, regenerates every `pub mod <util>_args;`
   entry and every `vendored.toml` module against resolved upstream HEAD,
   bumps `UUTILS_REVISION`, verifies bashkit builds and cat/tac spec tests
   pass, builds the uutils multicall and runs the differential harness with
   `BASHKIT_RUN_COREUTILS_DIFF=1`, then uploads a binary git patch for
   `generated/` if non-empty.
2. A separate write-scoped PR job checks out bashkit without persisted
   credentials, applies only generated-file changes from that patch, and
   opens or updates the drift PR with `gh`.

The read-only job is the only job that builds or executes uutils code. The
write-scoped job must not checkout, build, or execute uutils code, and must
not use third-party PR-creation actions. Module diffs and args diffs land in
the **same** auto-PR (one bot PR per drift run); adding a vendored module is
one TOML stanza and the next run picks it up.

The PR's intermediate commits are bot-authored (automated drift detection,
not a code change). Maintainers must **squash-merge as a human** so the
merge commit is attributed correctly per `AGENTS.md`. Reviewing the auto-PR
is part of the maintenance checklist — see `specs/maintenance.md`
§ Coreutils Argument-Surface Drift.

## Alternatives considered

- **Direct dep on `uu_*` crates** — rejected: forces Fluent init, drags `rustix`/`winapi-util`, breaks WASM, locks bashkit to uutils' clap major.
- **`build.rs` regenerating every build** — rejected: hides generated code from PR diffs, slows clean builds, bashkit avoids fetching at build time.
- **Manual port of each `uu_app()`** — rejected: ~100 utilities is too many for hand-translation, and uutils tracks GNU upstream changes we want to pull in.

## See also

- `specs/builtins.md` — `Builtin` trait, `ClapBuiltin`, command dispatch.
- `crates/bashkit-coreutils-port/src/main.rs` — codegen entry point; `args.rs` / `module.rs` host the two modes, `manifest.rs` the `vendored.toml` schema.
- `crates/bashkit-coreutils-port/vendored.toml` — vendored-module manifest.
- `crates/bashkit/src/builtins/cat.rs`, `textrev.rs` — port consumers.
