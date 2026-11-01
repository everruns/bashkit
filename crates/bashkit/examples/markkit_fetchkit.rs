//! `markkit` and `fetchkit` builtins: the standalone CLIs inside the sandbox.
//!
//! Run with:
//!
//! ```bash
//! cargo run --example markkit_fetchkit --features markkit,fetchkit
//! # also fetch a real page through the network allowlist:
//! LIVE_URL=https://example.com cargo run --example markkit_fetchkit --features markkit,fetchkit
//! ```
//!
//! The offline part serves a page from an in-process `HttpTransport`, so it is
//! deterministic in CI and still exercises the real request path (allowlist,
//! redirects, response limits). Each step asserts its result, so the example
//! fails loudly if the builtins regress.

use std::sync::Arc;

use async_trait::async_trait;
use bashkit::{
    Bash, HttpResponse, HttpTransport, HttpTransportError, HttpTransportRequest, NetworkAllowlist,
};

/// Stand-in for a web server: `/docs` redirects to `/docs/intro`, which serves HTML.
struct DocsSite;

#[async_trait]
impl HttpTransport for DocsSite {
    async fn execute(
        &self,
        request: HttpTransportRequest,
    ) -> Result<HttpResponse, HttpTransportError> {
        let html = |body: &str| HttpResponse {
            status: 200,
            headers: vec![("Content-Type".into(), "text/html; charset=utf-8".into())],
            body: body.as_bytes().to_vec(),
        };
        Ok(if request.url.ends_with("/docs") {
            HttpResponse {
                status: 302,
                headers: vec![("Location".into(), "/docs/intro".into())],
                body: vec![],
            }
        } else if request.url.ends_with("/docs/intro") {
            html(
                "<html><head><title>Intro</title></head><body>\
                 <nav>skip me</nav><main><h1>Getting started</h1>\
                 <p>Install with <code>cargo add bashkit</code>.</p></main></body></html>",
            )
        } else {
            HttpResponse {
                status: 404,
                headers: vec![("Content-Type".into(), "text/plain".into())],
                body: b"not found".to_vec(),
            }
        })
    }
}

fn step(title: &str, r: &bashkit::ExecResult) {
    println!("--- {title} (exit {})", r.exit_code);
    print!("{}", r.stdout);
    if !r.stderr.is_empty() {
        print!("stderr: {}", r.stderr);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    const SITE: &str = "http://93.184.216.34";
    let mut bash = Bash::builder()
        .network(NetworkAllowlist::new().allow(SITE))
        .http_transport(Arc::new(DocsSite))
        .build();

    // 1. A file in the VFS -> Markdown.
    let r = bash
        .exec(
            r#"mkdir -p /work && cd /work
               printf '<h1>Report</h1><ul><li>alpha</li><li>beta</li></ul>' > report.html
               markkit extract report.html --file"#,
        )
        .await?;
    step("markkit extract report.html --file", &r);
    anyhow::ensure!(r.exit_code == 0 && r.stdout.contains("# Report"));
    anyhow::ensure!(r.stdout.contains("- alpha"));

    // 2. Pipeline stdin, JSON output, piped into jq.
    let r = bash
        .exec(r#"echo '<p>Hello <b>world</b></p>' | markkit extract - --json | jq -r .markdown"#)
        .await?;
    step("markkit extract - --json | jq", &r);
    anyhow::ensure!(r.exit_code == 0 && r.stdout.contains("Hello **world**"));

    // 3. markkit on a URL: the redirect hop is re-checked against the allowlist.
    let r = bash
        .exec(&format!("markkit extract {SITE}/docs --main-content"))
        .await?;
    step("markkit extract URL --main-content", &r);
    anyhow::ensure!(r.exit_code == 0 && r.stdout.contains("# Getting started"));
    anyhow::ensure!(!r.stdout.contains("skip me"));

    // 4. fetchkit: Markdown with YAML frontmatter, then JSON status via jq.
    let r = bash.exec(&format!("fetchkit fetch {SITE}/docs")).await?;
    step("fetchkit fetch URL", &r);
    anyhow::ensure!(r.exit_code == 0 && r.stdout.starts_with("---"));
    anyhow::ensure!(r.stdout.contains("Getting started"));

    let r = bash
        .exec(&format!(
            "fetchkit fetch {SITE}/docs/intro -o json | jq .status_code"
        ))
        .await?;
    step("fetchkit fetch -o json | jq .status_code", &r);
    anyhow::ensure!(r.stdout.trim() == "200");

    // 5. Off the allowlist: refused before any request leaves the sandbox.
    let r = bash.exec("fetchkit fetch https://evil.example/").await?;
    step("fetchkit fetch (not allowlisted)", &r);
    anyhow::ensure!(r.exit_code != 0);

    // 6. Host-only flags parse but fail closed.
    let r = bash
        .exec(&format!("fetchkit fetch {SITE}/docs --render-rakers"))
        .await?;
    step("fetchkit --render-rakers", &r);
    anyhow::ensure!(r.exit_code == 1 && r.stderr.contains("is not available in bashkit"));

    // Optional: a real page over the network, e.g. LIVE_URL=https://example.com
    if let Ok(url) = std::env::var("LIVE_URL") {
        let origin = url::Url::parse(&url)?.origin().ascii_serialization();
        let mut live = Bash::builder()
            .network(NetworkAllowlist::new().allow(&origin))
            .build();
        let r = live.exec(&format!("fetchkit fetch {url}")).await?;
        step("LIVE fetchkit fetch", &r);
        anyhow::ensure!(r.exit_code == 0, "live fetchkit failed");
        let r = live.exec(&format!("markkit extract {url}")).await?;
        step("LIVE markkit extract", &r);
        anyhow::ensure!(r.exit_code == 0, "live markkit failed");
    }

    println!("ok");
    Ok(())
}
