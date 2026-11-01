---
type: Subsystem Design
title: Markkit and Fetchkit Builtins
description: The markkit and fetchkit CLIs as bashkit builtins, with the identical interface, on the VFS and network allowlist.
tags:
  - bashkit
  - builtins
  - markkit
  - fetchkit
  - network
---

# `markkit` and `fetchkit` Builtins

## Status
Implemented, off by default (features `markkit`, `fetchkit`), native only.

## Decision

Bashkit ships [markkit](https://github.com/everruns/markkit) (document to
Markdown) and [fetchkit](https://github.com/everruns/fetchkit) (URL to LLM-ready
Markdown) as builtins with the **exact same CLI** as their standalone binaries.

```bash
markkit extract report.pdf --file
curl -s "$URL" | markkit extract - --main-content
fetchkit fetch https://docs.rs/tokio --content-focus agent
fetchkit fetch https://example.com -o json | jq .status_code
```

### Same CLI by sharing code, not copying flags

Both upstream CLIs are libraries: `markkit_cli::run(args, &impl CliHost)` and
`fetchkit_cli::run(args, &dyn CliHost)` return an exit code. The standalone
binaries are thin wrappers over the same functions. The builtins pass `argv`
through untouched, so flags, `--help`, `--version`, error text and exit codes
(0 ok, 1 runtime error, 2 usage error) cannot drift. Integration tests assert
`--help`/`--version` equal the libraries' clap output. Both upstream crates
build clap without color or suggestions, matching bashkit, because clap
features unify across the dependency graph.

The `CliHost` trait is the only boundary:

| Host method | markkit builtin | fetchkit builtin |
|---|---|---|
| stdout / stderr | buffered into `ExecResult` | same |
| stdin | pipeline stdin | n/a |
| file reads (`--file`, `--rules`) | VFS, resolved against `cwd` | n/a |
| env | n/a | shell env (`ctx.env`), never host env |
| URL egress | `HttpClient` per hop, redirects followed here | fetchkit `HttpTransport` over `HttpClient` |
| limits | `--max-*`/`--timeout` may lower, never raise, markkit defaults | bashkit `HttpLimits` cap every body |
| `mcp` subcommand | refused | refused |

### Network

Every request goes through the per-`Bash` `HttpClient`: allowlist, SSRF
precheck, credential injection, bot-auth signing, `before_http`/`after_http`
hooks and the host `HttpTransport`. Redirects are one `HttpClient` call per hop,
so every hop is allowlist-checked (TM-NET-014). No network configured: both
commands fail closed with `network access not configured`.

- markkit is built **without** `http_client`; the builtin fetches bytes and calls
  `extract_document` with `base_url` = final URL (same as the binary).
- fetchkit is built **without** `reqwest-tls`. fetchkit's own DNS policy is off
  (`block_private_ips(false)`): bashkit already resolves-then-checks, and
  fetchkit's resolver is blocking. fetchkit's agent-discovery probes and
  specialized-fetcher API calls are ordinary hops through the same client.

Neither adds a TLS stack, aws-lc-sys, or a second markkit
(`cargo tree -i aws-lc-sys` is empty with both features).

### Refused flags

These parse and show in `--help` (same interface), then fail with exit 1 and
`Error: <flag> is not available in bashkit` before any input is read or request
is sent. Hosts refuse unknown future host features too (fail closed).

| Flag | Why |
|---|---|
| markkit `--ocr` (+ `--ocr-*`) | runs a host Tesseract binary, reads host model files |
| fetchkit `--allow-env-proxy` | host proxy env (TM-NET-015) |
| fetchkit `--render-rakers` | embedded QuickJS engine; decided never in the sandbox |
| fetchkit `--bot-auth-key` / `--bot-auth-agent` | bashkit signs in `HttpClient`; a key in argv double-signs and lands in history |
| fetchkit `--allow-remote-conversion` | uploads documents to a third party |
| `mcp` (both) | long-running stdio server; decided never in the sandbox |

### Isolation and startup

Builtins are stateless; all policy comes from the `Bash` instance's context.
The only shared values are the upstream crates' immutable clap trees. No
startup cost: registration is a map insert, nothing is loaded until a call.

## Constraints

- Native only (`not(target_arch = "wasm32")`): both CLI crates need tokio's
  multi-thread runtime.
- Off by default everywhere (core, bashkit-cli, Python, Node) until a separate
  decision turns them on (user, 2026-11-01).
- Upstream versions: markkit >= 0.3, fetchkit >= 0.7 (the `CliHost` releases).
  fetchkit must depend on the same markkit minor as bashkit, or bashkit links
  two markkit copies.

## Tests

- `tests/integration/markkit_builtin_tests.rs`,
  `tests/integration/fetchkit_builtin_tests.rs`: help/version parity, usage
  errors, VFS + stdin, allowlist and redirect enforcement, refused flags, limit
  clamping, tenant isolation.
- `no_leak_*` in `builtins/markkit.rs` and `builtins/fetchkit.rs` (TM-INF-022).
- Upstream: `crates/*-cli/tests/host.rs` in each repo (binary vs `run()` byte
  parity, host routing, `Send` futures).

## See also

- [Threat Model](../security/threat-model.md) TM-NET-030, TM-DOS-139
- [Limitations](../operations/limitations.md) L-DOC-001
- [HTTP Transport](../security/http-transport.md)
