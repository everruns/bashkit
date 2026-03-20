---
name: process-issues
description: Resolve all open GitHub issues. Each issue becomes exactly one shipped PR. Trigger when user says "process issues", "work through issues", "resolve issues", "handle open issues", "fix all issues", or asks to resolve GitHub issues end-to-end.
user_invocable: true
---

Resolve all qualifying open GitHub issues. Each issue becomes exactly one merged PR. Do not stop until every issue is resolved or explicitly deferred.

## Arguments

- `$ARGUMENTS` - Optional: specific issue numbers (e.g. "42 55") or labels. If omitted, process all open issues.

## Goal

Every qualifying open issue has a merged PR that resolves it. **One issue = one PR. No bundling. No skipping.**

## Qualifying issues

Only process issues that meet ONE of:
- Created by `chaliy`
- Has a comment from `chaliy` approving it

Skip all others silently.

## Per-issue outcomes

For each qualifying issue (ordered by issue number), achieve ALL of these before moving to the next:

### 1. Issue is understood and scoped

- Classify: bug, feat, test, chore, refactor, docs
- Identify affected areas: parser, interpreter, builtins, vfs, network, git, python, tool, eval, security
- Branch created from latest main: `fix/issue-{N}-{short-slug}`

### 2. Failing test exists (bugs) or scaffold test exists (features)

- A test that demonstrates the bug or validates the feature exists in `crates/bashkit/tests/spec_cases/` or relevant module
- For bugs: test fails before the fix, passes after

### 3. Fix or feature is implemented

- Minimal, focused changes
- Positive and negative tests pass
- Security tests added if change touches parser, interpreter, VFS, network, git, or user input (per `specs/005-security-testing.md`)
- Threat model updated if new attack surface (per `specs/006-threat-model.md`)

### 4. Ship via `/ship`

Invoke the `/ship` skill to handle all remaining steps: spec updates, artifact updates, code simplification, security review, smoke testing, quality gates, PR creation, CI, and merge.

Pass context: `Closes #N — <short description of the fix/feature>`

Do NOT duplicate any `/ship` steps manually — let the skill handle the full pipeline.

After `/ship` completes and the PR is merged, return to main before next issue: `git checkout main && git pull origin main`

## After all issues

Scan for `#[ignore]` tests that may now pass. Un-ignore any that are green. Single PR for all un-ignored tests.

## Rules

- **One issue = one PR.** Non-negotiable. Never bundle multiple issues.
- If an issue is unclear or not reproducible, comment asking for clarification and skip to next.
- If a fix would be >500 lines, split into sub-issues and link them.
- Never skip the failing-test-first step for bugs.
- Always rebase on latest main between issues.
