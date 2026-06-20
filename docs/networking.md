# Networking & HTTP

Bashkit's HTTP builtins — `curl`, `wget`, and `http` — are the only way a script
can reach the network, and they are **default-deny**. With no configuration, every
outbound request is blocked. You opt in host by host with a `NetworkAllowlist`.

There is no DNS rebinding window, no automatic redirect following across hosts,
and no access to private or cloud-metadata IP ranges. Networking is a sandbox
boundary, not a convenience.

<svg viewBox="0 0 720 132" role="img" aria-label="Outbound HTTP request pipeline: builtin, allowlist gate, request signing, send" xmlns="http://www.w3.org/2000/svg" style="max-width:100%;height:auto;margin:1rem 0;">
  <rect x="0.5" y="0.5" width="719" height="131" fill="#ffffff" stroke="#0a1636" stroke-opacity="0.12"/>
  <g font-family="ui-monospace,monospace" font-size="13" fill="#0a1636">
    <rect x="20" y="44" width="118" height="44" rx="4" fill="#f5f5f5" stroke="#0a1636" stroke-opacity="0.3"/>
    <text x="79" y="64" text-anchor="middle">curl / wget</text>
    <text x="79" y="80" text-anchor="middle" fill="#404040">http</text>

    <rect x="190" y="44" width="138" height="44" rx="4" fill="#fff" stroke="#d4a43a" stroke-width="1.5"/>
    <text x="259" y="64" text-anchor="middle">allowlist</text>
    <text x="259" y="80" text-anchor="middle" fill="#404040" font-size="11">+ private-IP block</text>

    <rect x="380" y="44" width="138" height="44" rx="4" fill="#fff" stroke="#0a1636" stroke-opacity="0.3"/>
    <text x="449" y="64" text-anchor="middle">sign (opt-in)</text>
    <text x="449" y="80" text-anchor="middle" fill="#404040" font-size="11">bot-auth</text>

    <rect x="570" y="44" width="128" height="44" rx="4" fill="#0a1636"/>
    <text x="634" y="70" text-anchor="middle" fill="#ffffff">send →</text>

    <g stroke="#0a1636" stroke-opacity="0.5" fill="none">
      <path d="M138 66 H190" marker-end="url(#ar)"/>
      <path d="M328 66 H380" marker-end="url(#ar)"/>
      <path d="M518 66 H570" marker-end="url(#ar)"/>
    </g>
    <text x="259" y="118" text-anchor="middle" fill="#404040" font-size="11">blocked → request fails</text>
  </g>
  <defs>
    <marker id="ar" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
      <path d="M0 0 L10 5 L0 10 z" fill="#0a1636" fill-opacity="0.5"/>
    </marker>
  </defs>
</svg>

## Allowing hosts

```rust,no_run
use bashkit::{Bash, NetworkAllowlist};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let allowlist = NetworkAllowlist::new()
    .allow("https://api.example.com")          // entire host
    .allow("https://cdn.example.com/assets/"); // path prefix

let mut bash = Bash::builder().network(allowlist).build();

bash.exec("curl https://api.example.com/v1/users").await?;
# Ok(())
# }
```

### Pattern matching

A request matches an allowlist entry when:

- **Scheme** matches exactly — `https` is not `http`.
- **Host** matches exactly — no wildcards, no implicit subdomains.
- **Port** matches — defaults applied (443 for https, 80 for http).
- **Path** is a prefix — the entry's path must be a prefix of the request path.

This is literal-string matching by design: there is no DNS resolution at check
time, which closes the DNS-spoofing and rebinding classes of attack
(TM-NET-001/002).

### Built-in SSRF protection

Even for an allowed host, requests that resolve to private or reserved IP ranges
are refused at connect time (`127.0.0.0/8`, `10.0.0.0/8`, `172.16.0.0/12`,
`192.168.0.0/16`, `169.254.0.0/16` including cloud metadata, CGNAT, and the IPv6
equivalents). This blocks SSRF via DNS rebinding even if an allowlisted hostname
later resolves to an internal address (TM-NET-002/004/008). Override only when
you fully control the environment:

```rust,no_run
# use bashkit::NetworkAllowlist;
let allowlist = NetworkAllowlist::new()
    .allow("http://localhost:8080")
    .block_private_ips(false); // dangerous — testing only
```

`NetworkAllowlist::allow_all()` disables host checks entirely. Use it only for
fully trusted scripts.

## CLI

The CLI keeps network access off unless you ask for it:

```bash
# Blocked by default
bashkit -c 'curl https://example.com'

# Unrestricted outbound (trusted scripts only)
bashkit --http-allow-all -c 'curl https://example.com'
```

Per-host allowlisting is a library-level concern (`NetworkAllowlist`); the CLI
exposes the coarse `--http-allow-all` switch for trusted use.

## Observing and rewriting requests

HTTP requests flow through the same [hooks](hooks.md) pipeline as the rest of the
interpreter, so a host can observe, rewrite, or cancel an outbound request before
it leaves — useful for logging, header injection, or policy enforcement.

## See also

- [Credential injection](credential-injection.md) — attach secrets to outbound
  requests without exposing them to the script.
- [Request signing](request-signing.md) — cryptographic bot identity for signed
  outbound requests.
- [Security](security.md) — the full sandbox boundary model.
