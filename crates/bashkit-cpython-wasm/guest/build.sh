#!/usr/bin/env bash
# Rebuild the CPython wasm32-wasip1 guest and stdlib committed under
# crates/bashkit-cpython-wasm/artifacts/.
#
# Decisions (see knowledge/runtimes/cpython-wasm.md):
# - CPython is built with its own Tools/wasm/wasi script and WASI SDK 24
#   (PEP 816 support matrix), plus static zlib and sqlite3.
# - The guest is linked as a reactor with bashkit_main.c instead of
#   Programs/python.c, then pre-initialized with `wasmtime wizer` so the
#   snapshot already holds a running interpreter with common modules imported.
# - The stdlib ships as one deflate-compressed zip of .py sources. The
#   snapshot caches the zip directory, so the runtime must serve the exact
#   same zip bytes; both artifacts are produced together by this script.
# - Output is a gzip of the stripped snapshot plus the zip. The crate's
#   build.rs compiles the snapshot to Pulley bytecode ahead of time.
#
# Usage: guest/build.sh [WORK_DIR]
# Requires: curl, tar, unzip, python3 (>=3.11), make, a C compiler for the
# native build Python. Everything else is downloaded into WORK_DIR.
set -euo pipefail

CPYTHON_VERSION=3.14.8
WASI_SDK_MAJOR=24
WASMTIME_VERSION=49.0.1
ZLIB_VERSION=1.3.1
SQLITE_AMALGAMATION=sqlite-amalgamation-3500400
SQLITE_YEAR=2025

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE="$(dirname "$HERE")"
WORK="${1:-${TMPDIR:-/tmp}/bashkit-cpython-wasm-build}"
mkdir -p "$WORK"
cd "$WORK"

arch="$(uname -m)"
case "$arch" in
x86_64) sdk_arch=x86_64 ;;
aarch64 | arm64) sdk_arch=arm64 ;;
*) echo "unsupported build host: $arch" >&2 && exit 1 ;;
esac

fetch() { # url dest
    [ -s "$2" ] || curl -fsSL -o "$2" "$1"
}

SDK="$WORK/wasi-sdk-${WASI_SDK_MAJOR}.0-${sdk_arch}-linux"
if [ ! -d "$SDK" ]; then
    fetch "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${WASI_SDK_MAJOR}/wasi-sdk-${WASI_SDK_MAJOR}.0-${sdk_arch}-linux.tar.gz" wasi-sdk.tar.gz
    tar xzf wasi-sdk.tar.gz
fi
WT_DIR="$WORK/wasmtime-v${WASMTIME_VERSION}-${arch}-linux"
if [ ! -x "$WT_DIR/wasmtime" ]; then
    fetch "https://github.com/bytecodealliance/wasmtime/releases/download/v${WASMTIME_VERSION}/wasmtime-v${WASMTIME_VERSION}-${arch}-linux.tar.xz" wasmtime.tar.xz
    tar xJf wasmtime.tar.xz
fi
WASMTIME="$WT_DIR/wasmtime"
CC_WASI="$SDK/bin/clang --target=wasm32-wasip1 --sysroot=$SDK/share/wasi-sysroot"

# --- static deps: zlib (no gz* file API needed) and sqlite3 ---------------
DEPS="$WORK/deps"
if [ ! -f "$DEPS/lib/libz.a" ]; then
    mkdir -p "$DEPS/lib" "$DEPS/include"
    fetch "https://github.com/madler/zlib/releases/download/v${ZLIB_VERSION}/zlib-${ZLIB_VERSION}.tar.gz" zlib.tar.gz
    tar xzf zlib.tar.gz
    (
        cd "zlib-${ZLIB_VERSION}"
        for f in adler32 crc32 deflate infback inffast inflate inftrees trees zutil compress uncompr; do
            $CC_WASI -O2 -c "$f.c" -o "$f.o"
        done
        "$SDK/bin/llvm-ar" rcs "$DEPS/lib/libz.a" ./*.o
        cp zlib.h zconf.h "$DEPS/include/"
    )
fi
if [ ! -f "$DEPS/lib/libsqlite3.a" ]; then
    fetch "https://www.sqlite.org/${SQLITE_YEAR}/${SQLITE_AMALGAMATION}.zip" sqlite.zip
    unzip -oq sqlite.zip
    (
        cd "$SQLITE_AMALGAMATION"
        $CC_WASI -O2 -DSQLITE_THREADSAFE=0 -DSQLITE_OMIT_LOAD_EXTENSION -DSQLITE_OMIT_WAL \
            -DSQLITE_OMIT_SHARED_CACHE -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_GETPID \
            -c sqlite3.c -o sqlite3.o
        "$SDK/bin/llvm-ar" rcs "$DEPS/lib/libsqlite3.a" sqlite3.o
        cp sqlite3.h "$DEPS/include/"
    )
fi

# --- CPython ---------------------------------------------------------------
SRC="$WORK/Python-${CPYTHON_VERSION}"
if [ ! -d "$SRC" ]; then
    fetch "https://www.python.org/ftp/python/${CPYTHON_VERSION}/Python-${CPYTHON_VERSION}.tar.xz" cpython.tar.xz
    tar xJf cpython.tar.xz
fi
HOST_BUILD="$SRC/cross-build/wasm32-wasip1"
BUILD_PY="$SRC/cross-build/$(uname -m)-pc-linux-gnu/python"
if [ ! -f "$HOST_BUILD/libpython3.14.a" ]; then
    (
        cd "$SRC"
        export PATH="$WT_DIR:$PATH" WASI_SDK_PATH="$SDK"
        export ZLIB_CFLAGS="-I$DEPS/include" ZLIB_LIBS="-L$DEPS/lib -lz"
        export LIBSQLITE3_CFLAGS="-I$DEPS/include" LIBSQLITE3_LIBS="-L$DEPS/lib -lsqlite3"
        python3 Tools/wasm/wasi build --quiet -- --disable-test-modules
    )
fi
if [ ! -x "$BUILD_PY" ]; then
    BUILD_PY="$(ls "$SRC"/cross-build/*-linux-gnu/python | head -1)"
fi

# --- guest link: bashkit_main.c replaces Programs/python.o ------------------
$CC_WASI -O2 -I"$SRC/Include" -I"$HOST_BUILD" -c "$HERE/bashkit_main.c" -o "$WORK/bashkit_main.o"
(
    cd "$HOST_BUILD"
    touch Programs/python.o
    link="$(make -n python.wasm 2>/dev/null | grep -- '-o python.wasm')"
    link="${link/Programs\/python.o/$WORK/bashkit_main.o}"
    link="${link/-o python.wasm/-o $WORK/bashkit_raw.wasm -mexec-model=reactor}"
    eval "$link"
)

# --- stdlib zip (sources only) ----------------------------------------------
ROOT="$WORK/root"
rm -rf "$ROOT" "$WORK/stdlib"
mkdir -p "$ROOT/usr/local/lib" "$WORK/stdlib"
cp -r "$SRC/Lib/." "$WORK/stdlib/"
cp "$HOST_BUILD"/build/lib.wasi-wasm32-3.14/_sysconfigdata_*.py "$WORK/stdlib/"
cp "$HERE/_bashkit_boot.py" "$WORK/stdlib/"
(
    cd "$WORK/stdlib"
    # Not usable or not useful in a sandboxed, single-threaded, headless guest.
    rm -rf test idlelib tkinter turtledemo ensurepip venv pydoc_data turtle.py \
        _pyrepl/__pycache__ curses dbm/gnu.py dbm/ndbm.py multiprocessing concurrent/futures/process.py
    find . -name __pycache__ -prune -exec rm -rf {} +
    find . -type d -name tests -prune -exec rm -rf {} +
    # Deterministic zip: sorted entries, fixed timestamps.
    "$BUILD_PY" -I - "$ROOT/usr/local/lib/python314.zip" <<'PY'
import os, sys, zipfile
out = sys.argv[1]
files = []
for dirpath, dirnames, filenames in os.walk("."):
    dirnames.sort()
    for name in sorted(filenames):
        if name.endswith(".py"):
            files.append(os.path.join(dirpath, name)[2:])
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for path in files:
        info = zipfile.ZipInfo(path, date_time=(1980, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        info.external_attr = 0o644 << 16
        with open(path, "rb") as f:
            z.writestr(info, f.read(), compresslevel=9)
PY
)

# --- snapshot ----------------------------------------------------------------
"$WASMTIME" wizer -S cli --dir "$ROOT::/" -o "$WORK/bashkit_wizer.wasm" "$WORK/bashkit_raw.wasm"
"$SDK/bin/llvm-strip" -o "$WORK/python.wasm" "$WORK/bashkit_wizer.wasm"

# Smoke test the snapshot with the reference runtime before committing it.
out="$("$WASMTIME" run -S cli --dir "$ROOT::/" --env PWD=/ --invoke bashkit_run \
    "$WORK/python.wasm" -c 'import json, re, csv, sqlite3, zlib; print(json.dumps({"ok": 1}))' 2>/dev/null)"
case "$out" in
*'{"ok": 1}'*) ;;
*) echo "snapshot smoke test failed: $out" >&2 && exit 1 ;;
esac

mkdir -p "$CRATE/artifacts"
gzip -9 -n -c "$WORK/python.wasm" >"$CRATE/artifacts/python.wasm.gz"
cp "$ROOT/usr/local/lib/python314.zip" "$CRATE/artifacts/python314.zip"
(
    cd "$CRATE/artifacts"
    {
        echo "cpython=${CPYTHON_VERSION}"
        echo "wasi_sdk=${WASI_SDK_MAJOR}"
        echo "wasmtime_wizer=${WASMTIME_VERSION}"
        echo "zlib=${ZLIB_VERSION}"
        echo "sqlite=${SQLITE_AMALGAMATION}"
        sha256sum python.wasm.gz python314.zip
    } >MANIFEST
)
ls -la "$CRATE/artifacts"
