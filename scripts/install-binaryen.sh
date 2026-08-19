#!/usr/bin/env bash
# Install a pinned `wasm-opt` (binaryen) onto PATH.
#
# Important decision: this replaces `apt-get update && apt-get install -y
# binaryen` in the two workflows that build the browser bundle. `apt-get`
# stalled indefinitely on two separate `main` runs (32170059933, 32218864077);
# with no step timeout each burned the 6-hour job ceiling and took the whole CI
# run down as `cancelled`, so `main` went red twice for an apt mirror problem
# unrelated to any diff.
#
# Two things change here. The download is bounded — explicit connect/total
# timeouts and retries, so a stalled mirror fails in minutes instead of hours —
# and the version is pinned, so `wasm-opt -Oz` output no longer depends on
# whichever binaryen the runner image's Ubuntu happens to carry. That matters
# beyond CI hygiene: publish-wasm.yml ships the optimized artifact to npm.
#
# The archive's checksum is pinned too. Binaryen publishes no signature or
# per-release checksum file, so this is the only thing standing between a
# compromised release asset and a wasm bundle we push to npm.
#
# Only `bin/wasm-opt` is extracted: it is statically linked (the archive's
# `lib/` holds just `libbinaryen.a`), so 19 MB of the 102 MB archive is all
# that is needed.
set -euo pipefail

BINARYEN_VERSION="${BINARYEN_VERSION:-125}"
BINARYEN_SHA256="${BINARYEN_SHA256:-7c3bc16599c8274a04d34a504fe4be2047884f900e0e2da2f6fb9cd667183be4}"
DEST="${DEST:-/usr/local/bin}"

archive="binaryen-version_${BINARYEN_VERSION}-x86_64-linux.tar.gz"
url="https://github.com/WebAssembly/binaryen/releases/download/version_${BINARYEN_VERSION}/${archive}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "==> downloading binaryen ${BINARYEN_VERSION}"
curl -fsSL \
    --retry 3 --retry-all-errors --retry-delay 5 \
    --connect-timeout 30 --max-time 300 \
    -o "$tmp/$archive" "$url"

echo "==> verifying checksum"
echo "${BINARYEN_SHA256}  $tmp/$archive" | sha256sum -c -

echo "==> installing wasm-opt to ${DEST}"
# `bin/wasm-opt` only; --strip-components=2 drops the `binaryen-version_N/bin/`
# prefix so the binary lands directly in DEST.
tar xzf "$tmp/$archive" -C "$DEST" --strip-components=2 \
    "binaryen-version_${BINARYEN_VERSION}/bin/wasm-opt"

"$DEST/wasm-opt" --version
