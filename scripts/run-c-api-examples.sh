#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
example_dir="$repo_root/crates/bashkit-capi/examples"
output_dir="$repo_root/target/c-api-examples"

"$repo_root/scripts/build-c-api.sh"
mkdir -p "$output_dir"

c++ -std=c++17 -Wall -Wextra -Werror -x c++ \
    -I "$repo_root/crates/bashkit-capi/include" \
    -c "$example_dir/hello.c" \
    -o "$output_dir/header-cxx.o"

case "$(uname -s)" in
    Darwin)
        loader_variable=DYLD_LIBRARY_PATH
        ;;
    Linux)
        loader_variable=LD_LIBRARY_PATH
        ;;
    *)
        echo "run-c-api-examples.sh supports macOS and Linux" >&2
        exit 1
        ;;
esac

for example in hello files; do
    cc -std=c11 -Wall -Wextra -Werror \
        -I "$repo_root/crates/bashkit-capi/include" \
        "$example_dir/$example.c" \
        -L "$repo_root/target/c-api/debug" -lbashkit \
        -o "$output_dir/$example"
    env "$loader_variable=$repo_root/target/c-api/debug" "$output_dir/$example"
done
