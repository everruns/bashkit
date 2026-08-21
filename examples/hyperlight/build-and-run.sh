#!/usr/bin/env bash
# Build the bashkit guest component and run it under wasmtime.
#
# Requires: rustup target add wasm32-unknown-unknown; cargo install wasm-tools
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$here"

core="target/wasm32-unknown-unknown/release/bashkit_hyperlight_guest.wasm"
component="target/bashkit-sandbox.component.wasm"

echo "==> building guest (wasm32-unknown-unknown, no JS, no WASI)"
RUSTFLAGS='--cfg getrandom_backend="custom"' \
    cargo build --release --target wasm32-unknown-unknown

echo "==> wrapping core module as a component"
wasm-tools component new "$core" -o "$component"

echo "==> guest imports (must be host-only)"
imports=$(wasm-tools print "$core" | grep -oE '\(import "[^"]+"' | sort -u)
echo "$imports"
if grep -qv '"bashkit:sandbox/host"' <<<"$imports"; then
    echo "FAIL: guest imports something other than the host world" >&2
    exit 1
fi

echo "==> running under wasmtime"
actual=$(cd host && cargo run --release --quiet -- "../$component" "$(cat "$here/smoke.sh")")
expected=$'beta gamma alpha \n6\nslept 1s'

echo "$actual"
if [[ "$actual" != "$expected" ]]; then
    echo "FAIL: unexpected output from the sandbox" >&2
    diff <(echo "$expected") <(echo "$actual") >&2 || true
    exit 1
fi
echo "==> OK"
