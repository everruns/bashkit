#!/usr/bin/env bash
# Debian-oracle scoreboard: run the pseudo-linus test bench against bashkit.
#
# The bench (https://github.com/JoaoHenriqueBarbosa/pseudo-linus, MIT) holds
# command cases with outputs recorded on real Debian. Some suites in its
# corpus are derived from GPL test suites (GNU grep/sed), so it is fetched at
# run time at a pinned commit, never vendored into this repository.
#
# Usage: run.sh [--update-floors] [TOOL...]
#   Prints a markdown table (also appended to $GITHUB_STEP_SUMMARY when set)
#   and fails when a tool's lenient pass count drops more than TOLERANCE cases
#   below scripts/debian-oracle/floors.tsv. --update-floors rewrites the floors
#   from this run. Failing cases land in $ORACLE_DIR/fails.txt.
set -euo pipefail

PSEUDO_LINUS_REPO="https://github.com/JoaoHenriqueBarbosa/pseudo-linus"
PSEUDO_LINUS_REV="039036aa8148a6802afe10663c9253e059241b45"
# One case of slack per tool for wall-clock timeouts on slow runners.
TOLERANCE=1

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
ORACLE_DIR="${ORACLE_DIR:-$ROOT/target/debian-oracle}"
SRC="$ORACLE_DIR/pseudo-linus"
FLOORS="$HERE/floors.tsv"

update=0
if [[ "${1:-}" == "--update-floors" ]]; then
  update=1
  shift
fi

mkdir -p "$ORACLE_DIR"
if [[ "$(git -C "$SRC" rev-parse HEAD 2>/dev/null)" != "$PSEUDO_LINUS_REV" ]]; then
  rm -rf "$SRC"
  git init -q "$SRC"
  git -C "$SRC" fetch -q --depth 1 "$PSEUDO_LINUS_REPO" "$PSEUDO_LINUS_REV"
  git -C "$SRC" checkout -q FETCH_HEAD
fi

TB="$SRC/testbench"
rm -rf "$TB/crates/bk-oracle"
cp -r "$HERE/adapter" "$TB/crates/bk-oracle"
sed -i "s|@BASHKIT@|$ROOT/crates/bashkit|" "$TB/crates/bk-oracle/Cargo.toml"

# The bench's .cargo/config.toml links with mold, which runners lack.
rm -f "$SRC/.cargo/config.toml"
(cd "$TB" && CARGO_TARGET_DIR="$ORACLE_DIR/target" \
  cargo build -q --release -p bk-oracle --ignore-rust-version)

scores="$ORACLE_DIR/scores.tsv"
(cd "$TB" && FAILS_OUT="$ORACLE_DIR/fails.txt" "$ORACLE_DIR/target/release/bk-oracle" "$@") > "$scores"

report="$ORACLE_DIR/report.md"
awk -F'\t' -v floors="$FLOORS" -v tol="$TOLERANCE" '
  BEGIN {
    while ((getline line < floors) > 0) {
      if (line ~ /^#/ || line == "") continue
      split(line, f, "\t"); floor[f[1]] = f[2]
    }
    print "| Tool | Cases | Strict | Lenient | Floor |"
    print "|------|------:|-------:|--------:|------:|"
  }
  {
    n += $2; s += $3; l += $4
    fl = ($1 in floor) ? floor[$1] : "-"
    mark = ""
    if (fl != "-" && $4 < fl - tol) { mark = " **regressed**"; bad = 1 }
    else if (fl != "-" && $4 > fl) { mark = " (above floor)" }
    printf "| %s | %d | %.1f%% | %.1f%% (%d) | %s%s |\n", $1, $2, 100*$3/($2?$2:1), 100*$4/($2?$2:1), $4, fl, mark
  }
  END {
    printf "| **Total** | %d | %.1f%% | %.1f%% | |\n", n, 100*s/(n?n:1), 100*l/(n?n:1)
    exit bad
  }
' "$scores" > "$report" && status=0 || status=$?

cat "$report"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  {
    echo "## Debian-oracle scoreboard"
    echo
    echo "pseudo-linus \`${PSEUDO_LINUS_REV:0:12}\`, lenient = output match ignoring known noise."
    echo
    cat "$report"
  } >> "$GITHUB_STEP_SUMMARY"
fi

if (( update )); then
  {
    echo "# tool<TAB>minimum lenient passes (scripts/debian-oracle/run.sh --update-floors)"
    cut -f1,4 "$scores"
  } > "$FLOORS"
  echo "floors updated: $FLOORS"
  exit 0
fi

if (( status != 0 )); then
  echo "::error::Debian-oracle score regressed below scripts/debian-oracle/floors.tsv; failing cases in $ORACLE_DIR/fails.txt" >&2
  exit 1
fi
