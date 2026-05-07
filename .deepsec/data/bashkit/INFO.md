# bashkit

## What this codebase does

Bashkit is a Rust Cargo workspace providing an in-process virtual Bash
interpreter for AI agents and multi-tenant sandboxes. Core code lives in
`crates/bashkit/` with parser, interpreter, virtual filesystem, builtins,
network, git, ssh, embedded Python/TypeScript/SQLite, and `BashTool`; bindings
live in `crates/bashkit-python/` and `crates/bashkit-js/`. The security goal is
running untrusted shell scripts without host process spawning, host filesystem
access, uncontrolled network access, secret leakage, or tenant cross-talk.

## Auth shape

- `NetworkAllowlist` gates all HTTP access used by `curl`, `wget`, and `http`.
- `HttpClient::set_bot_auth` and `BotAuthConfig` add transparent Ed25519
  request signing for allowlisted outbound HTTP when the feature is enabled.
- `CredentialPolicy`, `Credential`, `credential`, and `credential_placeholder`
  inject headers after allowlist matching without exposing real secrets to
  scripts.
- `SshAllowlist` / `SshClient` gate `ssh`, `scp`, and `sftp` destinations.
- `BashTool::execution` validates tool input and returns user-facing errors
  separately from internal diagnostics.

## Threat model

The attacker controls shell source, builtin arguments, mounted virtual files,
and sometimes embedded Python/TypeScript/SQLite programs. Highest impact bugs
are sandbox escape to host filesystem/process/network, secret or host path
disclosure, resource exhaustion, bypassing allowlists, request signing or
credential injection mistakes, and malformed snapshots restoring unsafe state.
Diagnostics from builtins are part of the attack surface because they are often
shown directly to LLMs or users.

## Project-specific patterns to flag

- Any builtin or embedded runtime path that touches `std::fs`, process APIs,
  host environment, sockets, or non-VFS paths instead of `FileSystem` /
  `HttpClient` / allowlist-controlled abstractions.
- `RealFs`, mount, overlay, copy-on-write, snapshot, or restore changes that
  skip canonicalization, `validate_path`, quota checks, whiteout accounting, or
  mount-prefix boundaries.
- HTTP/SSH/git remote operations that happen before allowlist validation, follow
  redirects without re-checking, leak URL credentials, or drop bot-auth /
  credential headers unexpectedly.
- Builtin stderr/error formatting that uses Rust Debug shapes or exposes host
  paths, registry paths, secrets, panic text, or long internal diagnostics.
- Resource limit counters, parser fuel/depth, timeout wrapping, output caps, and
  per-exec extension scopes that can be skipped by pipelines, background jobs,
  traps, command substitution, nested interpreters, or snapshots.

## Known false-positives

- Tests and fuzz targets intentionally contain hostile scripts, fake secrets,
  host canaries, traversal strings, malformed archives, and allowlist bypass
  attempts.
- `specs/` and `crates/bashkit/docs/` describe vulnerabilities and mitigations;
  treat them as design docs unless the same pattern appears in executable code.
- Optional features named `realfs`, `http_client`, `ssh`, `git`, `python`,
  `typescript`, `sqlite`, and `bot-auth` are intended capability gates, not
  automatically enabled escape hatches.
- Debug formatting is allowed in test assertions only when locally annotated
  with `debug-ok`; production builtin source under `crates/bashkit/src/builtins/`
  should not use `{:?}`.
- Generated package/build metadata, snapshots used as fixtures, and example
  API tokens are not findings unless a runtime path exposes real host data.
