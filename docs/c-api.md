# C API

Bashkit's experimental C ABI embeds the sandbox in native applications and
provides a foundation for external language bindings. It is synchronous,
binary-safe, and based on opaque handles.

## Install

Prebuilt archives are attached to each
[GitHub Release](https://github.com/everruns/bashkit/releases):

| Target | Archive |
|---|---|
| macOS Apple Silicon | `bashkit-capi-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `bashkit-capi-x86_64-apple-darwin.tar.gz` |
| Linux x86-64 | `bashkit-capi-x86_64-unknown-linux-gnu.tar.gz` |
| Linux ARM64 | `bashkit-capi-aarch64-unknown-linux-gnu.tar.gz` |
| Windows x86-64 | `bashkit-capi-x86_64-pc-windows-msvc.zip` |

Download the adjacent `.sha256` file and verify it before extracting. Each
archive contains the public header, both examples, notices, and the matching
native library. Windows archives also contain the `bashkit.lib` import library.
No Rust toolchain is required when using a release archive.

The complete platform-specific compile and deployment commands are in the
[`bashkit-capi` README](../crates/bashkit-capi/README.md).

## Build from source

```sh
./scripts/build-c-api.sh --release
```

The shared library is `libbashkit.so` on Linux,
`libbashkit.dylib` on macOS, and `bashkit.dll` on Windows. It contains the
complete Bashkit runtime; no second Bashkit library is required. Include
[`bashkit.h`](../crates/bashkit-capi/include/bashkit.h) in the host application.
The Cargo adapter target remains internally named `bashkit_capi` to avoid
colliding with Bashkit's Rust and Python workspace artifacts.

Source builds require the repository's pinned Rust toolchain and a C compiler.
The helper supports macOS and Linux and writes the native consumer artifact to
`target/c-api/release/`.

## Link and deploy

On Linux and macOS, compile with the extracted `include/` and `lib/` directories:

```sh
cc -std=c11 -I "$BASHKIT_SDK/include" app.c \
  -L "$BASHKIT_SDK/lib" -lbashkit -o app
```

At runtime the operating-system loader must be able to find `libbashkit.so` or
`libbashkit.dylib`. Prefer deploying it in an application-local `lib/` directory
and embedding `$ORIGIN/lib` on Linux or `@loader_path/lib` on macOS, as shown in
the packaged README. On Windows, link `bashkit.lib` and put `bashkit.dll` beside
the executable or on `PATH`.

Applications and libraries must use the same architecture. The current Linux
archives target GNU libc; a dynamic Bashkit library is self-contained with
respect to Bashkit and Rust dependencies but still uses normal platform system
libraries.

### CMake

Set `BASHKIT_ROOT` to the extracted archive directory:

```cmake
add_library(bashkit SHARED IMPORTED)
target_include_directories(my_app PRIVATE "${BASHKIT_ROOT}/include")

if(WIN32)
  set_target_properties(bashkit PROPERTIES
    IMPORTED_LOCATION "${BASHKIT_ROOT}/lib/bashkit.dll"
    IMPORTED_IMPLIB "${BASHKIT_ROOT}/lib/bashkit.lib")
elseif(APPLE)
  set_target_properties(bashkit PROPERTIES
    IMPORTED_LOCATION "${BASHKIT_ROOT}/lib/libbashkit.dylib")
else()
  set_target_properties(bashkit PROPERTIES
    IMPORTED_LOCATION "${BASHKIT_ROOT}/lib/libbashkit.so")
endif()

target_link_libraries(my_app PRIVATE bashkit)
```

The application must still deploy the runtime library where the platform loader
can locate it.

## Run the examples

The repository includes two real C programs:

```sh
./scripts/run-c-api-examples.sh
```

[`hello.c`](../crates/bashkit-capi/examples/hello.c) demonstrates instance and
result lifetimes by printing the virtual user and current UTC time.
[`files.c`](../crates/bashkit-capi/examples/files.c)
demonstrates JSON configuration and binary-safe VFS exchange.

## Surface

The v1 ABI provides:

- library ABI, package version, and compiled-capability queries;
- default or JSON-configured Bashkit construction;
- synchronous script execution;
- exit code, exact stdout bytes, UTF-8 stderr, truncation flags, and optional final
  environment JSON;
- virtual filesystem read, write, directory creation, and removal;
- explicit result, buffer, error, and instance destruction.

All strings enter as a pointer plus length. Scripts, paths, configuration, and
stderr are UTF-8; file contents and exact stdout may contain arbitrary bytes. A
null pointer with zero length represents an empty byte sequence. Construction
configuration is capped at `BASHKIT_MAX_CONFIG_BYTES` (10 MB).
The versioned JSON accepts the closed profile names `hardened`, `standard`, and
`interactive`; individual `limits` fields override the selected baseline.

## Errors and shell status

An ABI status describes whether Bashkit produced a result. A script ending in
`exit 7` is a successful ABI operation whose `BashkitResult` has exit code 7.
Invalid arguments, invalid UTF-8, invalid configuration, interpreter failures,
and VFS failures return an owned `BashkitError`.

Error messages are capped at 1 KiB. Free them with `bashkit_error_free()`.

## Ownership and concurrency

The library allocates every opaque object. Release each object with its matching
function; do not use the host's `free()` or retain borrowed byte views after the
owner is released.

Calls against one Bashkit instance are serialized. Independent instances may
run concurrently. The caller owns handle lifetime synchronization and must not
free a handle while another thread is using it.

## Initial exclusions

ABI v1 does not expose callbacks, custom builtins, streaming output, async
execution, host filesystem mounts, transport hooks, snapshots, or the existing
cross-addon filesystem-handle ABI. These require separate callback, reentrancy,
and library-unload contracts.

## Compatibility

Opaque object layouts are private. Existing functions, numeric status values,
ownership rules, and v1 configuration meanings will not change incompatibly.
New functions and status values may be added. An incompatible contract requires
a new ABI major rather than changing v1 in place.

When loading Bashkit dynamically, call `bashkit_abi_version()` and require
`BASHKIT_ABI_VERSION_1` before using the rest of the API. The package version
returned by `bashkit_version()` is informational and is independent of the ABI
major.

## Troubleshooting

- Linux `libbashkit.so: cannot open shared object file`: set an application
  rpath or point `LD_LIBRARY_PATH` at the library directory.
- macOS `Library not loaded: @rpath/libbashkit.dylib`: add an application rpath
  or point `DYLD_LIBRARY_PATH` at the library directory.
- Windows error 126: place `bashkit.dll` beside the executable and ensure the
  executable and DLL are both x86-64.
- Undefined `bashkit_*` link symbols: place `-lbashkit` after object inputs on
  Unix and use a header and library from the same release.

## See also

- [Get started](start.md) — choose another Bashkit package or runtime.
- [Sandbox configuration and limits](configuration.md) — common sandbox controls.
- [Security](security.md) — trust boundaries and resource protections.
