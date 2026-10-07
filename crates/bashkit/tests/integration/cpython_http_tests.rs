// HTTP from the CPython (WASI) python3 builtin through bashkit's egress
// pipeline (`cpython` + `http_client` features).
//
// The guest has no sockets: `http.client` / `urllib.request` hand each
// request to the host (`bashkit.http_request`), which runs it through
// `HttpClient` like `curl`. A recording transport stands in for the network.
// Threat IDs refer to knowledge/security/threat-model.md (TM-PY-CPY-003).

#![cfg(all(feature = "cpython", feature = "http_client"))]

use async_trait::async_trait;
use bashkit::testing::assert_no_leak;
use bashkit::{
    Bash, CPythonLimits, Credential, HttpResponse, HttpTransport, HttpTransportError,
    HttpTransportRequest, NetworkAllowlist,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const HOST: &str = "93.184.216.34";

type Log = Arc<Mutex<Vec<HttpTransportRequest>>>;

/// Answers by path; records every request it receives.
#[derive(Clone)]
struct FakeServer {
    log: Log,
}

#[async_trait]
impl HttpTransport for FakeServer {
    async fn execute(
        &self,
        request: HttpTransportRequest,
    ) -> Result<HttpResponse, HttpTransportError> {
        self.log.lock().unwrap().push(request.clone());
        let path = url::Url::parse(&request.url).unwrap().path().to_string();
        let reply = |status: u16, headers: &[(&str, &str)], body: &[u8]| HttpResponse {
            status,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: body.to_vec(),
        };
        Ok(match path.as_str() {
            "/json" => reply(
                200,
                &[
                    ("Content-Type", "application/json"),
                    ("X-Multi", "a"),
                    ("X-Multi", "b"),
                ],
                br#"{"ok": true}"#,
            ),
            "/echo" => {
                let mut body = format!("{} ", request.method.as_str()).into_bytes();
                body.extend_from_slice(request.body.as_deref().unwrap_or_default());
                reply(200, &[("Transfer-Encoding", "chunked")], &body)
            }
            "/missing" => reply(404, &[("Content-Type", "text/plain")], b"nope"),
            "/redirect" => reply(302, &[("Location", "/json")], b""),
            "/redirect-out" => reply(302, &[("Location", "http://203.0.113.9/x")], b""),
            "/slow" => {
                tokio::time::sleep(Duration::from_secs(10)).await;
                reply(200, &[], b"late")
            }
            _ => reply(200, &[("Content-Length", "5")], b"hello"),
        })
    }
}

fn server(allowlist: NetworkAllowlist) -> (bashkit::BashBuilder, Log) {
    let log = Log::default();
    let builder = Bash::builder()
        .cpython()
        .network(allowlist)
        .http_transport(Arc::new(FakeServer { log: log.clone() }));
    (builder, log)
}

fn allow_host() -> NetworkAllowlist {
    NetworkAllowlist::new()
        .allow(format!("http://{HOST}"))
        .allow(format!("https://{HOST}"))
}

async fn run(bash: &mut Bash, script: &str) -> bashkit::ExecResult {
    let r = bash.exec(script).await.unwrap();
    // Error text from the bridge reaches scripts (and often stdout): neither
    // stream may carry internal shapes or the injected secret.
    assert_no_leak(&r, "cpython-http stderr", &["real-secret"]);
    let mut out = r.clone();
    out.stderr = r.stdout.clone();
    assert_no_leak(&out, "cpython-http stdout", &["real-secret"]);
    r
}

fn header<'a>(req: &'a HttpTransportRequest, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[tokio::test]
async fn urllib_https_get() {
    let (builder, log) = server(allow_host());
    let mut bash = builder.build();
    let r = run(
        &mut bash,
        &format!(
            "python3 -c '
import json, urllib.request
with urllib.request.urlopen(\"https://{HOST}/json\") as resp:
    print(resp.status, resp.headers[\"Content-Type\"], resp.headers.get_all(\"X-Multi\"))
    print(json.load(resp))
'"
        ),
    )
    .await;
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "200 application/json ['a', 'b']\n{'ok': True}\n");
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].method.as_str(), "GET");
    assert_eq!(log[0].url, format!("https://{HOST}/json"));
    assert!(
        header(&log[0], "User-Agent")
            .unwrap()
            .starts_with("Python-urllib/3.14")
    );
    // Host-owned headers never come from the guest.
    assert!(header(&log[0], "Host").is_none());
    assert!(header(&log[0], "Connection").is_none());
}

#[tokio::test]
async fn urllib_post_body_and_http_error() {
    let (builder, log) = server(allow_host());
    let mut bash = builder.build();
    let r = run(
        &mut bash,
        &format!(
            "python3 -c '
import urllib.request, urllib.error
req = urllib.request.Request(\"http://{HOST}/echo\", data=b\"a=1&b=2\",
                             headers={{\"Content-Type\": \"application/x-www-form-urlencoded\"}})
print(urllib.request.urlopen(req).read().decode())
try:
    urllib.request.urlopen(\"http://{HOST}/missing\")
except urllib.error.HTTPError as e:
    print(e.code, e.read().decode())
'"
        ),
    )
    .await;
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "POST a=1&b=2\n404 nope\n");
    let log = log.lock().unwrap();
    assert_eq!(log[0].body.as_deref(), Some(b"a=1&b=2".as_slice()));
    assert_eq!(
        header(&log[0], "Content-Type"),
        Some("application/x-www-form-urlencoded")
    );
}

#[tokio::test]
async fn http_client_keep_alive_head_and_chunked_upload() {
    let (builder, log) = server(allow_host());
    let mut bash = builder.build();
    let r = run(
        &mut bash,
        &format!(
            "python3 -c '
import http.client
c = http.client.HTTPSConnection(\"{HOST}\", 443, timeout=5)
c.request(\"GET\", \"/plain?q=1\")
r = c.getresponse(); print(r.status, r.reason, r.read())
c.request(\"HEAD\", \"/plain\")
r = c.getresponse(); print(r.status, r.getheader(\"Content-Length\"), r.read())
c.request(\"PUT\", \"/echo\", body=iter([b\"ab\", b\"cd\"]), encode_chunked=True)
r = c.getresponse(); print(r.status, r.read())
c.close()
'"
        ),
    )
    .await;
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "200 OK b'hello'\n200 5 b''\n200 b'PUT abcd'\n");
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].url, format!("https://{HOST}/plain?q=1"));
    assert_eq!(log[1].method.as_str(), "HEAD");
    assert_eq!(log[2].body.as_deref(), Some(b"abcd".as_slice()));
    assert!(header(&log[2], "Transfer-Encoding").is_none());
}

#[tokio::test]
async fn redirects_are_rechecked_per_hop() {
    let (builder, log) = server(allow_host());
    let mut bash = builder.build();
    let r = run(
        &mut bash,
        &format!(
            "python3 -c '
import urllib.request, urllib.error
print(urllib.request.urlopen(\"http://{HOST}/redirect\").read())
try:
    urllib.request.urlopen(\"http://{HOST}/redirect-out\")
except urllib.error.URLError as e:
    print(\"denied:\", e.reason)
'"
        ),
    )
    .await;
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(
        r.stdout.starts_with("b'{\"ok\": true}'\ndenied: "),
        "{}",
        r.stdout
    );
    assert!(r.stdout.contains("access denied"), "{}", r.stdout);
    let urls: Vec<String> = log.lock().unwrap().iter().map(|r| r.url.clone()).collect();
    // The off-allowlist hop never reached the transport.
    assert_eq!(
        urls,
        vec![
            format!("http://{HOST}/redirect"),
            format!("http://{HOST}/json"),
            format!("http://{HOST}/redirect-out"),
        ]
    );
}

#[tokio::test]
async fn denied_and_private_hosts_never_reach_transport() {
    let (builder, log) = server(NetworkAllowlist::allow_all());
    let mut bash = builder.build();
    let r = run(
        &mut bash,
        "python3 -c '
import urllib.request, urllib.error
for url in (\"http://127.0.0.1/\", \"http://10.0.0.1/\", \"http://[::1]:8080/\", \"http://169.254.169.254/latest\"):
    try:
        urllib.request.urlopen(url)
        print(\"ALLOWED\", url)
    except urllib.error.URLError as e:
        print(\"denied\", \"access denied\" in str(e.reason))
'",
    )
    .await;
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "denied True\n".repeat(4));
    assert!(log.lock().unwrap().is_empty());

    let (builder, log) = server(allow_host());
    let mut bash = builder.build();
    let r = run(
        &mut bash,
        "python3 -c '
import http.client
c = http.client.HTTPConnection(\"203.0.113.9\")
try:
    c.request(\"GET\", \"/\")
except ConnectionError as e:
    print(type(e).__name__, e)
'",
    )
    .await;
    assert!(
        r.stdout.starts_with("ConnectionError access denied"),
        "{}",
        r.stdout
    );
    assert!(log.lock().unwrap().is_empty());
}

#[tokio::test]
async fn host_header_cannot_retarget_request() {
    // THREAT[TM-PY-CPY-003]: the allowlist checks the URL; a guest Host
    // header must not steer the request to another virtual host.
    let (builder, log) = server(allow_host());
    let mut bash = builder.build();
    let r = run(
        &mut bash,
        &format!(
            "python3 -c '
import http.client
c = http.client.HTTPConnection(\"{HOST}\")
c.putrequest(\"GET\", \"/plain\", skip_host=True)
c.putheader(\"Host\", \"internal.corp\")
c.putheader(\"X-Ok\", \"1\")
c.endheaders()
print(c.getresponse().status)
'"
        ),
    )
    .await;
    assert_eq!(r.stdout, "200\n", "{}", r.stderr);
    let log = log.lock().unwrap();
    assert_eq!(log[0].url, format!("http://{HOST}/plain"));
    assert!(header(&log[0], "Host").is_none());
    assert_eq!(header(&log[0], "X-Ok"), Some("1"));
}

#[tokio::test]
async fn header_injection_is_rejected() {
    let (builder, log) = server(allow_host());
    let mut bash = builder.build();
    let r = run(
        &mut bash,
        &format!(
            "python3 -c '
import _bashkit
for headers in ([(\"X\", \"a\\r\\nEvil: 1\")], [(\"Bad Name\", \"v\")]):
    try:
        _bashkit.http(\"GET\", \"http://{HOST}/\", headers, None, None)
        print(\"SENT\")
    except ValueError as e:
        print(\"rejected:\", e)
for method in (\"CONNECT\", \"TRACE\"):
    try:
        _bashkit.http(method, \"http://{HOST}/\", [], None, None)
        print(\"SENT\")
    except ValueError as e:
        print(\"rejected:\", e)
'"
        ),
    )
    .await;
    assert_eq!(
        r.stdout,
        "rejected: invalid header value\nrejected: invalid header name\n\
         rejected: unsupported HTTP method: CONNECT\nrejected: unsupported HTTP method: TRACE\n",
        "{}",
        r.stderr
    );
    assert!(log.lock().unwrap().is_empty());
}

#[tokio::test]
async fn credentials_injected_on_host_only() {
    let (builder, log) = server(allow_host());
    let mut bash = builder
        .credential_placeholder(
            "API_TOKEN",
            &format!("https://{HOST}"),
            Credential::bearer("real-secret"),
        )
        .build();
    let r = run(
        &mut bash,
        &format!(
            "python3 -c '
import os, urllib.request
tok = os.environ[\"API_TOKEN\"]
print(tok.startswith(\"bk_placeholder_\"), \"real-secret\" in tok)
req = urllib.request.Request(\"https://{HOST}/plain\", headers={{\"Authorization\": \"Bearer \" + tok}})
print(urllib.request.urlopen(req).read())
'"
        ),
    )
    .await;
    assert_eq!(r.stdout, "True False\nb'hello'\n", "{}", r.stderr);
    let log = log.lock().unwrap();
    assert_eq!(header(&log[0], "Authorization"), Some("Bearer real-secret"));
}

#[tokio::test]
async fn request_cap_per_call() {
    let log = Log::default();
    let mut bash = Bash::builder()
        .cpython_with_limits(CPythonLimits::default().max_http_requests(2))
        .network(allow_host())
        .http_transport(Arc::new(FakeServer { log: log.clone() }))
        .build();
    let r = run(
        &mut bash,
        &format!(
            "python3 -c '
import urllib.request, urllib.error
for i in range(3):
    try:
        urllib.request.urlopen(\"http://{HOST}/plain\").read(); print(\"ok\")
    except urllib.error.URLError as e:
        print(e.reason)
'"
        ),
    )
    .await;
    assert_eq!(
        r.stdout, "ok\nok\ntoo many HTTP requests (limit 2 per call)\n",
        "{}",
        r.stderr
    );
    assert_eq!(log.lock().unwrap().len(), 2);
    // The cap is per call, not per session.
    let r = run(
        &mut bash,
        &format!("python3 -c 'import urllib.request; print(urllib.request.urlopen(\"http://{HOST}/plain\").status)'"),
    )
    .await;
    assert_eq!(r.stdout, "200\n", "{}", r.stderr);
}

#[tokio::test]
async fn request_timeout_raises() {
    let (builder, _log) = server(allow_host());
    let mut bash = builder.build();
    let start = std::time::Instant::now();
    let r = run(
        &mut bash,
        &format!(
            "python3 -c '
import urllib.request, urllib.error
try:
    urllib.request.urlopen(\"http://{HOST}/slow\", timeout=0.3)
except urllib.error.URLError as e:
    print(type(e.reason).__name__, e.reason)
'"
        ),
    )
    .await;
    assert_eq!(r.stdout, "TimeoutError timed out\n", "{}", r.stderr);
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn call_deadline_bounds_requests() {
    let log = Log::default();
    let mut bash = Bash::builder()
        .cpython_with_limits(CPythonLimits::default().max_duration(Duration::from_millis(800)))
        .network(allow_host())
        .http_transport(Arc::new(FakeServer { log: log.clone() }))
        .build();
    let start = std::time::Instant::now();
    let r = run(
        &mut bash,
        &format!(
            "python3 -c 'import urllib.request; urllib.request.urlopen(\"http://{HOST}/slow\")'"
        ),
    )
    .await;
    assert_ne!(r.exit_code, 0);
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn no_network_configured() {
    let mut bash = Bash::builder().cpython().build();
    let r = run(
        &mut bash,
        "python3 -c '
import urllib.request, urllib.error
try:
    urllib.request.urlopen(\"https://example.com/\")
except urllib.error.URLError as e:
    print(e.reason)
'",
    )
    .await;
    assert_eq!(r.stdout, "network access not configured\n", "{}", r.stderr);
}
