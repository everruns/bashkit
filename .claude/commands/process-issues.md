Resolve all open GitHub issues. Each issue gets its own PR. Do not stop until every issue is resolved or deferred.

## Arguments

- `$ARGUMENTS` - Optional: specific issue number(s) to process. If omitted, process all open issues.

## Goals

Each goal below is an outcome to achieve per issue, not a script to follow. Use whatever tools and approaches make sense.

### 1. Every actionable issue has a merged PR

For each open issue, ordered by issue number, produce a **separate PR** that closes it.

**CRITICAL: One issue = one PR. Never bundle multiple issues into a single PR. This is a hard requirement — no exceptions.**

Only process issues authored by `chaliy` or approved by `chaliy` in comments. Skip all others silently.

Key tools: `gh issue list --state open`, `gh issue view`

### 2. Every bug fix proves the bug existed

A failing test exists before the fix is applied. Verify the failure. Then fix. This is non-negotiable.

For features, an acceptance test defines expected behavior before implementation.

### 3. Every change is hardened

Negative tests, edge cases, and security tests exist where applicable (parser, VFS, sandbox, permissions).

Key references: `specs/006-threat-model.md`, `specs/005-security-testing.md`

### 4. Specs and docs reflect reality

If behavior changed, the relevant spec in `specs/` is updated. `specs/009-implementation-status.md` reflects current feature status.

### 5. Every PR passes quality gates

`just pre-pr` is green before creating the PR. CI is green before merging.

### 6. Ignored tests are reviewed

After all issues are processed, `#[ignore]` tests are scanned. Any that now pass are un-ignored in a separate PR.

## Execution

- Branch from latest main per issue
- Conventional commits referencing `Closes #N`
- Squash-merge each PR, return to main before starting the next issue
- Unclear or non-reproducible issues → comment asking for clarification, skip to next
- Too large (>500 lines) → split into sub-issues, link them
- Use parallel agents for independent issues when possible
