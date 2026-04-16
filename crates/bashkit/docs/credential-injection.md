# Credential Injection

Bashkit supports transparent credential injection for outbound HTTP requests.
Secrets are injected at the transport layer — sandboxed scripts never see the
real credentials, preventing exfiltration.

**See also:**
- [Threat Model](./threat-model.md) - Security properties
- [Custom Builtins](./custom_builtins.md) - Extending the shell
- [Credential Injection Spec](https://github.com/everruns/bashkit/blob/main/specs/credential-injection.md) - Design decisions

## Two Modes

### Mode 1: Injection (recommended)

The script has **no knowledge** of credentials. It makes plain requests; bashkit
adds authentication headers automatically based on the URL.

```rust,ignore
use bashkit::{Bash, Credential, NetworkAllowlist};

let mut bash = Bash::builder()
    .network(NetworkAllowlist::new()
        .allow("https://api.github.com"))
    .credential("https://api.github.com",
        Credential::bearer("ghp_xxxx"))
    .build();

let result = bash.exec("curl -s https://api.github.com/repos/foo/bar").await?;
// Authorization: Bearer ghp_xxxx was added transparently.
// The script never referenced any token.
```

### Mode 2: Placeholder

The script sees an **opaque placeholder** string in an environment variable. It
uses the placeholder like a real API key. Bashkit replaces the placeholder with
the real credential in outbound HTTP headers.

```rust,ignore
use bashkit::{Bash, Credential, NetworkAllowlist};

let mut bash = Bash::builder()
    .network(NetworkAllowlist::new()
        .allow("https://api.openai.com"))
    .credential_placeholder("OPENAI_API_KEY",
        "https://api.openai.com",
        Credential::bearer("sk-real-key"))
    .build();

// Inside the sandbox, $OPENAI_API_KEY = "bk_placeholder_a8f3c9e1..."
let result = bash.exec(r#"
    curl -H "Authorization: Bearer $OPENAI_API_KEY" \
         https://api.openai.com/v1/chat/completions \
         -d '{"model":"gpt-4","messages":[{"role":"user","content":"hi"}]}'
"#).await?;
// The placeholder was replaced with "sk-real-key" in the Authorization header.
```

**When to use placeholder mode:**
- Agent-generated scripts that read env vars (e.g., `$OPENAI_API_KEY`)
- SDKs that require a non-empty API key to initialize
- Compatibility with existing code patterns

## Credential Types

```rust,no_run
use bashkit::Credential;

// Bearer token → Authorization: Bearer <token>
let cred = Credential::bearer("ghp_xxxx");

// Custom header
let cred = Credential::header("X-Api-Key", "secret123");

// Multiple headers
let cred = Credential::headers(vec![
    ("X-Api-Key".into(), "key".into()),
    ("X-Api-Secret".into(), "secret".into()),
]);
```

## URL Pattern Matching

Credential patterns use the same matching rules as [`NetworkAllowlist`]:

- **Scheme**: Must match exactly (`https` vs `http`)
- **Host**: Must match exactly (no wildcards)
- **Port**: Must match (defaults: 443 for HTTPS, 80 for HTTP)
- **Path**: Pattern path is treated as a prefix

```rust,no_run
use bashkit::{Bash, Credential, NetworkAllowlist};

let bash = Bash::builder()
    .network(NetworkAllowlist::new()
        .allow("https://api.example.com"))
    // Only inject for /v1/ paths
    .credential("https://api.example.com/v1/",
        Credential::bearer("v1_token"))
    // Different token for /v2/
    .credential("https://api.example.com/v2/",
        Credential::bearer("v2_token"))
    .build();
```

## Security Properties

### What the script cannot do

| Attack | Why it fails |
|--------|-------------|
| Read the real secret from env vars | Injection mode: no env var exists. Placeholder mode: env var contains random placeholder |
| Exfiltrate placeholder to `evil.com` | [`NetworkAllowlist`] blocks unapproved hosts. Placeholder only replaced for matching URL patterns |
| Set a fake `Authorization` header | Injected headers **overwrite** existing headers with the same name |
| Log the credential via `echo` | Script only has access to the placeholder string, not the real secret |

### Header overwrite

When injecting credentials, bashkit **removes** any existing headers with the
same name before adding the credential header. This prevents a script from
setting `Authorization: Basic evil` and having it forwarded alongside the
injected `Authorization: Bearer real`.

### Non-blocking failures

If credential injection fails (e.g., callback error), the request is sent
**without** credentials. This follows the same principle as bot-auth signing
(request-signing spec): tool availability is never sacrificed for authentication.

## Multiple Credentials

You can configure credentials for multiple hosts:

```rust,no_run
use bashkit::{Bash, Credential, NetworkAllowlist};

let bash = Bash::builder()
    .network(NetworkAllowlist::new()
        .allow("https://api.github.com")
        .allow("https://api.openai.com")
        .allow("https://registry.npmjs.org"))
    .credential("https://api.github.com",
        Credential::bearer("ghp_xxxx"))
    .credential("https://api.openai.com",
        Credential::bearer("sk-xxxx"))
    .credential("https://registry.npmjs.org",
        Credential::header("Authorization", "Bearer npm_xxxx"))
    .build();
```

## Mixing Modes

Injection and placeholder modes can be used together:

```rust,no_run
use bashkit::{Bash, Credential, NetworkAllowlist};

let bash = Bash::builder()
    .network(NetworkAllowlist::new()
        .allow("https://api.github.com")
        .allow("https://api.openai.com"))
    // GitHub: pure injection (script doesn't know about auth)
    .credential("https://api.github.com",
        Credential::bearer("ghp_xxxx"))
    // OpenAI: placeholder (script uses $OPENAI_API_KEY)
    .credential_placeholder("OPENAI_API_KEY",
        "https://api.openai.com",
        Credential::bearer("sk-xxxx"))
    .build();
```

## How It Works

Credential injection is built on the [`hooks`] system. At build time,
`BashBuilder` converts credential rules into a `before_http` interceptor hook.
The hook fires after the URL allowlist check but before the request is sent:

```text
1. Allowlist check         (security gate)
2. Private IP / SSRF check (SSRF protection)
3. before_http hooks       (credential injection happens here)
4. Bot-auth signing        (Ed25519 signatures, if configured)
5. Request sent
```

This means:
- Credentials are only injected for **allowed** URLs
- Credentials are never sent to **private IPs**
- Credentials compose with **bot-auth** signing
- Custom `before_http` hooks run alongside credential injection
