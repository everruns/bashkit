#!/usr/bin/env bash
set -euo pipefail

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

archive="ripgrep-${version}-${target}.tar.gz"
url="https://github.com/BurntSushi/ripgrep/releases/download/${version}/${archive}"
tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

curl -fsSL "${url}" -o "${tmp}/${archive}"
tar -xzf "${tmp}/${archive}" -C "${tmp}"

install_dir="${RUNNER_TOOL_CACHE:-${HOME}/.cache}/ripgrep-${version}-${target}/bin"
mkdir -p "${install_dir}"
cp "${tmp}/ripgrep-${version}-${target}/rg" "${install_dir}/rg"
chmod +x "${install_dir}/rg"

if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "${install_dir}" >> "${GITHUB_PATH}"
fi

"${install_dir}/rg" --version
