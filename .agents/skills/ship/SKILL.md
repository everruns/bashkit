---
name: ship
description: Run the full ship flow — verify quality, ensure test coverage, update artifacts, smoke test, push, create PR, and merge when CI is green. Trigger when user says "ship", "ship it", "fix and ship", or asks to push and merge a branch.
user_invocable: true
---

Run the full ship flow: verify quality, ensure test coverage, update artifacts, smoke test, then push, create PR, and merge when CI is green.

This skill implements the complete "Shipping" definition and Pre-PR Checklist from AGENTS.md. When the user says "ship" or "fix and ship", execute ALL phases below — not just the push/merge steps.

## Arguments

- `$ARGUMENTS` - Optional: description of what is being shipped (used for PR title/body context and to scope the quality checks)

## Instructions

### Phase 1: Pre-flight

1. Confirm we're NOT on `main` or `master`
2. Confirm there are no uncommitted changes (`git diff --quiet && git diff --cached --quiet`)
3. If uncommitted changes exist, stop and tell the user

### Phase 2: Test Coverage

Review the changes on this branch (use `git diff origin/main...HEAD` and `git log origin/main..HEAD`) and ensure comprehensive test coverage:

1. **Identify all changed code paths** — every new/modified function, module, builtin, tool
2. **Verify existing tests cover the changes** — run `cargo test --all-features` and check for failures
3. **Write missing tests** for any uncovered code paths:
   - **Positive tests**: happy path, valid inputs, expected state transitions
   - **Negative tests**: invalid inputs, error conditions, boundary cases, permission failures, missing resources
   - **Security tests**: if change touches parser, interpreter, VFS, network, git, or user input — add tests per `specs/005-security-testing.md`
   - **Compatibility tests**: if change affects Bash behavior parity — add differential tests comparing against real Bash
4. **Run all tests** to confirm green: `just test`
5. If any test fails, fix the code or test until green

### Phase 3: Artifact Updates

Review the changes and update project artifacts where applicable. Skip items that aren't affected.

1. **Specs** (`specs/`): if the change adds/modifies behavior covered by a spec, update the relevant spec file to stay in sync
2. **Threat model** (`specs/006-threat-model.md`): if the change introduces new attack surfaces, external inputs, authentication/authorization changes, or data handling — add or update threat entries using the `TM-<CATEGORY>-<NNN>` format and add `// THREAT[TM-XXX-NNN]` code comments at mitigation points
3. **AGENTS.md**: if the change adds new specs, commands, or modifies development workflows — update the relevant section
4. **Implementation status** (`specs/009-implementation-status.md`): if feature status changed, update the status table
5. **Documentation** (`crates/bashkit/docs/`): if the change affects public APIs, tools, or features — update the relevant guide markdown files

### Phase 3b: Code Simplification

Review all changed code for opportunities to simplify:

1. **Identify duplication** — look for repeated patterns that could share a helper or be consolidated
2. **Reduce complexity** — simplify nested logic, long match arms, deeply indented blocks
3. **Remove dead code** — unused functions, unreachable branches, commented-out code
4. **Check naming** — ensure functions, variables, and types have clear, descriptive names
5. **Verify no over-engineering** — remove unnecessary abstractions, feature flags, or indirection that don't serve the current change

If simplification changes are made, loop back to Phase 2 to verify tests still pass.

### Phase 3c: Security Review

Analyze all changed code for security vulnerabilities:

1. **Input validation** — check that user-supplied data (script input, file paths, environment variables, command arguments) is validated before use
2. **Injection risks** — look for command injection, path traversal, environment variable injection, or shell metacharacter issues
3. **Sandbox escapes** — if changes touch VFS, builtins, or process execution, verify they cannot escape the sandbox (see `specs/006-threat-model.md`)
4. **Resource exhaustion** — check for unbounded loops, unbounded allocations, or missing limits on user-controlled sizes
5. **Error handling** — ensure errors don't leak internal state, file paths, or sensitive information
6. **Unsafe code** — review any `unsafe` blocks for soundness; prefer safe alternatives

If security issues are found, fix them, add regression tests, and update `specs/006-threat-model.md` if a new threat category is identified.

### Phase 3d: Design Quality Review

Review all changed code for shortcuts, lazy abstractions, and premature compromises. This is a greenfield project — correctness and clean design matter more than compatibility or speed of delivery. Take the time to find better abstractions.

1. **No shortcut abstractions** — reject copy-paste patterns disguised as "good enough". If two things look similar, determine whether they are *actually* the same concept. If yes, unify properly. If no, keep them separate with clear names — don't force a bad shared interface.
2. **No lazy wrappers** — every abstraction must earn its place. A wrapper that just forwards calls adds indirection without value. An enum variant that exists "just in case" is dead weight. If a layer doesn't add meaning, remove it.
3. **Right abstraction level** — check that traits, types, and module boundaries model the actual domain, not implementation accidents. A `StringOrList` enum is a parser leak; a `Pattern` type is a domain concept. Prefer the latter.
4. **No stringly-typed interfaces** — look for magic strings, string matching on variant names, ad-hoc parsing of structured data. Replace with enums, newtypes, or proper typed APIs.
5. **No premature generics** — a function generic over three trait bounds used in one call site is harder to read than a concrete function. Generalize only when there are (or will immediately be) multiple real callers.
6. **No compatibility shims** — this is greenfield. If an interface is wrong, change it. Don't add adapters, conversion layers, or deprecated alternatives. Fix call sites instead.
7. **Error types are first-class** — check that error enums are specific and actionable, not catch-all `Other(String)` buckets. Each variant should guide the caller's recovery logic.
8. **Module boundaries enforce invariants** — if a `pub` field or function lets outside code break a module's assumptions, tighten visibility. Constructors and accessors exist to protect invariants, not to be "nice".

If design issues are found, refactor, update tests (loop back to Phase 2), and update specs if the change alters documented behavior.

### Phase 4: Smoke Testing

Smoke test impacted functionality to verify it works end-to-end:

1. **CLI changes**: run `just run` with relevant commands, verify output
2. **Builtin/interpreter changes**: run example scripts via `just run-script <file>` to verify behavior
3. **Tool changes**: if LLM tool interface changed, run a quick tool invocation test
4. **Python bindings**: if Python code changed, run `ruff check crates/bashkit-python && ruff format --check crates/bashkit-python`

If smoke testing reveals issues, fix them and loop back to Phase 2 (tests must still pass).

### Phase 5: Quality Gates

```bash
git fetch origin main && git rebase origin/main
```

- If rebase fails with conflicts, abort and tell the user to resolve manually

```bash
just pre-pr
```

- If it fails, run `just fmt` to auto-fix, then retry once
- If still failing, stop and report

### Phase 6: Push and PR

```bash
git push -u origin <current-branch>
```

Check for existing PR:

```bash
gh pr view --json url 2>/dev/null
```

If no PR exists, create one:

- **Title**: conventional commit style from the branch commits
- **Body**: summary of What, Why, How, and what tests were added/verified
- Use `gh pr create`

If a PR already exists, update it if needed and report its URL.

### Phase 7: Wait for CI and Merge

- Check CI status with `gh pr checks` (poll every 30s, up to 15 minutes)
- If CI is green, merge with `gh pr merge --squash --auto`
- If CI fails, report the failing checks and stop
- **NEVER** merge when CI is red

### Phase 8: Post-merge

After successful merge:

- Report the merged PR URL
- Done

## Rules

- Phases 2-4 (tests, artifacts, simplification, security review, smoke testing) are the quality core — do NOT skip them.
- The `$ARGUMENTS` context helps scope which tests, specs, and smoke tests are relevant.
- For "fix and ship" requests: implement the fix first, then run `/ship` to validate and merge.
- **Never close a half-done issue.** If the PR only covers a subset of the issue's tasks/checkboxes, use `Part of #N` instead of `Closes #N` or `Fixes #N`. Only use closing keywords when every task in the issue is complete. Premature closure hides remaining work.
