# Pluggable HTTP Transport

> Host-injectable transport for all outbound HTTP made by the `curl`/`wget`/`http` builtins, so embedders can direct sandbox traffic through their own boundary (egress gateway, proxy, audit layer) while bashkit keeps enforcing HTTP policy.

## Problem

Embedding hosts (reference consumer: [everruns](https://github.com/everruns/everruns), `specs/egress.md` there) must route *all* outbound traffic through a host-owned egress boundary — for network policy, audit, signing, and airgapped deployments. bashkit's built-in reqwest connectivity dials the network directly and deliberately ignores host proxy env vars (TM-NET-015), so without an injection point an embedder cannot centralize sandbox HTTP. The legacy `HttpHandler` hook was too weak for this: loose `(method, url, body, headers)` args, no timeouts, no SSRF precheck result, stringly-typed errors that could not distinguish a host policy denial from a connect failure.

## Design Decisions

1. **bashkit owns policy, the transport owns connectivity.** Every policy step runs *before* `HttpTransport::execute`: URL allowlist check, DNS/private-IP SSRF precheck, `before_http` hooks (credential injection), bot-auth signing, and the response size cap (re-checked after the transport returns). A transport moves bytes; it cannot be used to bypass the sandbox boundary.
2. **Follows fetchkit.** Same shape as fetchkit's `HttpTransport`/`TransportRequest` (reference: `everruns/fetchkit`), so a host can back both libraries with one egress implementation. Differences: bashkit's response is buffered (curl/wget buffer bodies up to `max_response_bytes` anyway) and the request carries `connect_timeout` (curl `--connect-timeout`).
3. **Request struct, not loose args.** `HttpTransportRequest` is `#[non_exhaustive]`: method, url, merged headers, body, effective timeout, connect timeout, `pinned_addrs`, `max_response_bytes`. New context can be added without breaking transports. No `Debug` derive — headers can carry credentials (TM-LOG-001).
4. **Typed errors map to curl exit codes.** `HttpTransportError::{Denied, Timeout, TooLarge, Transport}` render with the message prefixes curl/wget already map to exit codes 7/28/63/1. A host policy denial at the egress boundary surfaces to the script exactly like a bashkit allowlist denial.
5. **Pinned addresses close the rebind window at the host boundary.** The SSRF precheck's resolve-then-check result is forwarded as `pinned_addrs` (the validated IP literal, or the resolved-and-filtered addresses; empty on the documented DNS fail-open path or when private-IP blocking is disabled). Host transports forward them (e.g. `EgressRequest.pinned_addrs`); self-dialing transports connect to them or re-resolve + re-filter (`is_private_ip`). Same TM-NET-023 responsibility split as `HttpHandler` had, now with the data to act on it.
6. **Signing is preserved.** Bot-auth signing headers are computed in `HttpClient` and merged into `HttpTransportRequest.headers` before dispatch — identical to the built-in reqwest path. Redirects are followed manually by curl/wget, so every hop is re-validated, re-signed, and re-dispatched through the transport (fetchkit parity: one transport call per hop).
7. **Limits are communicated, then enforced.** `max_response_bytes` and the effective timeout ride on the request so a well-behaved transport stops early with `TooLarge`/`Timeout`; bashkit still enforces both after the fact (`tokio::time::timeout` around the call, size re-check on the returned body), so a misbehaving transport cannot exceed them.
8. **Disabled by default, unchanged.** The transport does not widen network access: without the `http_client` feature *and* a `BashBuilder::network(allowlist)` call, HTTP builtins cannot make requests and the transport is never invoked.
9. **`HttpHandler` deprecated, not removed.** `set_handler`/`http_handler` wrap the handler in an internal `HandlerTransport` adapter, preserving behavior (errors surface verbatim as `Transport`). One extension point going forward.
10. **`Arc`, not `Box`.** Hosts that build one `Bash` per execution share a single transport across instances.

## Request Pipeline

```
1. Allowlist check              ← security gate
2. Private IP / SSRF check      ← SSRF protection, produces pinned_addrs
3. before_http hooks            ← credential injection lives here
4. Bot-auth signing             ← Ed25519 headers
5. Custom HttpTransport OR reqwest   ← connectivity only
6. Response size cap re-check   ← misbehaving-transport backstop
7. after_http hooks             ← observational
```

## API

`HttpTransport` (trait, `execute(HttpTransportRequest) -> Result<Response, HttpTransportError>`), `HttpTransportRequest`, `HttpTransportError`, re-exported `HttpMethod`/`HttpResponse`; injected via `BashBuilder::http_transport(Arc<dyn HttpTransport>)` or `HttpClient::set_transport`. See rustdoc on `bashkit::HttpTransport` for the full contract and an egress-shaped example.

## Testing

- Unit (`network/client.rs`, `network/transport.rs`): merged signing headers reach the transport, pinned addrs for IP literals, timeout/cap forwarding, deadline + size enforcement around misbehaving transports, deprecated-handler shim, error Display ↔ exit-code contract.
- Integration (`tests/integration/network_security_tests.rs`, `custom_transport` module): curl/wget end-to-end through a mock transport, allowlist still enforced ahead of the transport, `Denied`→7 / `Timeout`→28 / `TooLarge`→63 / `Transport`→1 exit codes, deprecated `http_handler` builder path.

## See also

- `specs/request-signing.md` — signing pipeline the transport inherits
- `specs/credential-injection.md` — header injection ahead of dispatch
- `specs/threat-model.md` — TM-NET-023 (SSRF responsibility of custom transports), TM-NET-015 (host proxy isolation on the built-in path)
- `specs/tool-contract.md` — LLM tool surface this feeds
