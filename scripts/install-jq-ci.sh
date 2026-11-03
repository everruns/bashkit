#!/usr/bin/env bash
set -euo pipefail

# CI executes the installed `jq` as the oracle for the jq differential tests;
# pin the release binary's digest before it lands on PATH so a compromised
# release asset or redirect cannot become CI code execution.
#
# Pinning the version matters on its own here, not just the digest. The jq
# differential tests assert byte-for-byte parity against whatever `jq` is on
# PATH, so an unpinned oracle makes the suite a function of the runner image:
# jq 1.7 and 1.8 disagree on `--indent 0` (1.7 implied compact output, 1.8
# emits newlines without indentation, jqlang/jq#3232), and when `ubuntu-latest`
# began rolling from 24.04 to 26.04 the two images shipped the two versions.
# That turned one parity case red on whichever half of the fleet disagreed with
# the implementation, with no change on our side.
version="${1:-1.8.1}"
os="$(uname -s)"
arch="$(uname -m)"

case "${os}:${arch}" in
  Linux:x86_64)
    target="linux-amd64"
    ;;
  Linux:aarch64 | Linux:arm64)
    target="linux-arm64"
    ;;
  Darwin:x86_64)
    target="macos-amd64"
    ;;
  Darwin:aarch64 | Darwin:arm64)
    target="macos-arm64"
    ;;
  *)
    echo "unsupported platform for pinned jq install: ${os} ${arch}" >&2
    exit 1
    ;;
esac

case "${version}:${target}" in
  1.8.1:linux-amd64)
    expected_sha256="020468de7539ce70ef1bceaf7cde2e8c4f2ca6c3afb84642aabc5c97d9fc2a0d"
    ;;
  1.8.1:linux-arm64)
    expected_sha256="6bc62f25981328edd3cfcfe6fe51b073f2d7e7710d7ef7fcdac28d4e384fc3d4"
    ;;
  1.8.1:macos-amd64)
    expected_sha256="e80dbe0d2a2597e3c11c404f03337b981d74b4a8504b70586c354b7697a7c27f"
    ;;
  1.8.1:macos-arm64)
    expected_sha256="a9fe3ea2f86dfc72f6728417521ec9067b343277152b114f4e98d8cb0e263603"
    ;;
  *)
    echo "missing pinned SHA-256 for jq ${version} ${target}" >&2
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

binary="jq-${target}"
url="https://github.com/jqlang/jq/releases/download/jq-${version}/${binary}"
tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

curl -fsSL "${url}" -o "${tmp}/jq"
verify_sha256 "${tmp}/jq" "${expected_sha256}"

# Ahead of the image's own /usr/bin/jq on PATH: GitHub prepends each
# GITHUB_PATH entry, so the pinned build wins over a preinstalled one.
install_dir="${RUNNER_TOOL_CACHE:-${HOME}/.cache}/jq-${version}-${target}/bin"
mkdir -p "${install_dir}"
cp "${tmp}/jq" "${install_dir}/jq"
chmod +x "${install_dir}/jq"

if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "${install_dir}" >> "${GITHUB_PATH}"
fi

"${install_dir}/jq" --version
