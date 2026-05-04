# Coreutils argument-surface port

## Status
POC

## Decision

Reuse uutils/coreutils' clap argument definitions in bashkit by **port-time
codegen**, not by depending on `uu_*` crates at runtime.

`crates/bashkit-coreutils-port/` is a small standalone binary that:

1. Parses `<uutils>/src/uu/<util>/src/<util>.rs` with `syn`.
2. Reads `<uutils>/src/uu/<util>/locales/en-US.ftl` for help/about strings.
3. Rewrites the `uu_app()` AST in place:
   - `translate!("k")` → `String::from("<value from ftl>")`
   - `uucore::crate_version!()` → `env!("CARGO_PKG_VERSION")`
   - `uucore::format_usage(x)` → local `format_usage` shim
   - `.help_template(uucore::localized_help_template(...))` → chain step elided
4. Emits a generated file under
   `crates/bashkit/src/builtins/generated/<util>_args.rs` with a clean
   `pub fn <util>_command() -> clap::Command`.

bashkit's `Builtin::execute` calls `<util>_command().try_get_matches_from(...)`
and implements behaviour against the VFS. The `clap-builtins` feature gates the
new path; the legacy hand-parsers remain as a fallback for
`--no-default-features` consumers.

Help template is overridden in the calling builtin (e.g. `cat.rs`) to put the
`Usage:` line first, matching GNU coreutils' layout.

## Rationale

The uu_* crates expose `uu_app()` as their canonical clap definition, but
they:

- Hardcode `std::fs` / `io::stdin()` / `io::stdout()` (incompatible with VFS).
- Are sync (incompatible with bashkit's tokio-async builtins).
- Resolve every help/about string through Fluent at runtime.
- Pull `rustix` / `winapi-util` (hostile to WASM).

A runtime dep would force Fluent init and locale bundles into bashkit. A
build-time `build.rs` would either vendor uutils as a submodule or fetch it
during every clean build, both of which violate bashkit norms (build does
not fetch; generated artifacts are not in `target/`). Codegen run via a
binary, with output committed, gives reviewability, grep-ability, and
predictable build times — at the cost of needing to re-run the recipe on
every uutils bump (CI guard recommended below).

## Verification

POC ports `cat` and `tac`:

- `crates/bashkit/src/builtins/generated/cat_args.rs`
- `crates/bashkit/src/builtins/generated/tac_args.rs`

Spec tests:

- `tests/spec_cases/bash/cat.test.sh` — covers `-n`, `-b`, `-E`, `-s`, `-ns`,
  `-A`, `-` (stdin), and rejection of unknown flags.
- `tests/spec_cases/bash/textrev.test.sh` — adds tac unknown-flag and
  parser-accepts-but-unimplemented (`-s`) cases on top of the existing
  reverse-line tests.
- `tests/spec_cases/bash/help-flag.test.sh` — `cat_help` now matches GNU's
  `Usage:`-first layout.

Regenerate with `just regen-coreutils-args` (or run the per-util command
directly):

```bash
cargo run -p bashkit-coreutils-port -- /tmp/uutils cat <REV> \
    > crates/bashkit/src/builtins/generated/cat_args.rs
```

## Scaling

Each new utility port is three steps:

1. `just regen-coreutils-args` (extend the for-loop to add the util).
2. Add `pub mod <util>_args;` to
   `crates/bashkit/src/builtins/generated/mod.rs`.
3. In the matching builtin, replace handwritten parsing with
   `<util>_command().try_get_matches_from(...)` and read the boolean/value
   flags.

The codegen tool handles every uutils utility whose `uu_app()` follows the
common shape (`Command::new(...)` chain, `mod options`, flat `en-US.ftl`).
Ports that need bespoke transforms (e.g. utils with no `mod options`, or
help strings using Fluent placables/selectors) currently fail with an
`unresolved translate!()` error rather than emitting silently-wrong code.

## Suggested CI guard

A nightly job that runs `just regen-coreutils-args` and fails if
`git diff --exit-code` is non-empty surfaces unexpected upstream changes
(both new flags we should pick up and removed flags we should drop) without
breaking on every uutils commit.

## Alternatives considered

- **Direct dep on `uu_*` crates** — rejected: forces Fluent init, drags
  `rustix`/`winapi-util`, breaks WASM, and locks bashkit to uutils' clap
  major version.
- **`build.rs` regenerating every build** — rejected: hides generated code
  from PR diffs, slows clean builds, and bashkit avoids fetching at build
  time.
- **Manual port of each `uu_app()`** — rejected: the user requested an
  automated approach. ~100 utilities is too many for hand-translation, and
  uutils tracks GNU upstream changes that we'd want to pull in.

## See also

- `specs/builtins.md` — `Builtin` trait, `ClapBuiltin`, command dispatch.
- `crates/bashkit-coreutils-port/src/main.rs` — codegen implementation.
- `crates/bashkit/src/builtins/cat.rs`, `textrev.rs` — port consumers.
