#!/usr/bin/env bash
#
# Run the advisory scan over every lockfile in the tree, in every ecosystem.
#
# Single source of truth for the advisory scan: both the CI `audit` job
# (.github/workflows/ci.yml, on every push and PR) and the nightly
# `advisories` job (.github/workflows/nightly.yml, on a schedule) call this.
# The suppression lists below used to live inline in ci.yml, so a second caller
# meant a second copy that could drift out of sync with deny.toml. They live
# here now; deny.toml and knowledge/security/threat-model.md must still agree
# with them.
#
# Why a scheduled caller exists at all: advisories are published against code
# that has not changed. A push-only scan leaves main unaudited for as long as
# nobody pushes, which is exactly when a fresh critical advisory is least
# likely to be noticed.
#
# Why discovery rather than a list: this repo has more than one workspace per
# ecosystem, and a `cargo audit` at the root only ever sees the root lockfile.
# Naming the others by hand failed twice — the fuzz lockfile rotted onto an
# unsound `anyhow` before it was added, and `examples/hyperlight/host` shipped
# wasmtime 38.0.4 with 16 advisories (two of them critical) because nothing
# audited it either. Discovering every lockfile covers a new workspace the day
# it lands.
#
# Why three ecosystems rather than cargo alone: the same lesson, one ecosystem
# out. Until 2026-10-02 this scan was cargo-only, so the npm and PyPI lockfiles
# were covered by Dependabot version bumps and nothing else. A bump is not a
# scan — it fires when a release exists, not when an advisory lands — and on
# 2026-10-01 seven advisories (four high) were found sitting in three lockfiles
# by hand, with no job that was ever going to fail. Coverage that stops at an
# ecosystem boundary is quiet, not clean.
#
# All three ecosystems are audited on every run, and each is reported
# separately, so one ecosystem's advisory never masks another's.

set -uo pipefail

# Accepted risks, per ecosystem. Every entry needs a rationale and a removal
# condition in "Suppressed advisories" in knowledge/security/threat-model.md.
# The cargo list must also match the `[advisories] ignore` list in deny.toml.
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

# npm advisories, by GHSA (or CVE) id. A patched release is almost always
# reachable by a bump, because the npm dependencies are all dev- or
# example-only; the entry below is the exception the mechanism exists for. It
# landed on 2026-10-02 against the newest published version of the package,
# with no fixed release to move to, so the choice is an explicit, documented
# acceptance or a red scan that stays red — not a pinned floor nobody records.
#
# It is a `last_affected` advisory in OSV, not a `fixed` one: there is no
# patched version, so re-check the advisory's own fixed-range before dropping
# the entry. A sibling entry for GHSA-ch52-4w7c-c8xp
# (`http-cache-semantics`) was dropped this way once 4.3.0 published above its
# `last_affected` 4.2.0; `site/package.json` now pins that floor as an
# override. The package below reaches no consumer of a published artifact.
#
# GHSA-vfj7-8cjw-p6xm — stack-exhaustion DoS in `braces` from deeply nested
# patterns. Reached only as ava → globby → fast-glob → micromatch → braces, a
# devDependency of crates/bashkit-js: the test runner globbing test-file
# patterns that live in this repo. `braces` 3.0.3 is both the latest release
# and the last affected one. The published package ships the files listed in
# its package.json `files` array, so no consumer installs this chain. Drop
# once `braces` ships a release above 3.0.3.
#
IGNORED_NPM_ADVISORIES=(
    GHSA-vfj7-8cjw-p6xm
)

# PyPI advisories, by OSV (or GHSA) id. Empty: nothing has yet needed
# suppressing here. Same bar as the npm list above — an entry needs a rationale
# and a removal condition in the threat model, which a test enforces.
IGNORED_PYPI_ADVISORIES=()

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root" || exit 1

status=0

# `${arr[@]+...}` keeps an empty suppression list from tripping `set -u`.
cargo_ignore_flags=()
for advisory in ${IGNORED_ADVISORIES[@]+"${IGNORED_ADVISORIES[@]}"}; do
    cargo_ignore_flags+=(--ignore "$advisory")
done

npm_ignore_flags=()
for advisory in ${IGNORED_NPM_ADVISORIES[@]+"${IGNORED_NPM_ADVISORIES[@]}"}; do
    npm_ignore_flags+=(--ignore "$advisory")
done

pypi_ignore_flags=()
for advisory in ${IGNORED_PYPI_ADVISORIES[@]+"${IGNORED_PYPI_ADVISORIES[@]}"}; do
    pypi_ignore_flags+=(--ignore "$advisory")
done

# Discover every lockfile of one kind. `target` and `node_modules` hold
# dependency copies, not lockfiles this repo resolves.
discover() {
    find . -name "$1" -not -path './target/*' -not -path '*/node_modules/*' | sort
}

# A scan that audited nothing is a broken scan, not a clean one. Each ecosystem
# asserts its own floor, so a lockfile kind disappearing from discovery fails
# here instead of silently narrowing the scan.
require_found() {
    if [ "$1" -eq 0 ]; then
        echo "error: no $2 found under $root" >&2
        status=1
    fi
}

found=0
while IFS= read -r lock; do
    found=$((found + 1))
    echo "::group::cargo audit $lock"
    cargo audit ${cargo_ignore_flags[@]+"${cargo_ignore_flags[@]}"} -f "$lock" || status=1
    echo "::endgroup::"
done < <(discover Cargo.lock)
require_found "$found" Cargo.lock

found=0
while IFS= read -r lock; do
    found=$((found + 1))
    echo "::group::pnpm audit $lock"
    "$root/scripts/lib/npm_audit.py" \
        ${npm_ignore_flags[@]+"${npm_ignore_flags[@]}"} "$(dirname "$lock")" || status=1
    echo "::endgroup::"
done < <(discover pnpm-lock.yaml)
require_found "$found" pnpm-lock.yaml

found=0
while IFS= read -r lock; do
    found=$((found + 1))
    echo "::group::osv audit $lock"
    "$root/scripts/lib/pypi_audit.py" \
        ${pypi_ignore_flags[@]+"${pypi_ignore_flags[@]}"} "$lock" || status=1
    echo "::endgroup::"
done < <(discover uv.lock)
require_found "$found" uv.lock

exit $status
