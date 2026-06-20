# Request signing

Bot identity on the web has historically been an honor system. A `User-Agent`
string is trivially spoofable — nothing stops a scraper from claiming to be
Googlebot — and IP allowlists fall apart the moment your agent runs on ephemeral
cloud addresses. Servers are left inferring identity from traffic patterns
instead of verifying it.

Request signing replaces that guesswork with cryptography. Behind the `bot-auth`
feature, Bashkit transparently signs **every** outbound HTTP request with an
Ed25519 signature per [RFC 9421](https://www.rfc-editor.org/rfc/rfc9421)
(web-bot-auth profile). A server that trusts your agent's public key can verify,
not assume, who is calling — unlocking per-key rate limits, selective API access,
and real audit trails.

> Background: [Request Signing — Cryptographic Identity for AI Agents](https://medium.com/everruns/request-signing-cryptographic-identity-for-ai-agents-0e5fc1b52aa3).

<svg viewBox="0 0 720 188" role="img" aria-label="Request signing flow: agent signs request with private key, server verifies via public key directory" xmlns="http://www.w3.org/2000/svg" style="max-width:100%;height:auto;margin:1rem 0;">
  <rect x="0.5" y="0.5" width="719" height="187" fill="#ffffff" stroke="#0a1636" stroke-opacity="0.12"/>
  <g font-family="ui-monospace,monospace" font-size="13" fill="#0a1636">
    <rect x="24" y="68" width="150" height="52" rx="4" fill="#f5f5f5" stroke="#0a1636" stroke-opacity="0.3"/>
    <text x="99" y="90" text-anchor="middle">Bashkit agent</text>
    <text x="99" y="108" text-anchor="middle" fill="#404040" font-size="11">private seed (zeroized)</text>

    <rect x="546" y="68" width="150" height="52" rx="4" fill="#0a1636"/>
    <text x="621" y="90" text-anchor="middle" fill="#ffffff">target server</text>
    <text x="621" y="108" text-anchor="middle" fill="#d4a43a" font-size="11">verifies signature</text>

    <g stroke="#0a1636" stroke-opacity="0.6" fill="none">
      <path d="M174 86 H540" marker-end="url(#ar2)"/>
    </g>
    <text x="357" y="78" text-anchor="middle" fill="#0a1636" font-size="11">signed request + Signature-Agent: agent.example</text>

    <rect x="300" y="132" width="280" height="40" rx="4" fill="#fff" stroke="#d4a43a" stroke-width="1.5"/>
    <text x="440" y="150" text-anchor="middle" font-size="11">well-known key directory</text>
    <text x="440" y="164" text-anchor="middle" fill="#404040" font-size="11">JWK Thumbprint → public key</text>
    <g stroke="#0a1636" stroke-opacity="0.5" fill="none" stroke-dasharray="4 3">
      <path d="M560 132 C 560 120, 600 120, 621 120" marker-end="url(#ar2)"/>
    </g>
    <text x="357" y="34" text-anchor="middle" fill="#404040" font-size="11">Signature · Signature-Input · Signature-Agent</text>
  </g>
  <defs>
    <marker id="ar2" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="#0a1636" fill-opacity="0.6"/>
    </marker>
  </defs>
</svg>

## How it works

Signing happens inside `HttpClient`, at the same layer as the
[network allowlist](networking.md) check — so it covers `curl`, `wget`, `http`,
per-request timeouts, custom handlers, and each hop of a manually-followed
redirect. No script can bypass it, and there are no CLI flags or script changes
to make: it is transparent.

It is also **non-blocking**. If signing ever fails (clock skew, key issue), the
request is sent unsigned rather than dropped — tool availability is never
sacrificed for signing (TM-AVAIL-001).

## Configuration

```rust,no_run
use bashkit::{Bash, BotAuthConfig};

# fn main() -> bashkit::Result<()> {
let seed = std::env::var("AGENT_SIGNING_SEED").unwrap();
let config = BotAuthConfig::from_base64_seed(&seed)?
    .with_agent_fqdn("agent.example.com")  // emitted as Signature-Agent
    .with_validity_secs(300);              // signature lifetime (default 300)

let mut bash = Bash::builder().bot_auth(config).build();
# let _ = bash;
# Ok(())
# }
```

The signing key never leaves `BotAuthConfig`; only the public key is derivable.
The seed bytes are zeroized on drop (TM-CRY-001).

## Serving your public key

A verifying server needs your public key. Derive it (without ever exposing the
seed) and publish it at your well-known key directory endpoint:

```rust,no_run
use bashkit::derive_bot_auth_public_key;

# fn main() -> bashkit::Result<()> {
# let seed = [0u8; 32];
let public = derive_bot_auth_public_key(&seed)?;
println!("key id: {}", public.key_id); // JWK Thumbprint (RFC 7638)
// public.jwk is the JSON Web Key to serve from your directory
# Ok(())
# }
```

The `keyid` in the signature is the JWK Thumbprint, so a server can look up the
exact key that signed a request.

## What gets added

Per RFC 9421 with the `web-bot-auth` tag, signing covers `@method` and
`@target-uri` (plus `signature-agent` when an FQDN is set), with a fresh 32-byte
nonce and `created` / `expires` timestamps on every request:

| Header | Value |
|--------|-------|
| `Signature` | `sig=:<base64url signature>:` |
| `Signature-Input` | `sig=("@method" "@target-uri");created=…;expires=…;keyid="…";alg="ed25519";nonce="…";tag="web-bot-auth"` |
| `Signature-Agent` | your FQDN (only when set) |

The nonce defends against replay; the expiry window bounds signature validity.

## See also

- [Networking & HTTP](networking.md) — the allowlist that gates every request.
- [Credential injection](credential-injection.md) — attach bearer tokens without
  exposing them to scripts.
- Spec: [`specs/request-signing.md`](https://github.com/everruns/bashkit/blob/main/specs/request-signing.md).
- [RFC 9421](https://www.rfc-editor.org/rfc/rfc9421) ·
  [RFC 7638](https://www.rfc-editor.org/rfc/rfc7638) ·
  [web-bot-auth architecture](https://datatracker.ietf.org/doc/html/draft-meunier-web-bot-auth-architecture).
