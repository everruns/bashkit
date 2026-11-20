## Coding-agent guidance

- Start from the latest `origin/main`. Fetch and rebase or recreate stale worktrees before editing.
- Fix root causes. Preserve unrelated changes and ask only if they block the work. Keep changes small, runnable, and testable.
- For bugs, write a failing regression test first. Verify crate behavior before depending on it.
- Record important local decisions near the code. Private implementation details can change freely within the documented public contracts.
- Verify behavior end to end. Explain deferred work in a `TODO` or `WTF` comment. Keep messages concise.

### Knowledge and docs

Read the relevant [knowledge index](knowledge/index.md) entries before changing
behavior. Update the affected concepts in the same change. The
[knowledge contract](knowledge/knowledge-contract.md) defines the OKF rules and
`just check-okf` validates them.

Use the [documentation guide](knowledge/operations/documentation.md) for public
articles, Rustdoc, source-relative links, and agent-facing site artifacts.

### Local Dev

Use `just --list` to discover tasks, `just build` to build, and `just test` to
test. Run `just pre-pr` for the pre-PR gate.

**Never run `cargo test --all-features` in one invocation.** The combined link
can exhaust runner memory, and failpoints require a single test thread. See
[feature slices and test layout](knowledge/operations/testing.md#local-test-commands).
Use the pinned development toolchain in [rust-toolchain.toml](rust-toolchain.toml)
and preserve the independent [consumer Rust floor](knowledge/operations/testing.md#rust-toolchain-and-consumer-compatibility).

Run `cargo deny check` for dependency policy checks. Keep builtin diagnostic
output free of internal debug shapes and add regression coverage for new
library wrappers. See [builtin diagnostics and fuzzing](knowledge/security/security-testing.md#builtin-diagnostics-and-fuzzing).

### Cloud Agent Setup

Run `./scripts/init-cloud-env.sh` for `just` and `gh`, then `just build`.
When `DOPPLER_TOKEN` is available and `gh` needs credentials, load:

```bash
export GH_TOKEN=$(curl -fsS "https://api.doppler.com/v3/configs/config/secret?name=GITHUB_TOKEN" \
  -u "$DOPPLER_TOKEN:" | python3 -c "import sys,json; print(json.load(sys.stdin)['value']['raw'])")
```

Use the same endpoint with a different name for an example API key such as
`ANTHROPIC_API_KEY`.

### Maintenance invocation

Use [maintain](.agents/skills/maintain/SKILL.md) for maintenance requests.
The [maintenance playbook](knowledge/operations/maintenance.md#invocation-contract)
defines the default scope and the analysis-only option.

### Pre-PR Checklist

Use the [ship skill](.agents/skills/ship/SKILL.md) for test coverage,
affected benchmarks, artifact updates, and review. Resolve review findings
and **never merge while CI is red**.

### Commits and PRs

Use [Conventional Commits](https://www.conventionalcommits.org), with
`chore` for updates to `knowledge/` and `AGENTS.md`. Use the
[PR template](.github/pull_request_template.md) and squash-merge. Honor
requests for separate PRs. Keep session links out of commits and PR descriptions.

[Agent PR rules](knowledge/operations/agent-pr-rules.md), binding even with
green CI and when asked to review, watch, or ship:

- **Never merge external contributions.** A maintainer merges PRs from
  non-maintainers or forks by hand. Review, comment when asked, and report.
- **Never merge with unaddressed review comments.** Every comment, from people
  or bots, is fixed, answered on its thread, or deferred by a maintainer.
- **Never auto-reply to human-written comments.** Reply on your own only to
  bots; bring a person's comment to the maintainer with a suggested answer.

### Commit Attribution

Attribute every commit, including the squash commit, to the real human
user. Omit AI co-author trailers and generated-by text. Verify
`git config user.name` and `git config user.email` before committing. If
missing or bot-like, set them from `$GIT_USER_NAME` and `$GIT_USER_EMAIL`.
If those values are unavailable, ask for the human identity.
