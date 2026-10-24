---
type: Package Design
title: Node Package
description: Native JavaScript bindings and embedded-runtime callback execution across Node, Bun and Deno.
tags:
  - bashkit
  - javascript
  - callbacks
---

# Node package host functions

`@everruns/bashkit` enables Python and TypeScript independently. Both `Bash`
and `BashTool` accept runtime names and dispatchers; all JS agent adapters
forward these options. LangChain awaits execution and forwards RunnableConfig's
AbortSignal. Python dispatch includes kwargs; TypeScript uses positional values.

Foreign runtime values use the shared core `runtime_value` JSON codec: depth 64,
finite numbers, signed 64-bit integers, string-key dictionaries, arrays and scalars.
The JavaScript boundary additionally rejects integers outside the safe range.
Unsupported values fail instead of stringifying. Undefined maps to null.
Native Rust callbacks retain the language-specific value APIs.

Each async execution binds an AsyncResource before entering the bounded JS queue.
This preserves caller AsyncLocalStorage across NAPI and queued requests. Its
revocable execution extension owns a weak NAPI threadsafe dispatcher. Completion,
timeout or cancellation drops native waits and aborts the JS session's signal.
Host I/O must observe that signal; arbitrary application side effects cannot be
rolled back. Request context and callbacks are never snapshot data.
Live byte sinks and explicit capture install the same execution-local host-function
lease as buffered calls. Both paths use the async/sync admission guard; queued
Python/TypeScript callbacks preserve caller context and revoke their signals when
the execution and sinks finish.
Live sink callbacks bind an AsyncResource before crossing NAPI, preserving the
same caller context before and after awaiting downstream work.

An AsyncLocalStorage owner marker rejects same-instance callback reentry,
including live filesystem handles, while independent async requests queue. Native
async-execution and sync-entry counters close the race where synchronous access
blocks the loop just as a handler or streaming output callback starts. Blocking
access fails during asynchronous execution; asynchronous entry waits for any
earlier synchronous entry to finish. Sync host calls fail explicitly;
ordinary synchronous commands remain available. Separate worker-thread environments
own separate dispatchers; weak references do not keep Node alive at shutdown.
If worker termination drops the last private-runtime owner inside an async task,
the owner uses Tokio's nonblocking shutdown instead of Runtime::drop (which
panics in that context). Ordinary thread teardown keeps graceful runtime drop.
A subprocess regression captures native stderr while terminating workers during
pending and CPU-bound Python/TypeScript host callbacks; completed ScriptedTool
worker teardown is covered too. ScriptedTool retains its synchronous callback
ABI; force-interrupting V8 inside such a native callback frame is outside this
async host-function shutdown contract.

`runtime-functions.spec.ts` exercises real HTTP request contexts, aborts, timeout,
queued calls, worker threads, adapters, reentry, bad values and snapshot/reset.
`runtime-compat/runtime-functions.test.mjs` runs in Node, Bun and Deno. Dedicated
macOS and Windows CI runs the callback suite as well as Linux's runtime matrix.

## Host runtime security

Build/release jobs use Node 24.22.1 and general CI uses 22.23.4, the patched
October 2026 releases. The runtime matrix explicitly tests both versions.
Node 20 remains an EOL compatibility-only lane; its green result does not imply
security support. The npm `engines >=18` field is the native ABI compatibility
floor, not a security-supported runtime list. Consumers supply Node themselves
and must use a currently patched supported runtime. No Node executable is bundled.

## See also

- [Python Package](python-package.md)
- [Browser Package](browser-package.md)
- [Capability Parity](../status/capability-parity.md)
- [Threat Model](../security/threat-model.md)
