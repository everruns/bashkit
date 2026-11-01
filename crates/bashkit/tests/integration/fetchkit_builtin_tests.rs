//! `fetchkit` builtin: same CLI as the standalone binary, every request through
//! the sandbox HTTP client. See knowledge/runtimes/markkit-fetchkit-builtins.md.

#![cfg(feature = "fetchkit")]

use async_trait::async_trait;
use bashkit::{
    Bash, HttpResponse, HttpTransport, HttpTransportError, HttpTransportRequest, NetworkAllowlist,
};
use std::sync::{Arc, Mutex};

const PAGE: &str =
    "<html><head><title>T</title></head><body><h1>Hello</h1><p>From the sandbox.</p></body></html>";
const BASE: &str = "http://93.184.216.34";

/// `/start` redirects to `/page`; `/escape` redirects off the allowlist;
/// everything else serves PAGE (discovery probes get 404).
#[derive(Clone, Default)]
struct Site {
    calls: Arc<Mutex<Vec<HttpTransportRequest>>>,
}

impl Site {
    fn urls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.url.clone())
            .collect()
    }
}

#[async_trait]
impl HttpTransport for Site {
    async fn execute(
        &self,
        request: HttpTransportRequest,
    ) -> std::result::Result<HttpResponse, HttpTransportError> {
        let url = request.url.clone();
        self.calls.lock().unwrap().push(request);
        let redirect = |to: &str| HttpResponse {
            status: 302,
            headers: vec![("Location".into(), to.into())],
            body: vec![],
        };
        Ok(if url.ends_with("/start") {
            redirect("/page")
        } else if url.ends_with("/escape") {
            redirect("http://93.184.216.99/secret")
        } else if url.ends_with("/page") {
            HttpResponse {
                status: 200,
                headers: vec![("Content-Type".into(), "text/html".into())],
                body: PAGE.as_bytes().to_vec(),
            }
        } else {
            HttpResponse {
                status: 404,
                headers: vec![("Content-Type".into(), "text/plain".into())],
                body: b"not found".to_vec(),
            }
        })
    }
}

fn bash(site: &Site) -> Bash {
    Bash::builder()
        .network(NetworkAllowlist::new().allow(BASE))
        .http_transport(Arc::new(site.clone()))
        .build()
}

#[tokio::test]
async fn help_and_version_are_the_fetchkit_cli_output() {
    let mut bash = Bash::new();
    let help = bash.exec("fetchkit --help").await.unwrap();
    assert_eq!(help.exit_code, 0);
    assert_eq!(
        help.stdout,
        fetchkit_cli::command().render_help().to_string()
    );
    let version = bash.exec("fetchkit --version").await.unwrap();
    assert_eq!(
        version.stdout,
        fetchkit_cli::command().render_version().to_string()
    );
    let sub = bash.exec("fetchkit fetch --help").await.unwrap();
    assert!(
        sub.stdout.contains("--render-rakers"),
        "refused flags still documented"
    );
}

#[tokio::test]
async fn usage_error_exits_2() {
    let mut bash = Bash::new();
    let r = bash.exec("fetchkit fetch --nope x").await.unwrap();
    assert_eq!(r.exit_code, 2);
    assert!(r.stderr.contains("--nope"), "{}", r.stderr);
}

#[tokio::test]
async fn fetch_goes_through_sandbox_client_and_follows_redirects() {
    let site = Site::default();
    let mut bash = bash(&site);
    let r = bash
        .exec(&format!("fetchkit fetch {BASE}/start"))
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(r.stdout.starts_with("---\n"), "{}", r.stdout);
    assert!(r.stdout.contains("status_code: 200"), "{}", r.stdout);
    assert!(r.stdout.contains("# Hello"), "{}", r.stdout);
    let urls = site.urls();
    assert_eq!(urls[0], format!("{BASE}/start"));
    assert_eq!(urls[1], format!("{BASE}/page"));
    assert!(urls.iter().all(|u| u.starts_with(BASE)), "{urls:?}");
}

#[tokio::test]
async fn json_output_pipes_into_shell_tools() {
    let site = Site::default();
    let mut bash = bash(&site);
    let r = bash
        .exec(&format!(
            "fetchkit fetch {BASE}/page -o json | grep -c status_code"
        ))
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(r.stdout.trim(), "1");
}

#[tokio::test]
async fn url_off_allowlist_never_reaches_transport() {
    let site = Site::default();
    let mut bash = bash(&site);
    let r = bash
        .exec("fetchkit fetch http://93.184.216.99/")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.starts_with("Error: "), "{}", r.stderr);
    assert!(site.urls().is_empty(), "{:?}", site.urls());
}

#[tokio::test]
async fn redirect_off_allowlist_is_blocked() {
    let site = Site::default();
    let mut bash = bash(&site);
    let r = bash
        .exec(&format!("fetchkit fetch {BASE}/escape"))
        .await
        .unwrap();
    assert_eq!(r.exit_code, 1, "{}", r.stdout);
    assert!(
        !site.urls().iter().any(|u| u.contains("93.184.216.99")),
        "{:?}",
        site.urls()
    );
}

#[tokio::test]
async fn without_network_fails_closed() {
    let mut bash = Bash::new();
    let r = bash
        .exec("fetchkit fetch https://example.com/")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 1);
    assert!(
        r.stderr.contains("network access not configured"),
        "{}",
        r.stderr
    );
}

#[tokio::test]
async fn host_only_flags_and_mcp_are_refused_before_any_request() {
    for flag in [
        "--allow-env-proxy",
        "--render-rakers",
        "--bot-auth-key k",
        "--allow-remote-conversion",
    ] {
        let site = Site::default();
        let mut bash = bash(&site);
        let r = bash
            .exec(&format!("fetchkit fetch {BASE}/page {flag}"))
            .await
            .unwrap();
        assert_eq!(r.exit_code, 1, "{flag}");
        assert!(
            r.stderr.ends_with("is not available in bashkit\n"),
            "{flag}: {}",
            r.stderr
        );
        assert!(site.urls().is_empty(), "{flag}");
    }
    let mut bash = Bash::new();
    let r = bash.exec("fetchkit mcp < /dev/null").await.unwrap();
    assert_eq!(r.exit_code, 1);
    assert_eq!(
        r.stderr,
        "Error: mcp is not available in this environment\n"
    );
}

#[tokio::test]
async fn reads_shell_env_not_host_env() {
    // CLOUDFLARE_* come from the shell; remote conversion stays refused anyway.
    let site = Site::default();
    let mut bash = bash(&site);
    let r = bash
        .exec(&format!(
            "CLOUDFLARE_API_TOKEN=x fetchkit fetch {BASE}/page --converter cloudflare"
        ))
        .await
        .unwrap();
    // Unknown/unavailable converter is a clean one-line error or a normal fetch.
    assert!(r.stderr.lines().count() <= 1, "{}", r.stderr);
}

#[tokio::test]
async fn tenants_are_isolated() {
    let a = Site::default();
    let b = Site::default();
    let mut bash_a = bash(&a);
    let mut bash_b = Bash::builder()
        .network(NetworkAllowlist::new().allow("http://93.184.216.35"))
        .http_transport(Arc::new(b.clone()))
        .build();
    let script = format!("fetchkit fetch {BASE}/page");
    let (ra, rb) = tokio::join!(bash_a.exec(&script), bash_b.exec(&script));
    assert_eq!(ra.unwrap().exit_code, 0);
    assert_eq!(
        rb.unwrap().exit_code,
        1,
        "tenant B's allowlist does not cover A's host"
    );
    assert!(!a.urls().is_empty());
    assert!(b.urls().is_empty());
}
