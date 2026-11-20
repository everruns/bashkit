---
type: Playbook
title: Agent Pull Request Rules
description: When coding agents may merge pull requests and reply to review comments.
tags:
  - bashkit
  - operations
  - review
  - process
---

# Agent Pull Request Rules

These rules bind coding agents working on `everruns/bashkit`. They apply even
when CI is green, reviews are positive, and the agent was asked to review,
watch, or "ship" the pull request. Only an explicit instruction from a
maintainer for that specific pull request lifts them.

## Never merge external contributions

An external contribution is a pull request whose author is not a maintainer of
`everruns/bashkit`, including any pull request opened from a fork. Bot pull
requests (Dependabot, the drift workflows) are not external contributions.

Agents may:

- Review the pull request and report findings to the maintainer.
- Post review comments when the maintainer asks.
- Approve fork workflow runs when the maintainer asks, so CI runs.
- Report when CI is green and the pull request is ready, then stop.

Agents must not:

- Merge, enable auto-merge, or add the pull request to a merge queue.
- Promise a merge on the maintainer's behalf in a comment.
- Push commits to the contributor's branch, unless the maintainer asks for a
  specific change.

## Never merge with unaddressed review comments

Before merging any pull request, every review comment, from people and bots,
must be addressed:

- fixed in the pull request, or
- answered on its thread with why no change is needed, or
- explicitly deferred by a maintainer to a named follow-up.

An unresolved thread counts as unaddressed until one of those is true. An
outdated thread counts as addressed only if the code it points at was removed
or changed in a way that settles it; check, do not assume. Nits and optional
findings count too: fix them or answer them.

## Never auto-reply to human-written comments

Agents reply on their own only to comments written by bots (review bots,
Copilot, CI bots). A comment written by a person, whether a maintainer, a
contributor, or anyone else, is brought to the maintainer with a suggested
answer. The agent posts a reply only when the maintainer asks, and in the words
the maintainer approves.

## Why

On 2026-11-20 an agent squash-merged external pull request #2959 after CI went
green, without an explicit go-ahead to merge, and with an open review-bot
finding (stale `wasm32-unknown-unknown` docs in `time_compat`) still
unaddressed. Merging outside code is a trust decision about the contributor
and the change, not a CI outcome, and replies to people speak for the
maintainer, so both stay with a person.

## See also

- [Maintenance](maintenance.md), which runs merges of the agent's own pull requests
- [Threat Model](../security/threat-model.md)
