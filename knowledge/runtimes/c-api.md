---
type: Interface Contract
title: C API
description: Versioned native C ABI, ownership rules, packaging, and compatibility boundaries.
tags:
  - bashkit
  - ffi
  - c-api
  - abi
---

# C API

## Status

Experimental ABI v1.

## Decision

`crates/bashkit-capi` is the general-purpose native boundary. It publishes the
canonical `libbashkit` shared library and exposes the checked-in
`include/bashkit.h` contract. Its Cargo target remains uniquely named
`bashkit_capi`; the build and release boundary renames the native artifact and
sets its loader identity. This avoids collisions with the Rust core rlib and
Python extension while keeping their developer experience unchanged. The existing
`bashkit::interop::fs` ABI remains a narrower cross-addon filesystem exchange
contract and is not the library entry point.

The ABI uses opaque library-owned handles, pointer-plus-length byte views,
fixed-width status values, and matching destruction functions. Rust object
layouts, allocators, traits, futures, and Tokio types never cross the boundary.
Each exported Rust function contains panics; fallible functions report a capped
error object rather than unwinding into the host.

The handle retains its configured script-size limit so execution rejects an
oversized byte view before dereferencing it or validating UTF-8. This preserves
the core input resource limit at the untrusted native boundary.

Execution is synchronous in v1. Each handle owns a current-thread Tokio runtime
and mutex-protected `Bash`; same-handle calls serialize, while distinct handles
can execute concurrently. Callers own lifetime synchronization and cannot race
destruction with use.

Construction supports defaults or a strict, versioned JSON schema. Binary file
data uses direct VFS functions rather than base64 configuration. Shell nonzero
exit codes are successful ABI calls represented in `BashkitResult`; ABI status
is reserved for boundary and execution failures.

## Compatibility

- ABI version is independent of the Bashkit package version.
- Existing v1 symbols, numeric values, ownership, and semantics do not change
  incompatibly.
- Opaque layouts may change at any time.
- Additive functions and status values are allowed.
- Breaking changes require a parallel ABI major.

## Deferred surface

Callbacks, custom builtins, streaming, async cancellation, host mounts,
transport hooks, snapshots, scripted tools, and external filesystem providers
remain outside v1. They need explicit reentrancy, callback lifetime, and dynamic
library unload rules before becoming permanent ABI.

## Verification

Rust contract tests cover success, shell failure, configuration, binary VFS
content, invalid UTF-8, pre-validation script limits, null outputs, and version
rejection. The C example runner compiles the public header under C11 with
warnings denied and executes two programs against the built shared library.

## See also

- [Architecture](../foundations/architecture.md), crate boundaries and async-first core.
- [Virtual Filesystem](../foundations/vfs.md), the separate cross-addon filesystem ABI.
- [Testing Strategy](../operations/testing.md), repository test organization.
- [Release Process](../operations/release-process.md), native artifact publication.
