#!/usr/bin/env bash
#
# Run `cargo audit` over every Cargo.lock in the tree.
#
# Single source of truth for the advisory scan: both the CI `audit` job
# (.github/workflows/ci.yml, on every push and PR) and the nightly
# `advisories` job (.github/workflows/nightly.yml, on a schedule) call this.
# The suppression list below used to live inline in ci.yml, so a second caller
# meant a second copy that could drift out of sync with deny.toml. It lives
# here now; deny.toml and knowledge/security/threat-model.md must still agree
# with it.
#
# Why a scheduled caller exists at all: advisories are published against code
# that has not changed. A push-only scan leaves main unaudited for as long as
# nobody pushes, which is exactly when a fresh critical advisory is least
# likely to be noticed.
#
# Why discovery rather than a list: this repo has more than one cargo
# workspace, and a `cargo audit` at the root only ever sees the root lockfile.
# Naming the others by hand failed twice — the fuzz lockfile rotted onto an
# unsound `anyhow` before it was added, and `examples/hyperlight/host` shipped
# wasmtime 38.0.4 with 16 advisories (two of them critical) because nothing
# audited it either. Discovering every lockfile covers a new workspace the day
# it lands.

set -uo pipefail

# Accepted risks. Every entry needs a rationale and a removal condition in
# "Suppressed advisories" in knowledge/security/threat-model.md, and must match
# the `[advisories] ignore` list in deny.toml.
#
# RUSTSEC-2023-0071 — Marvin timing sidechannel in `rsa` (TM-CRY-002). The
# advisory covers every published `rsa` version, so there is nothing to upgrade
# to, and the crate is reachable only through the opt-in `ssh` feature. Drop
# once `rsa` ships a constant-time release.
#
# The flag currently matches nothing: `ssh-key` resolves `rsa` to a pre-release
# (0.10.0-rc.18) and RustSec ranges skip pre-releases. That is not a fix (the
# advisory still has `patched = []`), so keep the flag — matching resumes when
# `rsa` ships a stable release.
IGNORED_ADVISORIES=(
    RUSTSEC-2023-0071
)

ignore_flags=()
for advisory in "${IGNORED_ADVISORIES[@]}"; do
    ignore_flags+=(--ignore "$advisory")
done

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root" || exit 1

status=0
found=0
while IFS= read -r lock; do
    found=1
    echo "::group::cargo audit $lock"
    cargo audit "${ignore_flags[@]}" -f "$lock" || status=1
    echo "::endgroup::"
done < <(find . -name Cargo.lock -not -path './target/*' -not -path '*/node_modules/*' | sort)

# A scan that audited nothing is a broken scan, not a clean one.
if [ "$found" -eq 0 ]; then
    echo "error: no Cargo.lock found under $root" >&2
    exit 1
fi

exit $status
