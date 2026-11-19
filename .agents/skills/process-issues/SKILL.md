---
name: process-issues
description: Resolve qualifying open GitHub issues one at a time, with one shipped PR per issue. Use for process issues, work through issues, resolve issues, handle open issues, or fix all issues.
user_invocable: true
metadata:
  internal: true
---

# Process GitHub issues

Use the requested issue numbers or labels to scope the run. With no filter,
consider every open issue. Qualifying issues were created or approved in a
comment by `chaliy`. Skip other issues silently.

1. Work through qualifying issues in issue-number order. Identify the affected
   behavior and start `fix/issue-{N}-{short-slug}` from current `origin/main`.
   If an issue is unclear or cannot be reproduced, seek clarification on it
   and continue. If the fix would exceed 500 lines, split the work into linked
   sub-issues.
2. Add a failing regression test for bugs or a behavior test for features.
   Implement a focused fix for this issue.
3. Use [ship](../ship/SKILL.md) for review, validation, pull request, CI, and
   merge. Follow its issue-closing rules. Use one PR per issue.
4. Refresh main after the merge before starting the next issue. Continue
   until the qualifying issues are resolved or explicitly deferred.

Afterward, check whether previously ignored tests now pass. Collect any
verified removals of `#[ignore]` in one separate PR.
