#!/usr/bin/env bash
# The Cargo target stays uniquely named; this script creates the canonical
# consumer artifact without colliding with Rust and Python workspace outputs.
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
profile=debug
cargo_args=(-p bashkit-capi)

if [[ "${1:-}" == "--release" ]]; then
    profile=release
    cargo_args+=(--release)
    shift
fi

target=""
if [[ "${1:-}" == "--target" ]]; then
    target=${2:?missing target triple}
    cargo_args+=(--target "$target")
    shift 2
fi

if [[ $# -ne 0 ]]; then
    echo "usage: $0 [--release] [--target TARGET]" >&2
    exit 2
fi

if [[ -n "$target" ]]; then
    build_dir="$repo_root/target/$target/$profile"
else
    build_dir="$repo_root/target/$profile"
fi
native_dir="$repo_root/target/c-api/$profile"
mkdir -p "$native_dir"

case "$(uname -s)" in
    Darwin)
        cargo build --manifest-path "$repo_root/Cargo.toml" "${cargo_args[@]}"
        cp "$build_dir/libbashkit_capi.dylib" "$native_dir/libbashkit.dylib"
        install_name_tool -id @rpath/libbashkit.dylib "$native_dir/libbashkit.dylib"
        ;;
    Linux)
        cargo rustc --manifest-path "$repo_root/Cargo.toml" \
            "${cargo_args[@]}" --lib -- -C link-arg=-Wl,-soname,libbashkit.so
        cp "$build_dir/libbashkit_capi.so" "$native_dir/libbashkit.so"
        ;;
    *)
        echo "build-c-api.sh supports macOS and Linux" >&2
        exit 1
        ;;
esac
