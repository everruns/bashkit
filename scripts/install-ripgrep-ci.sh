#!/usr/bin/env bash
set -euo pipefail

# CI executes the installed `rg`; pin archive digests before extraction so a
# compromised release asset or redirect cannot become CI code execution.
version="${1:-15.1.0}"
os="$(uname -s)"
arch="$(uname -m)"

case "${os}:${arch}" in
  Linux:x86_64)
    target="x86_64-unknown-linux-musl"
    ;;
  Linux:aarch64 | Linux:arm64)
    target="aarch64-unknown-linux-gnu"
    ;;
  Darwin:x86_64)
    target="x86_64-apple-darwin"
    ;;
  Darwin:aarch64 | Darwin:arm64)
    target="aarch64-apple-darwin"
    ;;
  *)
    echo "unsupported platform for pinned ripgrep install: ${os} ${arch}" >&2
    exit 1
    ;;
esac

case "${version}:${target}" in
  15.1.0:x86_64-unknown-linux-musl)
    expected_sha256="1c9297be4a084eea7ecaedf93eb03d058d6faae29bbc57ecdaf5063921491599"
    ;;
  15.1.0:aarch64-unknown-linux-gnu)
    expected_sha256="2b661c6ef508e902f388e9098d9c4c5aca72c87b55922d94abdba830b4dc885e"
    ;;
  15.1.0:x86_64-apple-darwin)
    expected_sha256="64811cb24e77cac3057d6c40b63ac9becf9082eedd54ca411b475b755d334882"
    ;;
  15.1.0:aarch64-apple-darwin)
    expected_sha256="378e973289176ca0c6054054ee7f631a065874a352bf43f0fa60ef079b6ba715"
    ;;
  *)
    echo "missing pinned SHA-256 for ripgrep ${version} ${target}" >&2
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

archive="ripgrep-${version}-${target}.tar.gz"
url="https://github.com/BurntSushi/ripgrep/releases/download/${version}/${archive}"
tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

curl -fsSL "${url}" -o "${tmp}/${archive}"
verify_sha256 "${tmp}/${archive}" "${expected_sha256}"
tar -xzf "${tmp}/${archive}" -C "${tmp}"

install_dir="${RUNNER_TOOL_CACHE:-${HOME}/.cache}/ripgrep-${version}-${target}/bin"
mkdir -p "${install_dir}"
cp "${tmp}/ripgrep-${version}-${target}/rg" "${install_dir}/rg"
chmod +x "${install_dir}/rg"

if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "${install_dir}" >> "${GITHUB_PATH}"
fi

"${install_dir}/rg" --version
