# Coreutils argument-surface port

## Status
Active (`cat`, `tac`, `ls`, `shuf`, `readlink`, `truncate`)

## Decision

Reuse uutils/coreutils' clap argument definitions in bashkit by **port-time
codegen**, not by depending on `uu_*` crates at runtime.

`crates/bashkit-coreutils-port/` is a small standalone binary that:

1. Parses `<uutils>/src/uu/<util>/src/<util>.rs` with `syn`.
2. Falls back to scanning sibling `.rs` files (e.g. `ls/src/config.rs`) when
   the `mod options` it needs lives next to `<util>.rs` rather than inside it.
3. Reads `<uutils>/src/uu/<util>/locales/en-US.ftl` for help/about strings.
4. Rewrites the `uu_app()` AST in place:
   - `translate!("k")` → `String::from("<value from ftl>")`
   - `uucore::crate_version!()` → `env!("CARGO_PKG_VERSION")`
   - `uucore::format_usage(x)` → local `format_usage` shim
   - `.help_template(uucore::localized_help_template(...))` → chain step elided
   - `uucore::clap_localization::configure_localized_command(cmd)` → `cmd`
   - `ShortcutValueParser::new([…])` → `clap::builder::PossibleValuesParser::new([…])`
     (loses uucore's unambiguous-abbreviation behaviour; documented divergence)
   - `Arg::…env("FOO")…` → chain step elided AND harvested into a
     sidecar table (TM-INF-024). uutils attaches `.env(...)` to options
     like `TABSIZE`/`TIME_STYLE` so they pick up host process state;
     bashkit sandboxes scripts inside `ctx.env`, so the generated
     `<util>_command()` only consults argv. To preserve uutils' UX
     across the port, codegen records each stripped `.env(...)` into
     `pub static <UTIL>_ENV_DEFAULTS: &[clap_env::EnvDefault]` next to
     the command builder. Each row carries `(arg_id, long, env_var,
     kind ∈ {Single, Bool, Multi})`. The bashkit-side shim
     `crate::builtins::clap_env::apply_env_defaults` reads
     `<UTIL>_ENV_DEFAULTS` plus the caller's `ctx.env` and synthesises
     `--<long> <value>` (or `--<long>` for `Bool`) into argv before
     `try_get_matches_from`, emulating clap's documented "argv > env >
     default" precedence — but sourced from the sandbox, never
     `std::env`. Defence-in-depth: the workspace `clap` dep drops the
     `env` cargo feature, `builtins::tests::no_clap_env_in_generated_parsers`
     statically forbids runtime `.env(` calls in `generated/*.rs`, and
     `every_generated_parser_emits_env_defaults_table` enforces the
     uniform sidecar surface (every util emits the table, possibly
     empty). Per-builtin opt-in: a builtin chooses whether to wire
     through the shim — if it does, every uutils env-default
     auto-lights as that option's bashkit support lands.
5. Emits a generated file under
   `crates/bashkit/src/builtins/generated/<util>_args.rs` with a clean
   `pub fn <util>_command() -> clap::Command`.

bashkit's `Builtin::execute` calls `<util>_command().try_get_matches_from(...)`
and implements behaviour against the VFS. `clap` is an unconditional
dependency of `bashkit` — there is no feature flag for the ported path or
the `ClapBuiltin` trait.

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

## Verification — Differential tests

The args workflow above only catches **flag-signature drift**: it
regenerates `<util>_args.rs` and surfaces a diff if uutils added,
removed, or renamed flags. It cannot see **body drift** — semantic
divergence inside `cat.rs` / `textrev.rs` against GNU/uutils.

`crates/bashkit/tests/coreutils_differential_tests.rs` closes that gap:
for each fixture row it runs the same `<util> <args>` (with the same
stdin and the same input files) through bashkit and through the
matching uutils binary, then asserts byte-for-byte stdout parity plus
exit-code parity.

Pattern reference: `crates/bashkit/tests/sqlite_differential_tests.rs`.

Properties:

- One `DiffFixture` per row (util, argv, stdin, files, optional
  `diff_reason` for documented divergences). Adding a port is ~10
  lines: a new fixture row.
- **Opt-in.** The harness skips with a notice unless
  `BASHKIT_RUN_COREUTILS_DIFF=1` is set. Body divergences between
  bashkit and uutils are *expected* — the harness's purpose is to
  surface them, not to gate the regular workspace test run on them.
- After the env gate, also skips gracefully when neither `uu_<util>`
  nor a `coreutils` multicall binary is on `$PATH` — same UX as the
  sqlite harness.
- Files are materialized to a host tempdir for the uutils side and
  mounted at the same virtual path in bashkit, so both engines receive
  the same `<file>` argument.
- `LC_ALL=C` for the host side; bashkit currently does not localize.

CI integration:

- `.github/workflows/ci.yml`'s `Test` job pre-installs the uutils
  multicall via `taiki-e/install-action@v2` (cached, with
  `continue-on-error`). It does **not** set
  `BASHKIT_RUN_COREUTILS_DIFF`, so the harness still skips here —
  install is purely caching for downstream jobs.
- `.github/workflows/coreutils-args-drift.yml` builds the multicall
  from the *pinned* uutils clone, sets
  `BASHKIT_RUN_COREUTILS_DIFF=1`, and runs the harness so body drift
  surfaces in the same auto-PR as flag drift.

## Source-of-truth uutils revision pin

`crates/bashkit/src/builtins/generated/mod.rs` declares
`pub const UUTILS_REVISION: &str = "<short-rev>"`. This is the single
source of truth shared by:

- The codegen tool (drift workflow checks out uutils at this rev
  before regenerating `<util>_args.rs`).
- The body-drift harness (drift workflow builds the `coreutils`
  multicall from the same rev).
- `just regen-coreutils-args` (reads the pin and checks out the local
  uutils clone at it before regenerating).

A static test in `builtins/mod.rs::tests::generated_args_headers_\
match_pinned_uutils_revision` asserts every `<util>_args.rs` header
references the same rev as the constant. Manual partial-regenerations
that forget to bump (or mis-bump) the pin fail in CI.

The drift workflow always runs against upstream HEAD and bumps
`UUTILS_REVISION` together with the regenerated files in one PR — the
two never diverge across an auto-PR boundary.

## CI guard

`.github/workflows/coreutils-args-drift.yml` runs weekly (Mondays 05:00 UTC)
and on `workflow_dispatch`. It:

1. Checks out bashkit and `uutils/coreutils` side-by-side.
2. Reads the current pin from `generated/mod.rs` and checks out uutils
   at upstream HEAD for the regen.
3. Runs `bashkit-coreutils-port` against every `pub mod <util>_args;` line in
   `crates/bashkit/src/builtins/generated/mod.rs` and bumps
   `UUTILS_REVISION` to the rev it just generated against.
4. Verifies bashkit still builds and the cat/tac spec tests pass.
5. Builds the uutils multicall from the same checkout and runs the
   differential harness with `BASHKIT_RUN_COREUTILS_DIFF=1`.
6. Opens a PR with the regenerated files + bumped pin if `git diff` is
   non-empty.

The PR's intermediate commits are bot-authored (this is automated drift
detection, not a code change). Maintainers must **squash-merge as a human**
so the merge commit is attributed correctly per `AGENTS.md`.

Reviewing the auto-PR is part of the maintenance checklist — see
`specs/maintenance.md` § Coreutils Argument-Surface Drift.

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
