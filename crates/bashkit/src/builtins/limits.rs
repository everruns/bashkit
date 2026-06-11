//! Centralized DoS / resource caps for individual builtins.
//!
//! These constants are referenced by the threat model (TM-DOS-*).
//! Keeping them in one place makes auditing easier: every adjustment
//! lands in a single file, so a reviewer can read all per-builtin
//! limits without grepping the tree.
//!
//! Tunable runtime limits that are part of a public config surface
//! (e.g. `PythonLimits`, `TypeScriptLimits`, `SqliteLimits`, ssh
//! defaults) live on their own config types and are intentionally
//! NOT mirrored here.

/// Max width/precision for printf-style format specifiers to prevent
/// memory exhaustion. Shared by `printf` and `awk`.
pub(crate) const MAX_FORMAT_WIDTH: usize = 10_000;

/// archive: cap on decompression expansion ratio (zip-bomb guard).
pub(crate) const ARCHIVE_MAX_DECOMPRESSION_RATIO: usize = 100;

/// awk: max parser recursion depth.
pub(crate) const AWK_MAX_PARSER_DEPTH: usize = 100;
/// awk: max user-function call depth at runtime.
pub(crate) const AWK_MAX_CALL_DEPTH: usize = 64;
/// awk: total output byte cap per invocation.
pub(crate) const AWK_MAX_OUTPUT_BYTES: usize = 10_000_000;
/// awk: max distinct output redirection targets per invocation.
pub(crate) const AWK_MAX_OUTPUT_TARGETS: usize = 1_024;
/// awk: max distinct files held open by `getline < file`.
pub(crate) const AWK_MAX_GETLINE_CACHED_FILES: usize = 100;
/// awk: max bytes read from one `getline < file` input.
pub(crate) const AWK_MAX_GETLINE_FILE_BYTES: usize = 10_000_000;
/// awk: max total bytes retained by all `getline < file` inputs.
pub(crate) const AWK_MAX_GETLINE_CACHE_BYTES: usize = 10_000_000;

/// curl: max number of HTTP redirects to follow.
#[cfg(feature = "http_client")]
pub(crate) const CURL_MAX_REDIRECTS: u32 = 10;
/// curl: max request body bytes for `-d`, `-d @-`, `-d @file`, and multipart assembly.
#[cfg(feature = "http_client")]
pub(crate) const CURL_MAX_REQUEST_BODY_BYTES: usize = 10_000_000;

/// expand/unexpand: max accepted tab stop width.
pub(crate) const EXPAND_MAX_TAB_STOP: usize = 10_000;
/// expand: max output bytes per invocation before interpreter-level truncation.
pub(crate) const EXPAND_MAX_OUTPUT_BYTES: usize = 1_048_576;

/// dirs/pushd/popd: max entries on the directory stack.
pub(crate) const DIRSTACK_MAX_SIZE: usize = 4096;

/// find: total stdout cap for default and `-printf` output.
pub(crate) const FIND_MAX_OUTPUT_BYTES: usize = 1_048_576;

/// mktemp: max name-collision retries before giving up.
pub(crate) const MKTEMP_MAX_ATTEMPTS: usize = 64;

/// numfmt: total output / padding / precision cap.
pub(crate) const NUMFMT_MAX_OUTPUT_BYTES: usize = 1_048_576;

/// parallel: cap on Cartesian product expansion.
pub(crate) const PARALLEL_MAX_CARTESIAN_PRODUCT: usize = 100_000;

/// printf: max diagnostic message length.
pub(crate) const PRINTF_MAX_DIAG_CHARS: usize = 1_024;

/// retry: max retry attempts.
pub(crate) const RETRY_MAX_ATTEMPTS: u32 = 10_000;

/// sed: max group-nesting depth in `s` replacements.
pub(crate) const SED_MAX_GROUP_NESTING_DEPTH: usize = 128;

/// sleep: max sleep duration.
pub(crate) const SLEEP_MAX_SECONDS: f64 = 60.0;

/// template: max template-expansion recursion depth.
pub(crate) const TEMPLATE_MAX_DEPTH: usize = 100;

/// timeout: max timeout duration in seconds (5 minutes).
pub(crate) const TIMEOUT_MAX_SECONDS: u64 = 300;

/// yaml: max nesting depth.
pub(crate) const YAML_MAX_DEPTH: usize = 100;

/// yes: max lines and total output bytes per invocation.
pub(crate) const YES_MAX_LINES: usize = 10_000;
pub(crate) const YES_MAX_OUTPUT_BYTES: usize = 1_048_576;
