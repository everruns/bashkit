//! `markkit` builtin: the markkit CLI, unchanged, on the VFS and network allowlist.
//!
//! # Decisions
//!
//! - **Same CLI, same code.** Arguments go to `markkit_cli::run`, the function
//!   the standalone `markkit` binary calls. Flags, help, version, error text
//!   and exit codes are therefore identical; nothing here parses arguments.
//!   See `knowledge/runtimes/markkit-fetchkit-builtins.md`.
//! - **Host boundary.** [`Host`] is the only bridge: stdout/stderr buffers,
//!   pipeline stdin, VFS reads (paths resolved against `cwd`), and URL fetches
//!   through the per-`Bash` [`HttpClient`](crate::network::HttpClient), so the
//!   allowlist, SSRF precheck, credential injection, bot-auth signing and the
//!   host `HttpTransport` all apply. markkit is built without `http_client`:
//!   it links no second HTTP stack.
//! - **Redirects** are followed here, one `HttpClient` call per hop, so every
//!   hop is allowlist-checked (TM-NET-014), capped by markkit's
//!   `max_redirects`. Relative links resolve against the final URL, like the
//!   standalone CLI.
//! - **Limits only go down.** `--max-*`/`--timeout` flags may lower markkit's
//!   default limits but never raise them (`Host::limits`), so one tenant's
//!   script cannot buy a larger CPU/memory budget from a shared process.
//! - **Deadline (TM-DOS-143).** Extraction is synchronous CPU work. Inline
//!   it held the tenant's async worker (starving co-tenants scheduled there)
//!   and the `exec` timeout could not interrupt it. The run now happens on
//!   tokio's blocking pool, so the caller's future stays cancellable and the
//!   worker stays free, and markkit's cooperative `timeout` is capped at the
//!   remaining `exec` deadline so abandoned work stops soon after.
//! - **Refused in the sandbox:** `--ocr` (runs a host Tesseract binary, reads
//!   host model files) and the `mcp` subcommand (long-running stdio server).
//!   Both still parse and show in `--help`; they fail with exit 1.
//! - **Stateless.** No globals; every run builds its own `Markkit`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use markkit::{Extraction, Limits};
use markkit_cli::{CliHost, HostFeature};

use super::{Builtin, Context, resolve_path};
use crate::error::{Error, Result};
use crate::fs::FileSystem;
use crate::interpreter::ExecResult;

/// The `markkit` command (feature `markkit`).
pub struct Markkit;

#[async_trait]
impl Builtin for Markkit {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let host = Host {
            stdout: Mutex::new(String::new()),
            stderr: Mutex::new(String::new()),
            stdin: ctx.stdin.map(|s| s.as_bytes().to_vec()).unwrap_or_default(),
            fs: ctx.fs.clone(),
            cwd: ctx.cwd.clone(),
            #[cfg(feature = "http_client")]
            http: ctx.http_client.cloned(),
            deadline: ctx
                .remaining_deadline()
                .map(|left| crate::time_compat::Instant::now() + left),
        };
        let args: Vec<String> = std::iter::once("markkit".to_string())
            .chain(ctx.args.iter().cloned())
            .collect();
        let host = Arc::new(host);
        let exit_code = run_off_worker(args, host.clone()).await;
        Ok(ExecResult {
            stdout: host.take_stdout().into(),
            stderr: host.take_stderr().into(),
            exit_code,
            ..Default::default()
        })
    }

    fn llm_hint(&self) -> Option<&'static str> {
        Some(
            "markkit: Convert HTML, PDF, DOCX, PPTX, XLSX, ODT, EPUB, email and more to Markdown \
             (`markkit extract FILE --file`, `markkit extract URL`, `... | markkit extract -`). \
             Same CLI as the standalone markkit. URLs need network allowlist; no --ocr, no mcp.",
        )
    }
}

/// Runs the CLI on tokio's blocking pool (TM-DOS-143). `Handle::block_on`
/// drives the host's async VFS/HTTP calls there; on a current-thread runtime
/// the caller, parked awaiting the join handle, keeps driving IO and timers.
async fn run_off_worker(args: Vec<String>, host: Arc<Host>) -> i32 {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return markkit_cli::run(args, &*host).await;
    };
    let job = tokio::task::spawn_blocking(move || handle.block_on(markkit_cli::run(args, &*host)));
    // A markkit panic that unwinds reports as a plain failure.
    job.await.unwrap_or(1)
}

/// [`CliHost`] over one builtin invocation.
struct Host {
    stdout: Mutex<String>,
    stderr: Mutex<String>,
    stdin: Vec<u8>,
    fs: Arc<dyn FileSystem>,
    cwd: PathBuf,
    #[cfg(feature = "http_client")]
    http: Option<Arc<crate::network::HttpClient>>,
    /// End of the `exec` wall-clock budget, if limited. An instant, not a
    /// duration, so time queued behind a busy blocking pool still counts.
    deadline: Option<crate::time_compat::Instant>,
}

impl Host {
    fn take_stdout(&self) -> String {
        std::mem::take(&mut *self.stdout.lock().unwrap_or_else(|p| p.into_inner()))
    }

    fn take_stderr(&self) -> String {
        std::mem::take(&mut *self.stderr.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

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

    #[allow(clippy::manual_async_fn)]
    fn read_stdin(&self) -> impl Future<Output = io::Result<Vec<u8>>> + Send {
        let stdin = self.stdin.clone();
        async move { Ok(stdin) }
    }

    #[allow(clippy::manual_async_fn)]
    fn read_file(&self, path: &Path) -> impl Future<Output = io::Result<Vec<u8>>> + Send {
        let resolved = resolve_path(&self.cwd, &path.to_string_lossy());
        let fs = self.fs.clone();
        async move { fs.read_file(&resolved).await.map_err(to_io_error) }
    }

    #[allow(clippy::manual_async_fn)]
    fn extract_url(
        &self,
        markkit: &markkit::Markkit,
        url: &str,
    ) -> impl Future<Output = markkit::Result<Extraction>> + Send {
        let config = markkit.config().clone();
        let url = url.to_string();
        #[cfg(feature = "http_client")]
        let http = self.http.clone();
        async move {
            #[cfg(feature = "http_client")]
            {
                let Some(http) = http else {
                    return Err(not_configured());
                };
                fetch_and_extract(&http, config, &url).await
            }
            #[cfg(not(feature = "http_client"))]
            {
                let _ = (config, url);
                Err(not_configured())
            }
        }
    }

    fn limits(&self, requested: Limits) -> Limits {
        let left = self.deadline.map(|end| {
            end.checked_duration_since(crate::time_compat::Instant::now())
                .unwrap_or_default()
        });
        clamp_limits(requested, &deadline_cap(left))
    }

    /// Fail closed: every host-only feature (today `--ocr`) is refused.
    fn refuse(&self, feature: HostFeature) -> Option<String> {
        let _ = feature;
        Some("is not available in bashkit".to_string())
    }
}

fn not_configured() -> markkit::Error {
    markkit::Error::UrlBlocked(
        "network access not configured (see BashBuilder::network)".to_string(),
    )
}

/// VFS errors as `io::Error`, keeping the kind (NotFound, PermissionDenied...).
fn to_io_error(err: Error) -> io::Error {
    match err {
        Error::Io(e) => e,
        other => io::Error::other(other.to_string()),
    }
}

/// markkit's defaults, with `timeout` capped at the remaining `exec` budget.
fn deadline_cap(deadline: Option<std::time::Duration>) -> Limits {
    let mut cap = Limits::default();
    if let Some(remaining) = deadline {
        cap.timeout = cap.timeout.min(remaining);
    }
    cap
}

/// Each limit is the smaller of what the flags asked for and the cap.
fn clamp_limits(requested: Limits, cap: &Limits) -> Limits {
    let mut l = requested;
    macro_rules! clamp {
        ($($f:ident),* $(,)?) => {$( l.$f = l.$f.min(cap.$f); )*};
    }
    clamp!(
        max_input_size,
        max_output_size,
        timeout,
        max_nesting_depth,
        max_nodes,
        max_archive_entries,
        max_archive_entry_size,
        max_archive_total_size,
        max_sheet_rows,
        max_sheet_cols,
        max_spreadsheet_cells,
        max_redirects,
        max_pdf_pages,
        max_pdf_stream_size,
        max_pdf_decoded_size,
    );
    l
}

/// GET `url` through the sandbox client, following redirects hop by hop,
/// then extract with `base_url` = the final URL.
#[cfg(feature = "http_client")]
async fn fetch_and_extract(
    http: &crate::network::HttpClient,
    mut config: markkit::MarkkitBuilder,
    url: &str,
) -> markkit::Result<Extraction> {
    use markkit::{LimitExceeded, LimitKind};

    let limits = config.limits.clone();
    let mut current = url::Url::parse(url)
        .map_err(|e| markkit::Error::UrlBlocked(format!("invalid URL: {e}")))?;
    let mut redirects = 0usize;
    let response = loop {
        if !matches!(current.scheme(), "http" | "https") {
            return Err(markkit::Error::UrlBlocked(format!(
                "unsupported scheme: {}",
                current.scheme()
            )));
        }
        let response = tokio::time::timeout(limits.timeout, http.get(current.as_str()))
            .await
            .map_err(|_| {
                markkit::Error::from(LimitExceeded::new(
                    LimitKind::Timeout,
                    format!(
                        "fetch exceeded timeout of {}s",
                        limits.timeout.as_secs_f64()
                    ),
                ))
            })?
            .map_err(|e| match e {
                Error::Network(msg) if msg.contains("access denied") => {
                    markkit::Error::UrlBlocked(msg)
                }
                other => markkit::Error::Http(other.to_string()),
            })?;
        let location = matches!(response.status, 301 | 302 | 303 | 307 | 308)
            .then(|| header(&response.headers, "location"))
            .flatten();
        let Some(location) = location else {
            break response;
        };
        if redirects >= limits.max_redirects {
            return Err(markkit::Error::Http(format!(
                "too many redirects (max {})",
                limits.max_redirects
            )));
        }
        redirects += 1;
        current = current
            .join(location)
            .map_err(|e| markkit::Error::Http(format!("invalid redirect location: {e}")))?;
    };

    if response.body.len() > limits.max_input_size {
        return Err(LimitExceeded::new(
            LimitKind::MaxInputSize,
            format!(
                "response body exceeds max_input_size {}",
                limits.max_input_size
            ),
        )
        .into());
    }
    let content_type = header(&response.headers, "content-type").map(str::to_string);
    config.base_url = Some(current.to_string());
    markkit::Markkit::new(config).extract_document_sync(
        &response.body,
        markkit::DocumentHint {
            content_type: content_type.as_deref(),
            file_name: None,
        },
    )
}

#[cfg(feature = "http_client")]
fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_never_raises_limits() {
        let cap = Limits::default();
        let mut huge = Limits::default();
        huge.max_input_size = usize::MAX;
        huge.timeout = std::time::Duration::from_secs(1_000_000);
        huge.max_redirects = 1_000;
        let got = clamp_limits(huge, &cap);
        assert_eq!(got, cap);
    }

    #[test]
    fn timeout_capped_at_remaining_deadline() {
        let remaining = std::time::Duration::from_millis(1500);
        let got = clamp_limits(Limits::default(), &deadline_cap(Some(remaining)));
        assert_eq!(got.timeout, remaining);
        // No deadline: markkit's default stands.
        let got = clamp_limits(Limits::default(), &deadline_cap(None));
        assert_eq!(got.timeout, Limits::default().timeout);
    }

    #[test]
    fn clamp_keeps_lowered_limits() {
        let cap = Limits::default();
        let mut low = Limits::default();
        low.max_input_size = 10;
        low.max_redirects = 0;
        let got = clamp_limits(low.clone(), &cap);
        assert_eq!(got, low);
    }

    /// markkit internals that must never reach stderr (TM-INF-022).
    const MARKKIT_BANNED: &[&str] = &[
        "LimitExceeded {",
        "UnsupportedFormat(",
        "HtmlParse(",
        "Io(",
        "Custom {",
        "Stack backtrace",
        "src/",
    ];

    macro_rules! markkit_no_leak {
        ($name:ident, $script:expr) => {
            #[tokio::test]
            async fn $name() {
                let r = crate::builtins::debug_leak_check::run($script).await;
                assert_ne!(r.exit_code, 0, "{}", stringify!($name));
                crate::builtins::debug_leak_check::assert_no_leak(
                    &r,
                    stringify!($name),
                    MARKKIT_BANNED,
                );
            }
        };
    }

    markkit_no_leak!(no_leak_missing_file, "markkit extract --file /missing.pdf");
    markkit_no_leak!(
        no_leak_input_limit,
        "echo '<p>hello world</p>' | markkit extract - --max-input-size 3"
    );
    markkit_no_leak!(
        no_leak_bad_rules_json,
        "echo '{' > /r.json; echo '<p>x</p>' | markkit extract - --rules /r.json"
    );
    markkit_no_leak!(
        no_leak_garbage_pdf,
        "printf '%%PDF-1.7 garbage' > /x.pdf; markkit extract --file /x.pdf"
    );
    markkit_no_leak!(no_leak_bad_url, "markkit extract 'http://['");
    markkit_no_leak!(no_leak_no_source, "markkit extract");
}
