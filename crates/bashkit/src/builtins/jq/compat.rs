//! jq-compatibility shim definitions prepended to every filter.
//!
//! jaq is ~95% jq-compatible but its stdlib has gaps. We patch them here
//! so LLM-generated filters that target real jq compile and produce
//! the expected output.
//!
//! Important decisions:
//!  - `setpath` / `leaf_paths`: not in jaq stdlib; reimplemented in jq syntax.
//!  - `match` / `scan`: jaq's defaults differ subtly from jq; we override.
//!    For `scan`, real jq's `scan(re; flags)` with empty flags is global by
//!    default — we explicitly add `g` if not already present in `flags` to
//!    avoid a double-`g` regex compile error from `"g" + "g"`.
//!  - `@tsv` / `@csv`: jaq-std doesn't define them. Strict variants reject
//!    non-scalars with a runtime error matching real jq's wording.
//!  - `input_filename` / `input_line_number`: bashkit threads these as
//!    global variables (`$__bashkit_filename__`, `$__bashkit_lineno__`)
//!    that the main loop sets per-input. Real jq tracks per-file state;
//!    our stubs return `null` / `0` when stdin is the source.
//!  - `env` / `$ENV`: bound to a synthesized env object built from
//!    `ctx.env + ctx.variables`, never the host process env. (TM-INF-013)

/// Internal global variable name used to pass shell env to jq's `env` filter.
/// SECURITY: Replaces std::env::set_var() which was thread-unsafe and leaked
/// host process env vars (TM-INF-013).
pub(super) const ENV_VAR_NAME: &str = "$__bashkit_env__";

/// Public `$ENV` variable name. Real jq exposes the environment as both the
/// `env` function and the `$ENV` variable; bashkit binds both.
pub(super) const PUBLIC_ENV_VAR_NAME: &str = "$ENV";

/// Internal per-input filename, set by the main loop. Mirrors jq's
/// `input_filename` builtin which returns null for stdin.
pub(super) const FILENAME_VAR_NAME: &str = "$__bashkit_filename__";

/// Internal per-input line number. jq's `input_line_number` returns 0
/// when no input has been consumed.
pub(super) const LINENO_VAR_NAME: &str = "$__bashkit_lineno__";

/// `$ARGS` global, populated from `--args` / `--jsonargs` / `--arg` / `--argjson`.
pub(super) const ARGS_VAR_NAME: &str = "$ARGS";

/// Compat definitions prepended to every user filter. Order matters: each
/// def must precede any other def that references it (jaq resolves
/// top-down). The trailing `;` after every def is required.
pub(super) const JQ_COMPAT_DEFS: &str = r#"
def setpath(p; v):
  if (p | length) == 0 then v
  else p[0] as $k |
    (if . == null then
      if ($k | type) == "number" then [] else {} end
    else . end) |
    .[$k] |= setpath(p[1:]; v)
  end;
def leaf_paths: paths(scalars);
def match(re; flags):
  matches(re; flags)[] |
  .[0] as $m |
  { offset: $m.offset, length: $m.length, string: $m.string,
    captures: [.[1:][] | { offset: .offset, length: .length, string: .string,
    name: (if has("name") then .name else null end) }] };
def match(re): match(re; "");
def scan(re; flags):
  matches(re; if (flags | test("g")) then flags else "g" + flags end)[]
  | .[0].string;
def scan(re): scan(re; "");
def @tsv:
  [.[] |
    if type == "string" then
      (split("\\") | join("\\\\"))
      | (split("\t") | join("\\t"))
      | (split("\r") | join("\\r"))
      | (split("\n") | join("\\n"))
    elif type == "number" or type == "boolean" then tostring
    elif . == null then ""
    else error("\(type) (\(tojson)) is not valid in a tsv row")
    end
  ] | join("\t");
def @csv:
  [.[] |
    if type == "string" then
      "\"" + (split("\"") | join("\"\"")) + "\""
    elif type == "number" or type == "boolean" then tostring
    elif . == null then ""
    else error("\(type) (\(tojson)) is not valid in a csv row")
    end
  ] | join(",");
"#;

/// Build the prepended-defs prefix that wires `env`, `$ENV`,
/// `input_filename`, and `input_line_number` to internal globals.
pub(super) fn build_compat_prefix() -> String {
    format!(
        "{JQ_COMPAT_DEFS}\n\
         def env: {ENV_VAR_NAME};\n\
         def input_filename: {FILENAME_VAR_NAME};\n\
         def input_line_number: {LINENO_VAR_NAME};\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_compat_prefix_includes_setpath() {
        let prefix = build_compat_prefix();
        assert!(prefix.contains("def setpath"));
    }

    #[test]
    fn build_compat_prefix_includes_env_def() {
        let prefix = build_compat_prefix();
        assert!(prefix.contains("def env: $__bashkit_env__"));
    }

    #[test]
    fn build_compat_prefix_includes_filename_def() {
        let prefix = build_compat_prefix();
        assert!(prefix.contains("def input_filename: $__bashkit_filename__"));
    }

    #[test]
    fn build_compat_prefix_includes_lineno_def() {
        let prefix = build_compat_prefix();
        assert!(prefix.contains("def input_line_number: $__bashkit_lineno__"));
    }

    #[test]
    fn tsv_rejects_arrays_in_compat_def() {
        // The compat-def text must include the explicit error for non-scalars.
        assert!(JQ_COMPAT_DEFS.contains("is not valid in a tsv row"));
    }

    #[test]
    fn csv_rejects_arrays_in_compat_def() {
        assert!(JQ_COMPAT_DEFS.contains("is not valid in a csv row"));
    }

    #[test]
    fn scan_avoids_double_g_in_compat_def() {
        // The compat-def must check for existing 'g' in flags before prepending.
        assert!(JQ_COMPAT_DEFS.contains(r#"flags | test("g")"#));
    }
}
