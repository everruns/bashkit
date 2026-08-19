#!/usr/bin/env bash
set -euo pipefail

# Install `wasm-opt` from a pinned binaryen release instead of `apt-get`.
#
# Two reasons, both learned the hard way:
#   1. `apt-get update` has no overall timeout. On 2026-08-18 an Ubuntu mirror
#      stalled mid-transfer and the CI job blocked until GitHub's 6h hard limit
#      cancelled it, turning main red. Nothing about the build was broken.
#   2. CI executes the installed `wasm-opt`, so the archive digest is pinned
#      before extraction — a compromised release asset or redirect must not
#      become CI code execution. Same posture as scripts/install-ripgrep-ci.sh.
#
# Bump `version` and the digests together; crates/bashkit-wasm/scripts/build.sh
# only relies on flags that have been stable since binaryen 116.
version="${1:-123}"
os="$(uname -s)"
arch="$(uname -m)"

case "${os}:${arch}" in
  Linux:x86_64)
    target="x86_64-linux"
    ;;
  Linux:aarch64 | Linux:arm64)
    target="aarch64-linux"
    ;;
  Darwin:x86_64)
    target="x86_64-macos"
    ;;
  Darwin:aarch64 | Darwin:arm64)
    target="arm64-macos"
    ;;
  *)
    echo "unsupported platform for pinned binaryen install: ${os} ${arch}" >&2
    exit 1
    ;;
esac

case "${version}:${target}" in
  123:x86_64-linux)
    expected_sha256="e959f2170af4c20c552e9de3a0253704d6a9d2766e8fdb88e4d6ac4bae9388fe"
    ;;
  123:aarch64-linux)
    expected_sha256="4b6bd61ba6cd3b18c993b4657d93426c782f9b91b74be0d38018cd8be1319376"
    ;;
  123:x86_64-macos)
    expected_sha256="cc18b14d2b673d9c66bf54f31ff2b0ceb23ba5132455b893965ae2792f9e00dd"
    ;;
  123:arm64-macos)
    expected_sha256="74428be348c1a09863e7b642a1fa948cabf8ec9561052233d8288e941951725b"
    ;;
  *)
    echo "missing pinned SHA-256 for binaryen ${version} ${target}" >&2
    exit 1
    ;;
esac

sha256_file() {
  local file="$1"

  if command -v sha256sum > /dev/null 2>&1; then
    sha256sum "${file}" | awk '{print $1}'
  elif command -v shasum > /dev/null 2>&1; then
    shasum -a 256 "${file}" | awk '{print $1}'
  else
    echo "No SHA-256 tool found (need sha256sum or shasum)" >&2
    exit 1
  fi
}

verify_sha256() {
  local file="$1"
  local expected="$2"
  local actual

  actual="$(sha256_file "${file}")"
  if [[ "${actual}" != "${expected}" ]]; then
    echo "SHA-256 mismatch for ${file}" >&2
    echo "Expected: ${expected}" >&2
    echo "Actual:   ${actual}" >&2
    exit 1
  fi
}

archive="binaryen-version_${version}-${target}.tar.gz"
url="https://github.com/WebAssembly/binaryen/releases/download/version_${version}/${archive}"
tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

# Bound every attempt: a stalled CDN connection must cost seconds, not hours.
downloaded=""
for attempt in 1 2 3; do
  if curl -fsSL --connect-timeout 20 --max-time 300 --retry 2 "${url}" -o "${tmp}/${archive}"; then
    downloaded="yes"
    break
  fi
  echo "binaryen download attempt ${attempt} failed; retrying" >&2
  sleep $((attempt * 5))
done

if [[ -z "${downloaded}" ]]; then
  echo "failed to download ${url}" >&2
  exit 1
fi

verify_sha256 "${tmp}/${archive}" "${expected_sha256}"
tar -xzf "${tmp}/${archive}" -C "${tmp}"

install_dir="${RUNNER_TOOL_CACHE:-${HOME}/.cache}/binaryen-${version}-${target}"
rm -rf "${install_dir}"
mkdir -p "${install_dir}"
cp -R "${tmp}/binaryen-version_${version}/." "${install_dir}/"
chmod +x "${install_dir}/bin/wasm-opt"

if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "${install_dir}/bin" >> "${GITHUB_PATH}"
fi

"${install_dir}/bin/wasm-opt" --version
