#!/usr/bin/env bash
#
# Forbid Rust Debug formatting (`{:?}`, `{e:?}`, `{:#?}`) in builtin source.
#
# Why: real-shell tools print short, opaque error messages. Debug output
# dumps internal struct shapes (jaq's `File { code: ... }`, prepended
# compat-defs source, env var names) into stderr where LLM agents see
# them. Use Display (`{}`) or a domain-specific formatter instead.
#
# Per-line opt-out for legitimate cases (assert-failure messages,
# panics, intentional dev-only logging):
#
#     "got: {:?}", x  // debug-ok: assert-failure message
#
# Wired into `just pre-pr` and the .github/workflows/ci.yml lint job.

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
SCAN_DIRS=("$ROOT/crates/bashkit/src/builtins")

# Match `{:?}`, `{:#?}`, `{name:?}`, `{name:#?}`.
PATTERN='\{[A-Za-z0-9_]*:#?\?\}'

# Find candidate violations, drop:
#   - per-line `// debug-ok:` opt-outs
#   - lines that are #[derive(... Debug ...)] — that's defining Debug, not using {:?}
#   - lines whose comment-form is just documenting the pattern (e.g. doc tests)
violations=$(grep -rEn "$PATTERN" "${SCAN_DIRS[@]}" 2>/dev/null \
  | grep -v '// debug-ok:' \
  | grep -v '#\[derive(' \
  || true)

if [[ -n "$violations" ]]; then
  cat >&2 <<'EOF'
error: Debug formatting (`{:?}`, `{:#?}`, `{name:?}`) found in builtin
source. This leaks internal struct shapes into stderr.

Use Display (`{}`) or a domain-specific formatter. If this use is
legitimate (e.g. an assert-failure message inside `#[cfg(test)]`), add
a `// debug-ok: <reason>` comment on the same line.

Violations:
EOF
  echo "$violations" >&2
  exit 1
fi

echo "ok: no Debug formatting found in builtin source"
