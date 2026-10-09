# Wasm Coreutils (real uutils programs)

> **Experimental.** Behind the `wasm-coreutils` cargo feature, off by
> default.

Bashkit can run real [uutils/coreutils](https://github.com/uutils/coreutils)
programs instead of its own reimplementations. The programs are compiled to
WebAssembly (`wasm32-wasip1`), precompiled at build time and shipped inside
the binary. Each call runs in a fresh, isolated instance that can only reach
the Bashkit virtual filesystem and its own stdio, so many tenants can share
one process.

**See also:**
- [Embedded CPython (WebAssembly)](./cpython.md) - Real CPython on the same WASI host
- [Threat Model](./threat-model.md) - Security considerations (TM-WCU-*)
- [Compatibility Reference](./compatibility.md) - Bash feature support
- [`knowledge/runtimes/wasm-coreutils.md`][spec] - Design, measurements and decisions

## Quick start

```toml
[dependencies]
bashkit = { version = "0.18.2", features = ["wasm-coreutils"] }
```

```rust
use bashkit::Bash;

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder().wasm_coreutils().build();

// `pathchk` has no native builtin: the uutils program fills the gap.
let r = bash.exec("pathchk -p report_2026.txt && echo ok").await?;
assert_eq!(r.stdout, "ok\n");

// Any uutils program, also ones Bashkit implements natively, by name.
let r = bash.exec("printf '1K\\n3M\\n2\\n' | coreutils sort -h").await?;
assert_eq!(r.stdout, "2\n1K\n3M\n");
# Ok(())
# }
```

## What gets registered

- **`coreutils <utility> [args...]`**: runs any of the 71 embedded utilities;
  `coreutils --list` prints them.
- **Missing utilities by name**: `csplit`, `dir`, `dircolors`, `pathchk`,
  `ptx`, `shred`, `vdir` (the ones with no native builtin).
- Native builtins (`cat`, `sort`, `ls`, ...) stay native: they are much
  faster and stream. To route every embedded utility to its uutils program
  instead (GNU-compatible options and output), use
  `BashBuilder::wasm_coreutils_replace_native(limits)`.

The module loads on first use (one-time engine and module setup). Call
`bashkit::WasmCoreutil::warm_up()` at process start to move that cost out of
the first request.

## Limits

```rust
use bashkit::{Bash, WasmCoreutilsLimits};
use std::time::Duration;

let bash = Bash::builder()
    .wasm_coreutils_with_limits(
        WasmCoreutilsLimits::default()
            .max_duration(Duration::from_secs(2)) // wall clock per call (default 10 s)
            .max_memory(32 * 1024 * 1024)         // guest memory (default 64 MiB, max 256 MiB)
            .max_output(1024 * 1024),             // stdout + stderr bytes (default 16 MiB)
    )
    .build();
```

- **Time**: a call past its deadline stops with exit code 124 and
  `<util>: execution timed out ...`. A tighter Bashkit `ExecutionLimits`
  timeout or cancellation wins.
- **Memory**: allocation past the cap fails inside the program.
- **Output**: bytes past the cap are dropped and stderr ends with
  `<util>: output truncated at N bytes`.
- **Files**: Bashkit filesystem limits apply.

## Isolation

The programs never run native code in your process: they execute on
Wasmtime's Pulley interpreter, and the only things they reach are the
virtual filesystem, captured stdin/stdout/stderr, clocks and a random
source. Only exported shell variables are passed in; `PWD` becomes the
working directory. A crash ends only that call (`<util>: fatal error: ...`).

## Limitations

- 71 utilities: uutils' WebAssembly-buildable set. `stat`, `du`, `df`, `id`,
  `install`, `chown`, `timeout` and `tac` are not included.
- `ls -l` shows placeholder owners and permission bits (WASI has no file
  modes).
- Output is captured, not streamed.
- Programs run interpreted: much slower than native builtins on large
  inputs (see the measurements in the [design notes][spec]).

[spec]: https://github.com/everruns/bashkit/blob/main/knowledge/runtimes/wasm-coreutils.md
