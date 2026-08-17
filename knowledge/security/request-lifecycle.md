---
type: Interface Contract
title: Request Execution Lifecycle
description: Shared lifecycle and adversarial test contract for request-owned execution boundaries.
tags:
  - bashkit
  - security
  - lifecycle
  - cancellation
---

# Request Execution Lifecycle

Every `Bash::exec*` call owns one `ExecutionBudget` and one revocable capability
lease. The budget is the lifecycle authority for parser/interpreter work, embedded
runtimes, network transport, and host callbacks. The lease gates host facilities
that can be retained as handles, including extension values, custom-builtin VFS
views, host-call brokers, and callback context. A subsystem must not create a fresh
budget/lease or retain usable request authority after the call returns.

## State machine

`ExecutionBudget` has two externally relevant states:

1. **Active**: initialization and execution may check or charge the shared budget.
2. **Closed**: all checks and charges fail with the fixed `request closed` reason.

While active, cancellation, deadline, or quota exhaustion poisons the request. The
first poison remains authoritative for active descendants. Infallible RAII guards
transitions the budget to closed and the capability lease to revoked on success,
error, timeout, cancellation, unwind, or teardown failure. A new `exec*` call creates
a new pair; reset/reuse never reopens either member. Concurrent `Bash` requests have
distinct identities.

## Boundary rules

- Check before initialization and scalable synchronous work.
- Poll the budget while a synchronous VM/query loop runs.
- Await host-owned async work through `ExecutionBudget::run`; native targets poll
  cancellation, and every target checks again before accepting the result.
- Drop the awaited future on cancellation/deadline so callback/transport-owned RAII
  resources release. Budget byte leases release on `Drop`.
- Ignore late messages/results: a result produced after closure cannot pass the
  post-await check, and streaming callbacks are separately scoped by their drop guard.
- Expose request facilities only through `ExecutionCapability<T>` or a scoped facade.
  Retained handles return the fixed `execution capability revoked` error after lease
  revocation. `BuiltinRegistry::insert_trusted` is the explicit session-lived host
  VFS escape hatch; ordinary custom builtins remain request-scoped.
- Revoke before restoring the interpreter's extension slot. Cleanup drains only
  request-created capability cells (bounded by extension types and execution command
  limits), is idempotent, and reports sanitized status/counts rather than lock or
  callback diagnostics.
- Normalize cancellation to `Error::Cancelled` (`execution cancelled`) and deadlines
  to `LimitExceeded::Timeout`; callback diagnostics remain sanitized at the registry.

The 10 ms native cancellation poll bounds cooperative async cancellation latency.
Wasm has no reliable timer driver, so it checks before and after awaits and relies on
runtime checkpoints for synchronous work. No contract can preempt one non-cooperative
synchronous host callback or VM instruction; such code must return to a checkpoint.

## Surface inventory

| Surface | Initialization/execution checkpoints | Async boundary | Persistent state |
|---|---|---|---|
| Python/Monty | shared tracker plus suspend/resume checks | whole runtime future, including external functions | none per invocation |
| TypeScript/ZapCode | VM limits plus suspend/resume checks | whole runtime future, including external functions | snapshots die with invocation |
| SQLite/Turso | statement and row-step budget checks; statement interrupt on deadline | VFS awaits remain under top-level execution | file engine cache is session-owned; request budget is not cached |
| HTTP transport | shell admission and request budget checks | each transport request/redirect await | immutable transport may be shared; request future is not |
| runtime tool callbacks | registry admission and shared deadline | callback await in request scope | registry/callback set may be shared; tenant, trace, budget are request-owned |

## Adversarial evidence

`request_lifecycle_contract_tests` drives the same cancellation/drop/reuse harness
against Python, TypeScript, HTTP transport, and runtime tool callbacks; SQLite uses its
VM loop because it has no host callback suspension point. `execution_budget_tests`
covers late retained clones, repeated reuse, independent concurrent requests, and
monotonic request identity. `execution_capability_tests` covers positive access,
cancellation, retained and cross-request handles, scoped VFS, trusted-host access,
resource release, and cleanup failure/idempotence. Python's
`test_retained_ctx_fs_is_revoked_before_the_next_request` verifies the binding exposes
the same cross-request contract while preserving same-request lazy-provider re-entry.
Existing runtime deadline tests remain the surface-specific evidence for VM-native
timeout wording and interrupt behavior.

## See also

- [Threat Model](threat-model.md), TM-ISO-027 records stale request authority
- [Security Testing](security-testing.md), layered adversarial testing strategy
- [HTTP Transport](http-transport.md), host transport trust boundary
