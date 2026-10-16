# Browser ZapCode source

Upstream `zapcode-core` 1.5.1, MIT, https://github.com/TheUncharted/zapcode.
Native builds continue to use the registry crate and its original value types.
The browser copy allows replacing unsupported std clocks while preserving VM
resource enforcement; it adds no JS eval, imports, network or filesystem access.

`upstream-sha256.json` records every original source file. Mechanical change:
`crate::` becomes `crate::zapcode_browser::`; `lib.rs` becomes `mod.rs`.
Clock changes in sandbox.rs/trace.rs use Bashkit's platform clock shim.
