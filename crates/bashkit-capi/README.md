# Bashkit C API

`bashkit-capi` exposes Bashkit through a versioned native C ABI. It is intended
for language bindings and applications that cannot link Rust crates directly.

The ABI is experimental. Its opaque handles and explicit free functions keep
Rust layouts and allocators private, so compatible v1 additions do not require
callers to rebuild their own object layouts.

## Install a release archive

Download the archive for your platform from the
[Bashkit GitHub Releases](https://github.com/everruns/bashkit/releases):

| Platform | Archive | Runtime library |
|---|---|---|
| macOS Apple Silicon | `bashkit-capi-aarch64-apple-darwin.tar.gz` | `libbashkit.dylib` |
| macOS Intel | `bashkit-capi-x86_64-apple-darwin.tar.gz` | `libbashkit.dylib` |
| Linux x86-64 | `bashkit-capi-x86_64-unknown-linux-gnu.tar.gz` | `libbashkit.so` |
| Linux ARM64 | `bashkit-capi-aarch64-unknown-linux-gnu.tar.gz` | `libbashkit.so` |
| Windows x86-64 | `bashkit-capi-x86_64-pc-windows-msvc.zip` | `bashkit.dll` |

Verify the adjacent `.sha256` file before extracting:

```sh
sha256sum -c bashkit-capi-*.sha256       # Linux
shasum -a 256 -c bashkit-capi-*.sha256  # macOS
```

On Windows, compare `Get-FileHash <archive> -Algorithm SHA256` with the value in
the `.sha256` file. A release archive contains:

```text
include/bashkit.h
examples/hello.c
examples/files.c
lib/libbashkit.so       # Linux
lib/libbashkit.dylib    # macOS
lib/bashkit.dll         # Windows runtime
lib/bashkit.lib         # Windows import library
```

The shared library contains the complete Bashkit runtime. Applications do not
need a separate Rust installation or another Bashkit library.

### Linux

The recommended deployment keeps the library beside the application:

```sh
export BASHKIT_SDK=/path/to/extracted/bashkit-capi-target
mkdir -p dist/lib
cc -std=c11 -I "$BASHKIT_SDK/include" \
  "$BASHKIT_SDK/examples/hello.c" \
  -L "$BASHKIT_SDK/lib" -lbashkit \
  -Wl,-rpath,'$ORIGIN/lib' -o dist/hello
cp "$BASHKIT_SDK/lib/libbashkit.so" dist/lib/
./dist/hello
```

Set `BASHKIT_SDK` to the extracted archive directory. The embedded `$ORIGIN`
rpath makes the executable find `dist/lib/libbashkit.so` without modifying
`LD_LIBRARY_PATH` or installing anything system-wide.

### macOS

```sh
mkdir -p dist/lib
cc -std=c11 -I "$BASHKIT_SDK/include" \
  "$BASHKIT_SDK/examples/hello.c" \
  -L "$BASHKIT_SDK/lib" -lbashkit \
  -Wl,-rpath,@loader_path/lib -o dist/hello
cp "$BASHKIT_SDK/lib/libbashkit.dylib" dist/lib/
./dist/hello
```

The library identity is `@rpath/libbashkit.dylib`, so the application-local
layout works without `DYLD_LIBRARY_PATH`.

### Windows

Run these commands from an x64 Native Tools Command Prompt for Visual Studio:

```bat
set BASHKIT_SDK=C:\path\to\extracted\bashkit-capi-target
mkdir dist
cl /nologo /W4 /I "%BASHKIT_SDK%\include" ^
  "%BASHKIT_SDK%\examples\hello.c" ^
  "%BASHKIT_SDK%\lib\bashkit.lib" /Fe:dist\hello.exe
copy "%BASHKIT_SDK%\lib\bashkit.dll" dist\bashkit.dll
dist\hello.exe
```

`bashkit.lib` is the link-time import library. `bashkit.dll` must be beside the
executable or in another directory on `PATH` when the application starts.

## Build from source

Building requires the repository's pinned Rust toolchain plus a C compiler.
On macOS or Linux, from the repository root:

```sh
./scripts/build-c-api.sh --release
```

The consumer artifact is written to `target/c-api/release/` under its canonical
name. The internal Cargo target remains `bashkit_capi` to avoid colliding with
Bashkit's Rust and Python workspace artifacts.

On Windows, the release workflow builds the internal DLL, renames it to
`bashkit.dll`, and creates `bashkit.lib` from `include/bashkit.def`; release
archives are the recommended Windows developer input.

## Build and run repository examples

From the repository root on macOS or Linux:

```sh
./scripts/run-c-api-examples.sh
```

This builds `libbashkit`, compiles both C programs with warnings denied,
and runs them:

- [`examples/hello.c`](examples/hello.c) creates a shell and prints its virtual
  user and current UTC time.
- [`examples/files.c`](examples/files.c) configures a shell, exchanges files
  through the binary-safe VFS API, and runs a transformation.

For direct development compilation after `./scripts/build-c-api.sh`:

```sh
cc -std=c11 -I crates/bashkit-capi/include \
  crates/bashkit-capi/examples/hello.c \
  -L target/c-api/debug -lbashkit -o /tmp/bashkit-hello
LD_LIBRARY_PATH=target/c-api/debug /tmp/bashkit-hello       # Linux
DYLD_LIBRARY_PATH=target/c-api/debug /tmp/bashkit-hello     # macOS
```

## Verify compatibility

Call `bashkit_abi_version()` before using a dynamically discovered library and
require `BASHKIT_ABI_VERSION_1`. `bashkit_version()` reports the Bashkit package
version; package and ABI versions intentionally evolve independently.

The architecture of the application and library must match. For example, an
x86-64 application cannot load an ARM64 Bashkit library.

## Loader troubleshooting

- Linux `libbashkit.so: cannot open shared object file`: deploy the library at
  the compiled rpath, or temporarily set `LD_LIBRARY_PATH` to its directory.
- macOS `Library not loaded: @rpath/libbashkit.dylib`: add an application rpath
  or temporarily set `DYLD_LIBRARY_PATH`.
- Windows error 126: put `bashkit.dll` beside the executable and confirm both
  are x86-64.
- Undefined `bashkit_*` symbols while linking: place `-lbashkit` after source or
  object inputs on Unix, and verify that the header and library came from the
  same release archive.

## Configuration

`bashkit_create_default()` creates the standard isolated in-memory shell.
`bashkit_create_json()` accepts UTF-8 JSON with this v1 shape:

```json
{
  "schema_version": 1,
  "cwd": "/workspace",
  "env": {"CI": "true"},
  "files": {"/workspace/input.txt": "hello\n"},
  "limits": {
    "timeout_ms": 30000,
    "parser_timeout_ms": 5000,
    "max_commands": 10000,
    "max_input_bytes": 10000000,
    "max_output_bytes": 1048576
  },
  "username": "sandbox",
  "hostname": "bashkit",
  "readonly_filesystem": false,
  "capture_final_env": true
}
```

Unknown fields and schema versions are rejected. Text files may be initialized
through `files`; use `bashkit_write_file()` for arbitrary bytes. Configuration
input is capped at `BASHKIT_MAX_CONFIG_BYTES` (10 MB).

## Contract

- Inputs are borrowed only for the duration of a call.
- `out_error` may be `NULL` when the caller does not need diagnostic text.
- Result, buffer, and error byte views remain valid until their owner is freed.
- Call only Bashkit's matching free function; never pass Bashkit memory to the
  host allocator.
- A nonzero shell exit code is not an ABI failure. `bashkit_execute()` returns
  `BASHKIT_OK`, and the code is available through
  `bashkit_result_exit_code()`.
- Calls on one `Bashkit` instance serialize. Separate instances may run in
  parallel. The caller must not race `bashkit_free()` with another call.
- A null pointer is valid only where documented or when the accompanying byte
  length is zero. Other invalid pointers are caller undefined behavior.
- No Rust panic unwinds through the ABI. Unexpected panics become
  `BASHKIT_INTERNAL_ERROR` where the function can report a status.

See [`include/bashkit.h`](include/bashkit.h) for the complete surface and the
[C API guide](../../docs/c-api.md) for compatibility and scope.
