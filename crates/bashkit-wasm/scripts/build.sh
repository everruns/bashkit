#!/usr/bin/env bash
# Build the browser wasm bundle for @everruns/bashkit-wasm.
#
# Produces an ES-module package under `pkg/` via `wasm-bindgen --target web`,
# which needs NO bundler and NO cross-origin isolation headers to load.
#
# Requires: rustup target add wasm32-unknown-unknown; bash scripts/install-wasm-bindgen.sh
# Optional (smaller output): wasm-opt (binaryen) on PATH.
set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKSPACE_ROOT="$(cd "$CRATE_DIR/../.." && pwd)"
OUT_DIR="$CRATE_DIR/pkg"
TARGET=wasm32-unknown-unknown
PROFILE="${1:-wasm-release}"
# Cargo features, comma-separated. The npm package ships `terminal` (the
# `Terminal` class, ~60 KB gzipped); set BASHKIT_WASM_FEATURES="" to drop it.
FEATURES="${BASHKIT_WASM_FEATURES-terminal}"

cd "$WORKSPACE_ROOT"
# Keep `release` as the workflow-facing spelling; both CI and publication use
# the same dedicated profile. Reject typos instead of silently shipping debug.
case "$PROFILE" in
  release|wasm-release) PROFILE=wasm-release; TARGET_SUBDIR=wasm-release ;;
  dev|debug) PROFILE=dev; TARGET_SUBDIR=debug ;;
  *) echo "unsupported profile: $PROFILE" >&2; exit 1 ;;
esac
echo "==> cargo build ($PROFILE) for $TARGET"
# shellcheck disable=SC2086
cargo build --locked -p bashkit-wasm --target "$TARGET" --profile "$PROFILE" ${FEATURES:+--features "$FEATURES"}

WASM_IN="${CARGO_TARGET_DIR:-$WORKSPACE_ROOT/target}/$TARGET/$TARGET_SUBDIR/bashkit_wasm.wasm"

echo "==> wasm-bindgen --target web"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
wasm-bindgen "$WASM_IN" \
  --target web \
  --out-dir "$OUT_DIR" \
  --out-name bashkit_wasm \
  --omit-default-module-path

if command -v wasm-opt >/dev/null 2>&1; then
  WASM_OPT_VERSION="$(wasm-opt --version)"
  # Rust emits memory.copy, saturating float conversions, and sign extension;
  # enable those stable features without accepting unrelated proposals.
  echo "==> wasm-opt -Oz (Rust WebAssembly features enabled)"
  wasm-opt -Oz \
    --enable-bulk-memory \
    --enable-nontrapping-float-to-int \
    --enable-sign-ext \
    "$OUT_DIR/bashkit_wasm_bg.wasm" -o "$OUT_DIR/bashkit_wasm_bg.wasm"
else
  WASM_OPT_VERSION=none
  echo "==> wasm-opt not found; skipping size optimization"
fi

# Ship the hand-authored wrapper + option/context types alongside the generated glue.
cp "$CRATE_DIR/js/index.js" "$OUT_DIR/index.js"
cp "$CRATE_DIR/js/index.d.ts" "$OUT_DIR/index.d.ts"
cp "$CRATE_DIR/package.json" "$OUT_DIR/package.json"
cp "$CRATE_DIR/README.md" "$OUT_DIR/README.md"
node "$CRATE_DIR/scripts/build-info.mjs" "$OUT_DIR" "$PROFILE" "$FEATURES" "$WASM_OPT_VERSION"

echo "==> built $OUT_DIR"
ls -lh "$OUT_DIR"
