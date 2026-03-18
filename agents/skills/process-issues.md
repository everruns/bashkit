Resolve all open GitHub issues. Each issue becomes exactly one shipped PR. Do not stop until every issue is resolved or explicitly deferred.

Trigger when: user says "process issues", "work through issues", "resolve issues", "handle open issues", "fix all issues", or any variation asking to process/resolve GitHub issues end-to-end.

## Arguments

- `$ARGUMENTS` - Optional: filter to specific issue numbers (e.g. "42 55") or labels. If omitted, process all open issues.

## Goal

Every qualifying open issue has a merged PR that resolves it. One issue = one PR. No bundling. No skipping.

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

### 4. Specs are in sync

- Relevant specs in `specs/` updated if behavior changed
- `specs/009-implementation-status.md` updated if feature status changed

### 5. Quality gates pass

- `just pre-pr` green (fmt + clippy + test)
- Fix any failures before proceeding

### 6. PR is merged

- Commit with conventional format: `type(scope): description` referencing `Closes #N`
- PR created with summary + test plan
- CI green
- Squash-merged (`gh pr merge --squash --delete-branch`)
- Return to main before next issue: `git checkout main && git pull origin main`

## After all issues

Scan for `#[ignore]` tests that may now pass. Un-ignore any that are green. Single PR for all un-ignored tests.

## Rules

- **One issue = one PR.** This is non-negotiable. Never bundle multiple issues.
- If an issue is unclear or not reproducible, comment asking for clarification and skip to next.
- If a fix would be >500 lines, split into sub-issues and link them.
- Never skip the failing-test-first step for bugs.
- Always rebase on latest main between issues.
