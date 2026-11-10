# markkit and fetchkit

Bashkit can ship two document tools as builtins, with the **same command line
as their standalone binaries**:

- [`markkit`](https://github.com/everruns/markkit) converts HTML, PDF, DOCX,
  PPTX, XLSX/XLS/ODS, ODT, EPUB, email, notebooks and more to Markdown.
- [`fetchkit`](https://github.com/everruns/fetchkit) fetches a URL and returns
  LLM-ready Markdown with YAML frontmatter (or JSON), with site-aware fetchers
  and bounded crawling.

Both are **off by default**. Enable the Cargo features:

```toml
bashkit = { version = "0.21.0", features = ["markkit", "fetchkit"] }
```

```bash
markkit extract report.pdf --file
markkit extract https://example.com/post --main-content
cat page.html | markkit extract - --json

fetchkit fetch https://docs.rs/tokio --content-focus agent
fetchkit fetch https://example.com -o json | jq .status_code
```

`markkit --help` and `fetchkit --help` print exactly what the standalone tools
print: bashkit calls the same library code, so flags never drift.

## Sandbox behavior

- **Files** come from the virtual filesystem (paths relative to the shell's
  working directory). The host filesystem is never read.
- **Network** goes through the same default-deny allowlist as `curl`
  ([Networking](../../../docs/networking.md)). Every redirect hop is checked again.
  Credential injection, request signing and a custom `HttpTransport` apply.
  Without a `NetworkAllowlist`, URLs fail with `network access not configured`.
- **Limits**: markkit's `--max-*` and `--timeout` flags can lower its limits but
  not raise them. Response bodies are capped by bashkit's HTTP limits.
- **Environment**: fetchkit reads variables from the shell, never from the
  host process.

```rust,no_run
use bashkit::{Bash, NetworkAllowlist};

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder()
    .network(NetworkAllowlist::new().allow("https://docs.rs"))
    .build();
let out = bash.exec("fetchkit fetch https://docs.rs/tokio").await?;
assert_eq!(out.exit_code, 0);
# Ok(())
# }
```

A runnable, self-checking example lives in
[`examples/markkit_fetchkit.rs`](../examples/markkit_fetchkit.rs). It serves pages from an
in-process `HttpTransport`, so it needs no network. Set `LIVE_URL` to also
fetch a real page:

```bash
cargo run --example markkit_fetchkit --features markkit,fetchkit
LIVE_URL=https://example.com cargo run --example markkit_fetchkit --features markkit,fetchkit
```

Specialized fetchers follow the same rule. For example, fetchkit's arXiv
fetcher calls `export.arxiv.org`, so allow that origin too or the fetch fails
with `URL not in allowlist`.

## Not available in the sandbox

These flags are accepted by the parser and listed in `--help`, then fail with
exit code 1 and `Error: <flag> is not available in bashkit`:

| Command | Flag | Reason |
|---|---|---|
| both | `mcp` | long-running stdio server |
| markkit | `--ocr` | needs a host Tesseract binary or model files |
| fetchkit | `--allow-env-proxy` | host proxy settings |
| fetchkit | `--render-rakers` | embedded JavaScript renderer |
| fetchkit | `--bot-auth-key`, `--bot-auth-agent` | bashkit already signs requests ([Request signing](../../../docs/request-signing.md)) |
| fetchkit | `--allow-remote-conversion` | uploads documents to a third party |

Both builtins are native only; the browser (wasm) package does not include them.

## See also

- [Networking & HTTP](../../../docs/networking.md)
- [Credential injection](credential-injection.md)
- [Security](../../../docs/security.md)
