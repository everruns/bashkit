---
type: Playbook
title: External Contributions
description: Agents never merge pull requests from external contributors; a maintainer merges them by hand.
tags:
  - bashkit
  - operations
  - review
  - process
---

# External Contributions

## Rule

Coding agents must not merge an external contribution. An external
contribution is a pull request whose author is not a maintainer of
`everruns/bashkit`, including any pull request opened from a fork. Bot pull
requests (Dependabot, the drift workflows) are not external contributions.

This holds even when CI is green, reviews are positive, and the agent was asked
to review, watch, or "ship" the pull request. A maintainer merges it by hand.

## What agents may do

- Review the pull request and report findings to the maintainer.
- Post review comments when the maintainer asks.
- Approve fork workflow runs when the maintainer asks, so CI runs.
- Report when CI is green and the pull request is ready, then stop.

## What agents must not do

- Merge, enable auto-merge, or add the pull request to a merge queue.
- Promise a merge on the maintainer's behalf in a comment.
- Push commits to the contributor's branch, unless the maintainer asks for a
  specific change.

## Why

On 2026-11-20 an agent squash-merged external pull request #2959 after CI went
green, without the maintainer's explicit go-ahead to merge. Merging outside
code is a trust decision about the contributor and the change, not a CI
outcome, so it stays with a person.

## See also

- [Maintenance](maintenance.md), which runs merges of the agent's own pull requests
- [Threat Model](../security/threat-model.md)
