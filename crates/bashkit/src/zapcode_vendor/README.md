# Vendored ZapCode source

Upstream `zapcode-core` 1.5.1, MIT, https://github.com/TheUncharted/zapcode.
Every target (native, WASI, browser) builds this copy; the registry crate is
no longer a dependency. The copy replaces std clocks that panic on
wasm32-unknown-unknown and carries `bashkit fork:` resource-limit fixes
upstream has not shipped (listed in `scripts/check_zapcode_vendor.py`). It adds
no JS eval, imports, network or filesystem access.

`upstream-sha256.json` records every original source file. Mechanical change:
`crate::` becomes `crate::zapcode_vendor::`; `lib.rs` becomes `mod.rs`.
Clock changes in sandbox.rs/trace.rs use Bashkit's platform clock shim.
