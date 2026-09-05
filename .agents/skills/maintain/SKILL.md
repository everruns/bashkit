---
name: maintain
description: Analyze, fix, validate, and ship Bashkit maintenance through green CI and merge. Trigger on run maintenance, maintain, maintainace, or maintaiance; use analysis-only mode only when explicitly requested.
---

# Bashkit maintenance

A request to run maintenance authorizes the full outcome: analyze the repository,
fix findings, validate, push, open a PR, resolve review/CI failures, and squash-merge
when every required check is green. A local commit or a list of follow-up issues
is not completion. Explicit analysis-only requests omit edits and shipping.

Read `knowledge/operations/maintenance.md` in the Bashkit checkout and execute its
checklist, then use the repository's `ship` skill. Sync from latest origin/main
before editing. Use parallel agents for independent reviews and fixes when useful.

Keep findings in the active pass and fix root causes. A large diff, missing audit,
failed local check, or long build is work to complete, not a reason to defer.
Use a suitable environment for platform-specific checks. Never weaken tests,
security limits, or supply-chain criteria to ship an upgrade. If the newest
upstream version cannot preserve a required security contract, establish the
incompatibility, retain the newest safe version with a tested/documented pin,
and complete the rest of the pass; do not replace security checks with weaker ones.

Do not create deferral issues instead of completing maintenance unless the user
explicitly requests a deferred scope or a genuine external blocker requires it.
Only stop for an actual missing permission, unavailable dependency/service, or
user decision that cannot be resolved within the authorized task. Report concrete
evidence and continue independent work. CI failures require diagnosis and fixes;
never merge red CI or claim success while a required check is unfinished.

The final response identifies the merged PR, key fixes, verification, and any
explicitly accepted upstream constraints. Keep the spec's pass record factual.
