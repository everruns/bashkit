---
name: ship
description: Validate, review, and ship Bashkit changes through a green squash merge. Use for ship, ship it, fix and ship, push and merge, or Bashkit maintenance.
user_invocable: true
metadata:
  internal: true
---

# Ship Bashkit changes

Use the task context to scope validation. For “fix and ship,” implement the
fix first.

## Prepare

Work on a branch off current `origin/main`. Create a branch for detached
work. Include task-owned edits and pause if unrelated changes would be
included.

## Review, test, and smoke-test

- Review the complete diff and exercise the affected entry points. Cover
  success, errors, and relevant edge cases. Fix failures and rerun the
  relevant checks.
- Apply the [security testing strategy](../../../knowledge/security/security-testing.md)
  for changes to user input, parsing, execution, VFS, network, git, or
  permissions. Review input validation, sandbox boundaries, resource
  limits, diagnostics, and the soundness of changed unsafe code.
- Compare changed Bash behavior against real Bash. Smoke-test changed CLI
  commands and interpreter examples through `just run` or `just run-script`.
  Invoke changed tool interfaces end to end.
- Review the design for dead code, duplication, unnecessary abstraction,
  premature generalization, and weak boundaries. Keep errors typed where
  callers need to act on them.
- Run the applicable benchmarks.
  Use [parallel execution](../../../knowledge/foundations/parallel-execution.md#benchmark)
  for concurrency, interpreter, or builtin changes, `just bench-sqlite` for
  SQLite or its VFS/IO bridge, and the
  [snapshot history benchmark](../../../knowledge/foundations/snapshot-history.md#tests)
  for snapshot, commit, or checkout changes. Treat the golden snapshot
  corpus as read-only. Use `just bench` for broader interpreter, builtin, or
  tool performance changes. Follow the
  [benchmark artifact rules](../../../knowledge/operations/performance-results.md).

## Update artifacts

- Follow the [knowledge contract](../../../knowledge/knowledge-contract.md).
  For new threats, add a stable entry to the
  [threat model](../../../knowledge/security/threat-model.md) and a matching
  `THREAT[TM-…]` mitigation comment.
- Update [known limitations](../../../knowledge/operations/limitations.md) when
  behavior changes them. Run `just regen-builtins` when the builtin inventory
  changes.
- Update affected [public guides and Rustdoc](../../../knowledge/operations/documentation.md).
  Update [agent guidance](../../../AGENTS.md) if its entry points or guardrails change.

## Validate

Rebase on current `origin/main` and rerun affected checks. Run `just pre-pr`,
`just test`, and `cargo clippy --all-targets --all-features -- -D warnings`.
Use `just python-lint` for Python code changes. Follow the
[feature slices](../../../knowledge/operations/testing.md#local-test-commands)
for targeted runs. Preserve the existing tests, audit policy, and hooks.

## Publish and merge

1. Push the branch and create or update the PR using the
   [PR template](../../../.github/pull_request_template.md) and the
   [commit and PR rules](../../../AGENTS.md#commits-and-prs).
2. Use `Closes #N` or `Fixes #N` only when every task in the issue is complete.
   Use `Part of #N` and list the remaining work for partial fixes.
3. Wait in bounded intervals for CI, diagnose failures, and resolve review
   findings. Squash-merge after all required checks pass and CI is green.
4. Verify the PR actually merged and report its URL.
5. Never merge an external contribution (a PR from a non-maintainer or a fork):
   report it ready and stop. See
   [external contributions](../../../knowledge/operations/external-contributions.md).
