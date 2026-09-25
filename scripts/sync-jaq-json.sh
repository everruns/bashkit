#!/usr/bin/env bash
# Sync the vendored jaq-json copy in crates/bashkit/src/builtins/jq/jaq_json/
# with an upstream release. See knowledge/runtimes/jaq-json-vendor.md.
#
#   scripts/sync-jaq-json.sh --check        # report if a newer release exists
#   scripts/sync-jaq-json.sh <version>      # merge upstream <version> in
#   scripts/sync-jaq-json.sh --pristine <version> <dir>
#                                           # write the mechanically adapted,
#                                           # unpatched upstream copy to <dir>
#
# Decision: the vendored files are upstream + a fixed set of mechanical
# rewrites (crate -> module paths, cargo features, macro scoping; done by
# `adapt` below) + bashkit patches marked `BASHKIT PATCH`. A sync diffs the
# adapted old release against the adapted new release and applies that diff
# onto the vendored copy, so bashkit patches survive and conflicts show up as
# `.rej` files for review.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/crates/bashkit/src/builtins/jq/jaq_json"
VERSION_FILE="$DEST/UPSTREAM_VERSION"
CRATE=jaq-json

fetch() { # <version> <dir>: download and unpack the published crate
  local version=$1 dir=$2
  mkdir -p "$dir"
  curl -fsSL "https://static.crates.io/crates/$CRATE/$CRATE-$version.crate" |
    tar -xz -C "$dir" --strip-components=1
}

adapt() { # <src crate dir> <out dir>: upstream layout -> bashkit module
  local src=$1 out=$2
  mkdir -p "$out/tests"
  cp "$src/src/lib.rs" "$out/mod.rs"
  for f in funs num read write serde; do cp "$src/src/$f.rs" "$out/$f.rs"; done
  cp "$src/src/defs.jq" "$out/defs.jq"
  cp "$src/tests/common/mod.rs" "$out/tests/common.rs"
  cp "$src/tests/defs.rs" "$out/tests/defs.rs"
  cp "$src/tests/funs.rs" "$out/tests/funs.rs"

  # Crate-level attributes do not apply to a module.
  sed -i -e '/^#!\[no_std\]$/d' -e '/^#!\[warn(missing_docs)\]$/d' "$out/mod.rs"
  # Cargo features: std is always on, sync never, serde only for the
  # upstream tests.
  sed -i -e '/^ *#\[cfg(feature = "std")\]$/d' \
    -e '/^ *#\[cfg(not(feature = "sync"))\]$/d' \
    -e 's/#\[cfg(feature = "sync")\]/#[cfg(any())]/' \
    -e 's/#\[cfg(feature = "serde")\]/#[cfg(test)]/' \
    "$out"/*.rs
  sed -i -e 's/serde_core::/serde::/g' "$out/serde.rs"
  # `alloc` is not in scope outside a no_std crate root; std re-exports it.
  sed -i -e '/^extern crate \(alloc\|std\);$/d' -e 's/\balloc::/std::/g' "$out"/*.rs
  # Crate paths -> module paths.
  sed -i -e 's/\bcrate::/self::/g' "$out/mod.rs"
  sed -i -e 's/\bcrate::/super::/g' "$out/funs.rs" "$out/read.rs" "$out/serde.rs" "$out/num.rs"
  # write.rs macros: no #[macro_export] (would leak into bashkit's public
  # API); refer to macros textually and to items by full module path.
  sed -i -e '/^#\[macro_export\]$/d' \
    -e 's/\$crate::\(write_byte\|write_utf8\|write_bytes\|write_seq\|style\|format_val\|write_val\)!/\1!/g' \
    -e 's/\$crate::/$crate::builtins::jq::jaq_json::/g' \
    -e 's/\bcrate::Val\b/super::Val/g' \
    "$out/write.rs"
  # Upstream integration tests -> unit tests of this module.
  sed -i -e '/^#\[macro_export\]$/d' -e 's/\bjaq_json::/crate::builtins::jq::jaq_json::/g' \
    -e 's/^pub mod common;$/use super::common;/' \
    -e 's/^use common::/use super::common::/' \
    -e 's/\$crate::common::/super::common::/' \
    "$out"/tests/*.rs
}

current_version() { tr -d '[:space:]' <"$VERSION_FILE"; }

latest_version() {
  curl -fsSL -H "User-Agent: bashkit-maintenance" "https://crates.io/api/v1/crates/$CRATE" |
    python3 -c 'import json,sys; print(json.load(sys.stdin)["crate"]["max_stable_version"])'
}

case "${1:-}" in
--check)
  cur=$(current_version)
  latest=$(latest_version)
  if [ "$cur" = "$latest" ]; then
    echo "$CRATE vendored at $cur, up to date"
  else
    echo "$CRATE vendored at $cur, upstream has $latest: run scripts/sync-jaq-json.sh $latest"
    exit 1
  fi
  ;;
--pristine)
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  fetch "$2" "$tmp/src"
  adapt "$tmp/src" "$3"
  ;;
[0-9]*)
  new=$1
  old=$(current_version)
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  fetch "$old" "$tmp/old-src"
  fetch "$new" "$tmp/new-src"
  adapt "$tmp/old-src" "$tmp/a"
  adapt "$tmp/new-src" "$tmp/b"
  (cd "$tmp" && diff -ruN a b >upstream.diff) || true
  if [ ! -s "$tmp/upstream.diff" ]; then
    echo "no source changes between $old and $new"
  elif ! patch -d "$DEST" -p1 --no-backup-if-mismatch <"$tmp/upstream.diff"; then
    echo "upstream diff did not apply cleanly; resolve the .rej files in $DEST" >&2
    echo "$new" >"$VERSION_FILE"
    exit 1
  fi
  echo "$new" >"$VERSION_FILE"
  echo "synced $CRATE $old -> $new; also check its Cargo.toml dependency versions"
  ;;
*)
  sed -n '2,10p' "$0"
  exit 2
  ;;
esac
