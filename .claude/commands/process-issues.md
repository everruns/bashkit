Process all open GitHub issues end-to-end. Do not stop until every issue is resolved or explicitly deferred.

## Workflow

For each open issue (`gh issue list --state open`), in order by issue number:

### 0. Filter — owner/approval gate
- Only process issues that meet ONE of:
  - Created by `chaliy` (`gh issue view N --json author --jq '.author.login'`)
  - Has a comment from `chaliy` approving it (`gh api repos/{owner}/{repo}/issues/N/comments --jq '.[].author.login' | grep -q chaliy`)
- Skip all other issues silently — do not comment, do not close, just move to next

### 1. Understand
- Read the issue body and classify: bug, feat, test, chore, refactor, docs
- Identify affected areas: parser, interpreter, builtins, vfs, network, git, python, tool, eval, security
- Create a branch from main: `fix/issue-{N}-{short-slug}`

### 2. Reproduce (bugs) / Scaffold (features)
- Write a failing test first — spec test in `crates/bashkit/tests/spec_cases/` or unit test in the relevant module
- Verify the test actually fails before proceeding

### 3. Implement
- Fix the bug or implement the feature
- Keep changes minimal and focused

### 4. Threat model review
- If the change touches parser, interpreter, VFS, network, git, or user input:
  - Check `specs/006-threat-model.md` for applicable threat IDs
  - Add mitigations or update existing entries if needed
  - Add security tests in `tests/threat_model_tests.rs` if new attack surface

### 5. Exception & edge-case tests
- Add negative tests: invalid input, empty input, boundary values, error paths
- Add security tests if touching sandboxing or permissions (`specs/005-security-testing.md`)
- Ensure both positive and negative scenarios covered

### 6. Update specs
- If behavior changes, update the relevant spec in `specs/`
- Update `specs/009-implementation-status.md` if feature status changed

### 7. Verify
- Run `just pre-pr` (fmt + clippy + test + vet)
- Fix any failures before proceeding

### 8. Commit & PR
- Commit with conventional format: `type(scope): description` — reference `Closes #N`
- Push branch, create PR with summary + test plan
- Wait for CI green

### 9. Merge & close
- Squash-merge the PR (`gh pr merge --squash --delete-branch`)
- Add resolution comment on the issue if helpful
- Rebase on latest main before starting next issue: `git checkout main && git pull origin main`
- Move to next issue

### 10. Review ignored tests
After all issues are processed, scan for `#[ignore]` tests across the codebase:
- `grep -rn '#\[ignore\]' crates/` to find all ignored tests
- For each ignored test, determine why it was ignored (read surrounding comments, git blame)
- Classify into: (a) can un-ignore now — underlying issue fixed, (b) blocked — still needs work, create/link issue, (c) intentionally ignored — e.g. slow, requires external resource
- Un-ignore tests in category (a), run them, verify they pass
- For category (b), ensure a tracking issue exists
- Commit any un-ignored tests as `test: un-ignore {test_name}, now passing`
- Create a single PR for all un-ignored tests (separate from issue PRs)

## Rules
- One issue = one PR. Do not bundle.
- If an issue is unclear or not reproducible, comment asking for clarification and skip to next.
- If a fix would be too large (>500 lines), split into sub-issues and link them.
- Never skip the failing-test-first step for bugs.
- Return to main between issues: `git checkout main && git pull origin main`
