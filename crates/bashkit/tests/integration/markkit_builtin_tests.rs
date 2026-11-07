//! `markkit` builtin: same CLI as the standalone binary, on the VFS and the
//! network allowlist. See knowledge/runtimes/markkit-fetchkit-builtins.md.

#![cfg(feature = "markkit")]

use bashkit::Bash;

const PAGE: &str = "<html><head><title>T</title></head><body><h1>Hello</h1><p><a href=\"/next\">next</a></p></body></html>";

async fn run(bash: &mut Bash, script: &str) -> bashkit::ExecResult {
    bash.exec(script).await.unwrap()
}

#[tokio::test]
async fn help_and_version_are_the_markkit_cli_output() {
    let mut bash = Bash::new();
    let help = run(&mut bash, "markkit --help").await;
    assert_eq!(help.exit_code, 0);
    assert_eq!(
        help.stdout,
        markkit_cli::command().render_help().to_string()
    );
    let version = run(&mut bash, "markkit --version").await;
    assert_eq!(version.exit_code, 0);
    assert_eq!(
        version.stdout,
        markkit_cli::command().render_version().to_string()
    );
    let sub = run(&mut bash, "markkit extract --help").await;
    assert_eq!(sub.exit_code, 0);
    assert!(sub.stdout.contains("--max-input-size"));
    assert!(
        sub.stdout.contains("--ocr"),
        "refused flags still documented"
    );
}

#[tokio::test]
async fn usage_error_exits_2() {
    let mut bash = Bash::new();
    let r = run(&mut bash, "markkit extract --no-such-flag x").await;
    assert_eq!(r.exit_code, 2);
    assert!(r.stderr.contains("--no-such-flag"), "{}", r.stderr);
    assert!(r.stdout.is_empty());
}

#[tokio::test]
async fn extracts_file_from_vfs_relative_to_cwd() {
    let mut bash = Bash::new();
    let script = format!(
        "mkdir -p /work && cd /work && printf '%s' '{PAGE}' > doc.html && markkit extract --file doc.html"
    );
    let r = run(&mut bash, &script).await;
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("# Hello"), "{}", r.stdout);
}

#[tokio::test]
async fn extracts_from_pipeline_stdin_and_json() {
    let mut bash = Bash::new();
    let r = run(
        &mut bash,
        &format!("printf '%s' '{PAGE}' | markkit extract - --json"),
    )
    .await;
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let json: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(json["source"], "-");
    assert!(json["markdown"].as_str().unwrap().contains("# Hello"));
}

#[tokio::test]
async fn rules_file_read_from_vfs() {
    let mut bash = Bash::new();
    let script = format!(
        "printf '%s' '[{{\"selector\": \"h1\", \"action\": \"skip\"}}]' > /r.json; printf '%s' '{PAGE}' | markkit extract - --rules /r.json"
    );
    let r = run(&mut bash, &script).await;
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(!r.stdout.contains("Hello"), "{}", r.stdout);
}

#[tokio::test]
async fn missing_vfs_file_is_one_line_error() {
    let mut bash = Bash::new();
    let r = run(&mut bash, "markkit extract --file /nope.html").await;
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.starts_with("Error: "), "{}", r.stderr);
    assert!(
        !r.stderr.contains("Os {") && !r.stderr.contains("kind:"),
        "{}",
        r.stderr
    );
}

#[tokio::test]
async fn host_files_are_not_reachable() {
    // The VFS answers, never the host filesystem.
    let mut bash = Bash::new();
    let r = run(&mut bash, "markkit extract --file /proc/self/environ").await;
    assert_eq!(r.exit_code, 1, "{}", r.stdout);
}

#[tokio::test]
async fn ocr_and_mcp_are_refused() {
    let mut bash = Bash::new();
    let r = run(&mut bash, "echo x | markkit extract - --ocr").await;
    assert_eq!(r.exit_code, 1);
    assert_eq!(r.stderr, "Error: --ocr is not available in bashkit\n");
    let r = run(&mut bash, "markkit mcp < /dev/null").await;
    assert_eq!(r.exit_code, 1);
    assert_eq!(
        r.stderr,
        "Error: mcp is not available in this environment\n"
    );
}

#[tokio::test]
async fn url_without_network_is_blocked() {
    let mut bash = Bash::new();
    let r = run(&mut bash, "markkit extract https://example.com/").await;
    assert_eq!(r.exit_code, 1);
    assert!(
        r.stderr.contains("network access not configured"),
        "{}",
        r.stderr
    );
}

#[cfg(feature = "http_client")]
mod network {
    use super::*;
    use async_trait::async_trait;
    use bashkit::{
        HttpResponse, HttpTransport, HttpTransportError, HttpTransportRequest, NetworkAllowlist,
    };
    use std::sync::{Arc, Mutex};

    /// `/start` redirects to `/final/page`; `/escape` redirects off the allowlist.
    #[derive(Clone, Default)]
    struct Site {
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl HttpTransport for Site {
        async fn execute(
            &self,
            request: HttpTransportRequest,
        ) -> std::result::Result<HttpResponse, HttpTransportError> {
            self.calls.lock().unwrap().push(request.url.clone());
            let redirect = |to: &str| HttpResponse {
                status: 302,
                headers: vec![("Location".into(), to.into())],
                body: vec![],
            };
            Ok(if request.url.ends_with("/start") {
                redirect("/final/page")
            } else if request.url.ends_with("/escape") {
                redirect("http://93.184.216.99/secret")
            } else if request.url.ends_with("/loop") {
                redirect("/loop")
            } else {
                HttpResponse {
                    status: 200,
                    headers: vec![("Content-Type".into(), "text/html".into())],
                    body: PAGE.as_bytes().to_vec(),
                }
            })
        }
    }

    fn bash(site: &Site) -> Bash {
        Bash::builder()
            .network(NetworkAllowlist::new().allow("http://93.184.216.34"))
            .http_transport(Arc::new(site.clone()))
            .build()
    }

    #[tokio::test]
    async fn url_follows_redirects_through_sandbox_client() {
        let site = Site::default();
        let mut bash = bash(&site);
        let r = run(&mut bash, "markkit extract http://93.184.216.34/start").await;
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert!(r.stdout.contains("# Hello"), "{}", r.stdout);
        // Relative links resolve against the final URL, like the binary.
        assert!(
            r.stdout.contains("http://93.184.216.34/next"),
            "{}",
            r.stdout
        );
        assert_eq!(
            *site.calls.lock().unwrap(),
            [
                "http://93.184.216.34/start",
                "http://93.184.216.34/final/page"
            ]
        );
    }

    #[tokio::test]
    async fn redirect_off_allowlist_is_blocked() {
        let site = Site::default();
        let mut bash = bash(&site);
        let r = run(&mut bash, "markkit extract http://93.184.216.34/escape").await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("URL blocked"), "{}", r.stderr);
        assert_eq!(
            site.calls.lock().unwrap().len(),
            1,
            "blocked hop never sent"
        );
    }

    #[tokio::test]
    async fn url_off_allowlist_is_blocked() {
        let site = Site::default();
        let mut bash = bash(&site);
        let r = run(&mut bash, "markkit extract http://93.184.216.99/").await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("URL blocked"), "{}", r.stderr);
        assert!(site.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn redirect_loop_is_capped() {
        let site = Site::default();
        let mut bash = bash(&site);
        let r = run(
            &mut bash,
            "markkit extract http://93.184.216.34/loop --max-redirects 2",
        )
        .await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("too many redirects"), "{}", r.stderr);
        assert_eq!(site.calls.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn flags_cannot_raise_redirect_cap() {
        let site = Site::default();
        let mut bash = bash(&site);
        let r = run(
            &mut bash,
            "markkit extract http://93.184.216.34/loop --max-redirects 100000",
        )
        .await;
        assert_eq!(r.exit_code, 1);
        // Default cap (5) + the first request.
        assert_eq!(site.calls.lock().unwrap().len(), 6);
    }
}

// THREAT[TM-DOS-143]: extraction runs synchronously where the exec timeout
// cannot cancel it, so markkit's own timeout is capped at the remaining
// exec budget. Without the cap this ran to markkit's 60 s default.
#[tokio::test]
async fn extraction_stops_at_exec_deadline() {
    use bashkit::ExecutionLimits;
    use std::time::{Duration, Instant};

    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().timeout(Duration::from_millis(300)))
        .build();
    let page = "<div><p>word <b>bold</b> <a href=\"/x\">link</a></p></div>".repeat(170_000);
    bash.fs()
        .write_file(std::path::Path::new("/big.html"), page.as_bytes())
        .await
        .unwrap();
    let started = Instant::now();
    let r = bash
        .exec("markkit extract --file /big.html > /dev/null")
        .await;
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(3),
        "markkit ran {elapsed:?} past a 300 ms exec timeout ({r:?})" // debug-ok: test diagnostics
    );
}
