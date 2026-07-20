// GENERATED MODULE INDEX. Edit individual `*_args.rs` files only via the
// `bashkit-coreutils-port` codegen tool — see crates/bashkit-coreutils-port.
#![allow(dead_code)]
//
// Each `<util>_args.rs` exposes `pub fn <util>_command() -> clap::Command`
// derived from uutils/coreutils' `uu_app()` definitions, with `translate!()`
// keys resolved against the corresponding `en-US.ftl`.

/// Pinned uutils/coreutils revision used by:
///
/// 1. The `bashkit-coreutils-port` codegen tool (drift workflow checks
///    out uutils at this rev before regenerating `<util>_args.rs`).
/// 2. The `coreutils_differential_tests` body-drift harness (drift
///    workflow builds the `coreutils` multicall from this same rev
///    before running the harness with `BASHKIT_RUN_COREUTILS_DIFF=1`).
///
/// Single source of truth: when this constant moves, both the args
/// surfaces and the body-drift gate move with it. Bumping is normally
/// the work of the auto-PR opened by `coreutils-args-drift.yml`; manual
/// bumps require regenerating every `<util>_args.rs` file at the new
/// rev so headers stay aligned with this constant. The static test
/// `builtins::tests::generated_args_headers_match_pinned_uutils_\
/// revision` enforces that invariant.
//
// The constant is read at port time (codegen, drift workflow, justfile)
// and at test time (the invariant assertion). Nothing in the lib's
// runtime path needs it, hence `allow(dead_code)`.
#[allow(dead_code)]
pub const UUTILS_REVISION: &str = "4106bfaf6";

pub mod cat_args;
#[allow(clippy::collapsible_if, clippy::unwrap_used)]
pub mod extendedbigdecimal;
#[allow(clippy::collapsible_if, clippy::unwrap_used)]
pub mod format;
pub mod format_support;
pub mod ls_args;
pub mod mktemp_args;
#[allow(clippy::collapsible_if, clippy::unwrap_used)]
pub mod num_parser;
pub mod od_args;
pub mod readlink_args;
pub mod realpath_args;
pub mod shuf_args;
pub mod stat_args;
pub mod tac_args;
pub mod tee_args;
pub mod truncate_args;
