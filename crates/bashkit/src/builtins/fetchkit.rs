//! `fetchkit` builtin: the fetchkit CLI, unchanged, on the network allowlist.
//!
//! # Decisions
//!
//! - **Same CLI, same code.** Arguments go to `fetchkit_cli::run`, the function
//!   the standalone `fetchkit` binary calls, so flags, help, version, errors,
//!   output and exit codes are identical. Nothing here parses arguments. See
//!   `knowledge/runtimes/markkit-fetchkit-builtins.md`.
//! - **One egress path.** Every request fetchkit makes (the page, redirect
//!   hops, specialized-fetcher API calls, agent-discovery probes) goes through
//!   [`SandboxTransport`], a fetchkit `HttpTransport` over this `Bash`'s
//!   [`HttpClient`]: allowlist, SSRF precheck, credential injection, bot-auth
//!   signing, hooks and the host `HttpTransport` all apply. fetchkit is built
//!   without `reqwest-tls`, so it adds no TLS stack.
//! - **fetchkit DNS policy is off** (`block_private_ips(false)`): bashkit's
//!   client already resolves and checks before dispatch, and fetchkit's
//!   resolver is blocking `to_socket_addrs`.
//! - **Refused in the sandbox (fail closed):** `--allow-env-proxy` (host proxy
//!   env), `--render-rakers` (embedded JS engine), `--bot-auth-key`/`-agent`
//!   (bashkit signs in `HttpClient`; a key in argv would double-sign and land in
//!   history), `--allow-remote-conversion` (uploads documents to a third
//!   party), and the `mcp` subcommand. All still parse and show in `--help`.
//! - **Off the async worker (TM-DOS-143).** HTML-to-Markdown conversion is
//!   synchronous CPU work; the run happens on tokio's blocking pool so the
//!   `exec` timeout can cancel the caller and co-tenants keep the worker. Per
//!   request, the transport timeout is capped at the remaining `exec` budget.
//! - **Stateless.** No globals; every run builds its own fetchkit `Tool`.

use std::io;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fetchkit::{
    HttpTransport, ToolBuilder, TransportError, TransportMethod, TransportRequest,
    TransportResponse,
};
use fetchkit_cli::{CliHost, HostFeature};

use super::{Builtin, Context};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;
use crate::network::{HttpClient, Method};

/// The `fetchkit` command (feature `fetchkit`).
pub struct Fetchkit;

#[async_trait]
impl Builtin for Fetchkit {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let host = Host {
            stdout: Mutex::new(String::new()),
            stderr: Mutex::new(String::new()),
            env: ctx.env.clone(),
            http: ctx.http_client.cloned(),
            deadline: ctx
                .remaining_deadline()
                .map(|left| std::time::Instant::now() + left),
        };
        let args: Vec<String> = std::iter::once("fetchkit".to_string())
            .chain(ctx.args.iter().cloned())
            .collect();
        let host = Arc::new(host);
        let exit_code = run_off_worker(args, host.clone()).await;
        Ok(ExecResult {
            stdout: take(&host.stdout).into(),
            stderr: take(&host.stderr).into(),
            exit_code,
            ..Default::default()
        })
    }

    fn llm_hint(&self) -> Option<&'static str> {
        Some(
            "fetchkit: Fetch a URL as LLM-ready Markdown with YAML frontmatter \
             (`fetchkit fetch URL`, `-o json`, `--crawl`, `--content-focus agent`). Same CLI as \
             the standalone fetchkit. Needs network allowlist; no mcp, proxies, rakers or bot-auth flags.",
        )
    }
}

fn take(buf: &Mutex<String>) -> String {
    std::mem::take(&mut *buf.lock().unwrap_or_else(|p| p.into_inner()))
}

/// [`CliHost`] over one builtin invocation.
struct Host {
    stdout: Mutex<String>,
    stderr: Mutex<String>,
    env: std::collections::HashMap<String, String>,
    http: Option<Arc<HttpClient>>,
    /// End of the `exec` wall-clock budget, if limited.
    deadline: Option<std::time::Instant>,
}

/// Runs the CLI on tokio's blocking pool (TM-DOS-143). `Handle::block_on`
/// drives the transport there; on a current-thread runtime the caller, parked
/// awaiting the join handle, keeps driving IO and timers.
async fn run_off_worker(args: Vec<String>, host: Arc<Host>) -> i32 {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return fetchkit_cli::run(args, &*host).await;
    };
    let job = tokio::task::spawn_blocking(move || handle.block_on(fetchkit_cli::run(args, &*host)));
    job.await.unwrap_or(1)
}

#[async_trait]
impl CliHost for Host {
    fn write_stdout(&self, text: &str) -> io::Result<()> {
        self.stdout
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_str(text);
        Ok(())
    }

    fn write_stderr(&self, text: &str) {
        self.stderr
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_str(text);
    }

    /// Shell environment, never the host process environment.
    fn env_var(&self, name: &str) -> Option<String> {
        self.env.get(name).cloned()
    }

    fn configure_tool(&self, builder: ToolBuilder) -> ToolBuilder {
        builder
            .transport(Arc::new(SandboxTransport {
                http: self.http.clone(),
                deadline: self.deadline,
            }))
            .block_private_ips(false)
            .use_env_proxy(false)
    }

    fn refuse(&self, feature: HostFeature) -> Option<String> {
        let _ = feature;
        Some("is not available in bashkit".to_string())
    }
}

/// fetchkit `HttpTransport` over the sandbox [`HttpClient`]. One call per hop:
/// fetchkit follows redirects itself, so every hop is re-checked here.
struct SandboxTransport {
    http: Option<Arc<HttpClient>>,
    deadline: Option<std::time::Instant>,
}

#[async_trait]
impl HttpTransport for SandboxTransport {
    async fn execute(
        &self,
        req: TransportRequest,
    ) -> std::result::Result<TransportResponse, TransportError> {
        let Some(http) = &self.http else {
            return Err(TransportError::Request(
                "network access not configured (see BashBuilder::network)".to_string(),
            ));
        };
        let method = match req.method {
            TransportMethod::Get => Method::Get,
            TransportMethod::Head => Method::Head,
        };
        // bashkit clamps the deadline to its own bounds; round sub-second up.
        let left = self
            .deadline
            .map(|end| end.saturating_duration_since(std::time::Instant::now()));
        let timeout = match (req.timeout, left) {
            (Some(t), Some(left)) => Some(t.min(left)),
            (t, left) => t.or(left),
        };
        let timeout_secs = timeout.map(|t| t.as_secs().max(1));
        let response = http
            .request_with_timeout(method, req.url.as_str(), None, &req.headers, timeout_secs)
            .await
            .map_err(to_transport_error)?;
        let body = bytes::Bytes::from(response.body);
        Ok(TransportResponse {
            status: response.status,
            url: req.url,
            headers: response.headers,
            body: Box::pin(futures_util::stream::once(async move { Ok(body) })),
        })
    }
}

/// Keep bashkit's own one-line messages (no Debug shapes, TM-INF-022).
fn to_transport_error(err: Error) -> TransportError {
    match err {
        Error::Network(msg) => TransportError::Request(msg),
        other => TransportError::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    /// fetchkit internals that must never reach stderr (TM-INF-022).
    const FETCHKIT_BANNED: &[&str] = &[
        "RequestError(",
        "TransportError",
        "FetchError",
        "reqwest::",
        "Stack backtrace",
        "src/",
    ];

    macro_rules! fetchkit_no_leak {
        ($name:ident, $script:expr) => {
            #[tokio::test]
            async fn $name() {
                let r = crate::builtins::debug_leak_check::run($script).await;
                assert_ne!(r.exit_code, 0, "{}", stringify!($name));
                crate::builtins::debug_leak_check::assert_no_leak(
                    &r,
                    stringify!($name),
                    FETCHKIT_BANNED,
                );
            }
        };
    }

    fetchkit_no_leak!(no_leak_no_network, "fetchkit fetch https://example.com/");
    fetchkit_no_leak!(no_leak_bad_url, "fetchkit fetch 'http://['");
    fetchkit_no_leak!(no_leak_bad_scheme, "fetchkit fetch file:///etc/passwd");
    fetchkit_no_leak!(
        no_leak_refused_flag,
        "fetchkit fetch https://x.test --render-rakers"
    );
    fetchkit_no_leak!(
        no_leak_bad_converter,
        "fetchkit fetch https://x.test --converter nope"
    );
}
