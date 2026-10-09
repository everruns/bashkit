#!/usr/bin/env bash
# Rebuild the uutils/coreutils wasm32-wasip1 guest committed under
# crates/bashkit-coreutils-wasm/artifacts/.
#
# Decisions (see knowledge/runtimes/wasm-coreutils.md):
# - A plain Rust build of this directory's multicall crate; uutils needs no
#   C toolchain or WASI SDK for wasm32-wasip1 (it is a tier in uutils' own CI).
# - No Wizer snapshot: a Rust `main` starts in microseconds, there is no
#   interpreter state worth pre-initializing.
# - Output is xz-compressed (same as the CPython guest); the crate's build.rs
#   decodes it and compiles it to Pulley ahead of time.
#
# Usage: guest/build.sh
# Requires: rustup with the toolchain from rust-toolchain.toml, python3, xz,
# sha256sum.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE="$(dirname "$HERE")"
TOOLCHAIN="$(sed -n 's/^channel = "\(.*\)"/\1/p' "$CRATE/../../rust-toolchain.toml")"

python3 "$HERE/gen.py"
rustup target add --toolchain "$TOOLCHAIN" wasm32-wasip1 >/dev/null
cargo "+$TOOLCHAIN" build --release --target wasm32-wasip1 \
    --manifest-path "$HERE/Cargo.toml"

WASM="$HERE/target/wasm32-wasip1/release/bashkit-coreutils-guest.wasm"
xz -9e -T1 -c "$WASM" >"$CRATE/artifacts/coreutils.wasm.xz"
cp "$HERE/utils.txt" "$CRATE/artifacts/utils.txt"

UUTILS_VERSION="$(sed -n 's/^uucore = { version = "=\(.*\)" }/\1/p' "$HERE/Cargo.toml")"
{
    echo "uutils=$UUTILS_VERSION"
    echo "rustc=$TOOLCHAIN"
    (cd "$CRATE/artifacts" && sha256sum coreutils.wasm.xz utils.txt)
} >"$CRATE/artifacts/MANIFEST"
cat "$CRATE/artifacts/MANIFEST"
