#!/usr/bin/env bash
# The bindgen CLI schema must exactly match the crate in this checkout's lock.
# Reuse matching cached binaries; replace stale ones and verify the executable on PATH.
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BINDGEN_VERSION=$(python3 - "$REPO_ROOT/Cargo.lock" <<'PY'
import sys
import tomllib

with open(sys.argv[1], 'rb') as source:
    packages = tomllib.load(source)['package']
versions = {p['version'] for p in packages if p['name'] == 'wasm-bindgen'}
if len(versions) != 1:
    sys.exit('Expected exactly one wasm-bindgen version in Cargo.lock')
print(versions.pop())
PY
)

EXPECTED="wasm-bindgen $BINDGEN_VERSION"
if [[ "$(wasm-bindgen --version 2>/dev/null || true)" != "$EXPECTED" ]]; then
    cargo install wasm-bindgen-cli --version "$BINDGEN_VERSION" --locked --force
fi
if [[ "$(wasm-bindgen --version)" != "$EXPECTED" ]]; then
    echo "Error: wasm-bindgen on PATH does not match Cargo.lock ($BINDGEN_VERSION)" >&2
    exit 1
fi
echo "$EXPECTED (matches Cargo.lock)"
