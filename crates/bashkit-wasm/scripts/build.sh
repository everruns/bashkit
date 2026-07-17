#!/usr/bin/env bash
# Build the browser wasm bundle for @everruns/bashkit-web.
#
# Produces an ES-module package under `pkg/` via `wasm-bindgen --target web`,
# which needs NO bundler and NO cross-origin isolation headers to load.
#
# Requires: rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli
# Optional (smaller output): wasm-opt (binaryen) on PATH.
set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKSPACE_ROOT="$(cd "$CRATE_DIR/../.." && pwd)"
OUT_DIR="$CRATE_DIR/pkg"
TARGET=wasm32-unknown-unknown
PROFILE="${1:-release}"

echo "==> cargo build ($PROFILE) for $TARGET"
if [ "$PROFILE" = "release" ]; then
  CARGO_PROFILE_FLAG="--release"
  TARGET_SUBDIR="release"
else
  CARGO_PROFILE_FLAG=""
  TARGET_SUBDIR="debug"
fi
# shellcheck disable=SC2086
cargo build -p bashkit-wasm --target "$TARGET" $CARGO_PROFILE_FLAG

WASM_IN="$WORKSPACE_ROOT/target/$TARGET/$TARGET_SUBDIR/bashkit_wasm.wasm"

echo "==> wasm-bindgen --target web"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
wasm-bindgen "$WASM_IN" \
  --target web \
  --out-dir "$OUT_DIR" \
  --out-name bashkit_wasm \
  --omit-default-module-path

if command -v wasm-opt >/dev/null 2>&1; then
  echo "==> wasm-opt -Oz"
  wasm-opt -Oz "$OUT_DIR/bashkit_wasm_bg.wasm" -o "$OUT_DIR/bashkit_wasm_bg.wasm"
else
  echo "==> wasm-opt not found; skipping size optimization"
fi

# Ship the hand-authored wrapper + option/context types alongside the generated glue.
cp "$CRATE_DIR/js/index.js" "$OUT_DIR/index.js"
cp "$CRATE_DIR/js/index.d.ts" "$OUT_DIR/index.d.ts"
cp "$CRATE_DIR/package.json" "$OUT_DIR/package.json"
cp "$CRATE_DIR/README.md" "$OUT_DIR/README.md"

echo "==> built $OUT_DIR"
ls -lh "$OUT_DIR"
