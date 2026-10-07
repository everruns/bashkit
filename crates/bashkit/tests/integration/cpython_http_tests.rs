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
            "/see-other" => reply(303, &[("Location", "/echo")], b""),
            "/temp" => reply(307, &[("Location", "/echo")], b""),
            "/loop" => reply(302, &[("Location", "/loop")], b""),
            "/cross" => reply(302, &[("Location", "http://93.184.216.35/plain")], b""),
            "/gzip" => {
                use std::io::Write;
                let mut gz =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                gz.write_all(b"zipped").unwrap();
                reply(200, &[("Content-Encoding", "gzip")], &gz.finish().unwrap())
            }
            "/latin" => reply(200, &[("Content-Type", "text/plain")], b"caf\xe9"),
            "/lines" => reply(
                200,
                &[("Content-Type", "text/plain; charset=utf-8")],
                b"a\nb\nc",
            ),
            "/cookie" => reply(
                200,
                &[("Set-Cookie", "sid=abc; Path=/"), ("Set-Cookie", "t=1")],
                b"",
            ),
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
import urllib.request
try:
    urllib.request.urlopen(\"http://{HOST}/slow\", timeout=0.3)
except TimeoutError as e:
    # Like CPython: a read timeout is not wrapped in URLError.
    print(type(e).__name__, e)
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

/// Runs a Python script (HOST replaced) from a file, through `run`.
async fn py(bash: &mut Bash, script: &str) -> bashkit::ExecResult {
    let script = script.replace("HOST", HOST);
    run(
        bash,
        &format!("cat > /t.py <<'EOF'\n{script}\nEOF\npython3 /t.py"),
    )
    .await
}

/// bashkit's own `requests` (guest/requests/) over the bridge.
mod requests_lib {
    use super::*;

    #[tokio::test]
    async fn get_json_post_and_errors() {
        let (builder, log) = server(allow_host());
        let mut bash = builder.build();
        let r = py(
            &mut bash,
            r#"
import requests
r = requests.get("https://HOST/json", params={"q": "a b"}, timeout=5)
print(r.status_code, r.headers["content-type"], r.json())
print(requests.post("http://HOST/echo", json={"k": 1}).text)
r = requests.get("http://HOST/missing")
print(r.status_code, r.ok, r.reason, r.text)
try:
    r.raise_for_status()
except requests.HTTPError as e:
    print(e)
s = requests.Session()
print([s.get("http://HOST/plain").text for _ in range(2)])
r = requests.get("http://HOST/redirect")
print(r.json(), len(r.history), r.url)
try:
    requests.get("http://HOST/redirect-out")
except requests.ConnectionError as e:
    print("denied", "access denied" in str(e))
try:
    requests.get("HOST/x")
except requests.exceptions.MissingSchema:
    print("missing schema")
"#,
        )
        .await;
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(
            r.stdout,
            format!(
                "200 application/json {{'ok': True}}\n\
                 POST {{\"k\": 1}}\n\
                 404 False Not Found nope\n\
                 404 Client Error: Not Found for url: http://{HOST}/missing\n\
                 ['hello', 'hello']\n\
                 {{'ok': True}} 1 http://{HOST}/json\n\
                 denied True\n\
                 missing schema\n"
            )
        );
        let log = log.lock().unwrap();
        assert_eq!(log[0].url, format!("https://{HOST}/json?q=a+b"));
        assert!(
            header(&log[0], "User-Agent")
                .unwrap()
                .starts_with("python-requests/")
        );
        assert_eq!(header(&log[1], "Content-Type"), Some("application/json"));
        assert!(log.iter().all(|r| !r.url.contains("203.0.113.9")));
    }

    #[tokio::test]
    async fn form_multipart_put_auth_and_cookies() {
        let (builder, log) = server(allow_host());
        let mut bash = builder.build();
        let r = py(
            &mut bash,
            r#"
import requests
print(requests.post("http://HOST/echo", data={"a": "1 2"}).text)
r = requests.post("http://HOST/echo", files={"f": ("x.txt", b"DATA")})
print("DATA" in r.text, 'filename="x.txt"' in r.text)
print(requests.put("http://HOST/echo", data=b"raw", headers={"X-K": "v"}).text)
s = requests.Session()
s.get("http://HOST/cookie")
print(sorted(s.cookies.items()))
s.get("http://HOST/plain", auth=("u", "p"))
"#,
        )
        .await;
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(
            r.stdout,
            "POST a=1+2\nTrue True\nPUT raw\n[('sid', 'abc'), ('t', '1')]\n"
        );
        let log = log.lock().unwrap();
        assert_eq!(
            header(&log[0], "Content-Type"),
            Some("application/x-www-form-urlencoded")
        );
        assert!(
            header(&log[1], "Content-Type")
                .unwrap()
                .starts_with("multipart/form-data; boundary=")
        );
        assert_eq!(header(&log[2], "X-K"), Some("v"));
        assert_eq!(header(&log[4], "Cookie"), Some("sid=abc; t=1"));
        assert_eq!(header(&log[4], "Authorization"), Some("Basic dTpw"));
    }

    /// Cookies a server sets go back only to that host, and credentials
    /// are dropped when a redirect leaves the host.
    #[tokio::test]
    async fn cookies_and_auth_stay_on_their_host() {
        let other = "93.184.216.35";
        let allow = allow_host()
            .allow(format!("http://{other}"))
            .allow(format!("http://{HOST}"));
        let (builder, log) = server(allow);
        let mut bash = builder.build();
        let r = py(
            &mut bash,
            &r#"
import requests
s = requests.Session()
s.get("http://HOST/cookie")
s.get("http://OTHER/plain")
"#
            .replace("OTHER", other),
        )
        .await;
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        let log = log.lock().unwrap();
        assert_eq!(header(&log[1], "Cookie"), None);
    }

    /// 303 turns into GET without a body, 307 keeps method and body,
    /// loops end in TooManyRedirects; gzip, charset fallback, line
    /// iteration, response hooks and JSON errors.
    #[tokio::test]
    async fn redirect_rules_bodies_and_helpers() {
        let (builder, _log) = server(allow_host());
        let mut bash = builder.build();
        let r = py(
            &mut bash,
            r#"
import requests
print(repr(requests.post("http://HOST/see-other", data="x").text))
print(requests.post("http://HOST/temp", data="x").text)
try:
    requests.get("http://HOST/loop")
except requests.TooManyRedirects:
    print("too many")
print(requests.get("http://HOST/gzip").text)
r = requests.get("http://HOST/latin")
print(r.encoding, r.text)
r = requests.get("http://HOST/lines")
print(list(r.iter_lines(decode_unicode=True)), list(r.iter_content(2)))
requests.get("http://HOST/plain", hooks={"response": lambda r, *a, **k: print("hook", r.status_code)})
try:
    requests.get("http://HOST/plain").json()
except requests.JSONDecodeError as e:
    print("json error", isinstance(e, ValueError))
"#,
        )
        .await;
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(
            r.stdout,
            "'GET '\nPOST x\ntoo many\nzipped\nISO-8859-1 café\n\
             ['a', 'b', 'c'] [b'a\\n', b'b\\n', b'c']\nhook 200\njson error True\n"
        );
    }

    /// Credentials never follow a redirect to another host; bad headers
    /// and the per-call request cap surface as requests exceptions.
    #[tokio::test]
    async fn cross_host_auth_crlf_and_request_cap() {
        let log = Log::default();
        let mut bash = Bash::builder()
            .cpython_with_limits(CPythonLimits::default().max_http_requests(3))
            .network(allow_host().allow("http://93.184.216.35"))
            .http_transport(Arc::new(FakeServer { log: log.clone() }))
            .build();
        let r = py(
            &mut bash,
            r#"
import requests
print(requests.get("http://HOST/cross", auth=("u", "p")).status_code)
try:
    requests.get("http://HOST/plain", headers={"X": "a\r\nInjected: 1"})
except requests.exceptions.InvalidHeader as e:
    print("invalid header:", e)
requests.get("http://HOST/plain")
try:
    requests.get("http://HOST/plain")
except requests.ConnectionError as e:
    print(e)
"#,
        )
        .await;
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(
            r.stdout,
            "200\ninvalid header: invalid header value\n\
             too many HTTP requests (limit 3 per call)\n"
        );
        let log = log.lock().unwrap();
        assert_eq!(header(&log[0], "Authorization"), Some("Basic dTpw"));
        assert_eq!(log[1].url, "http://93.184.216.35/plain");
        assert_eq!(header(&log[1], "Authorization"), None);
        assert_eq!(log.len(), 3);
    }

    #[tokio::test]
    async fn timeout_and_no_network() {
        let (builder, _log) = server(allow_host());
        let mut bash = builder.build();
        let r = py(
            &mut bash,
            r#"
import requests
try:
    requests.get("http://HOST/slow", timeout=0.3)
except requests.ReadTimeout:
    print("timeout")
"#,
        )
        .await;
        assert_eq!(r.stdout, "timeout\n", "{}", r.stderr);

        let mut bash = Bash::builder().cpython().build();
        let r = py(
            &mut bash,
            r#"
import requests
try:
    requests.get("https://example.com/")
except requests.ConnectionError as e:
    print("network access not configured" in str(e))
"#,
        )
        .await;
        assert_eq!(r.stdout, "True\n", "{}", r.stderr);
    }

    /// The point of bashkit's own module: importing it pulls in none of the
    /// heavy stdlib HTTP stack, and it is already loaded in the snapshot.
    #[tokio::test]
    async fn import_is_light() {
        let mut bash = Bash::builder().cpython().build();
        let r = py(
            &mut bash,
            r#"
import sys
before = set(sys.modules)
import requests, httpx, httpx2
heavy = {"urllib3", "ssl", "http.cookiejar", "charset_normalizer"}
print(sorted(set(sys.modules) - before), sorted(heavy & set(sys.modules)))
"#,
        )
        .await;
        assert_eq!(r.stdout, "[] []\n", "{}", r.stderr);
    }
}

/// bashkit's own `httpx` (guest/httpx.py; `httpx2` is the same module).
mod httpx_lib {
    use super::*;

    #[tokio::test]
    async fn client_basics_and_redirects() {
        let (builder, log) = server(allow_host());
        let mut bash = builder.build();
        let r = py(
            &mut bash,
            r#"
import httpx, httpx2
print(httpx2 is httpx)
r = httpx.get("https://HOST/json", params={"q": "a b"})
print(r, r.json(), r.headers["content-type"], r.headers.get_list("x-multi"))
print(httpx.post("http://HOST/echo", json={"k": 1}).text)
print(httpx.post("http://HOST/echo", content=b"raw").text)
r = httpx.get("http://HOST/redirect")
print(r.status_code, r.next_request.url)
with httpx.Client(base_url="http://HOST", headers={"X-A": "1"}, auth=("u", "p"),
                  follow_redirects=True) as c:
    r = c.get("/redirect")
    print(r.url, len(r.history))
    c.get("/cookie")
    c.get("/plain")
    print(dict(c.cookies))
try:
    httpx.get("http://HOST/missing").raise_for_status()
except httpx.HTTPStatusError as e:
    print(e.response.status_code, str(e).splitlines()[0])
try:
    httpx.get("http://HOST/redirect-out", follow_redirects=True)
except httpx.ConnectError as e:
    print("denied", "access denied" in str(e))
try:
    httpx.get("ftp://HOST/")
except httpx.UnsupportedProtocol as e:
    print(e)
"#,
        )
        .await;
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(
            r.stdout,
            format!(
                "True\n\
                 <Response [200 OK]> {{'ok': True}} application/json ['a', 'b']\n\
                 POST {{\"k\":1}}\n\
                 POST raw\n\
                 302 http://{HOST}/json\n\
                 http://{HOST}/json 1\n\
                 {{'sid': 'abc', 't': '1'}}\n\
                 404 Client error '404 Not Found' for url 'http://{HOST}/missing'\n\
                 denied True\n\
                 Request URL has an unsupported protocol 'ftp://'.\n"
            )
        );
        let log = log.lock().unwrap();
        assert_eq!(log[0].url, format!("https://{HOST}/json?q=a+b"));
        assert!(
            header(&log[0], "User-Agent")
                .unwrap()
                .starts_with("python-httpx/")
        );
        let plain = log.iter().rfind(|r| r.url.ends_with("/plain")).unwrap();
        assert_eq!(header(plain, "Cookie"), Some("sid=abc; t=1"));
        assert_eq!(header(plain, "X-A"), Some("1"));
        assert_eq!(header(plain, "Authorization"), Some("Basic dTpw"));
        assert!(log.iter().all(|r| !r.url.contains("203.0.113.9")));
    }

    #[tokio::test]
    async fn redirects_gzip_hooks_stream_and_errors() {
        let (builder, log) = server(allow_host().allow("http://93.184.216.35"));
        let mut bash = builder.build();
        let r = py(
            &mut bash,
            r#"
import httpx
seen = []
with httpx.Client(follow_redirects=True, max_redirects=5,
                  event_hooks={"request": [lambda r: seen.append(r.method)],
                               "response": [lambda r: seen.append(r.status_code)]}) as c:
    print(repr(c.post("http://HOST/see-other", content=b"x").text))
    print(c.post("http://HOST/temp", content=b"x").text)
    try:
        c.get("http://HOST/loop")
    except httpx.TooManyRedirects:
        print("too many")
    print(c.get("http://HOST/cross", auth=("u", "p")).status_code)
    print(c.get("http://HOST/gzip").text)
    with c.stream("GET", "http://HOST/lines") as r:
        print(list(r.iter_lines()), list(r.iter_bytes(2)))
    try:
        c.get("http://HOST/plain", headers={"X": "a\r\nInjected: 1"})
    except httpx.LocalProtocolError as e:
        print("local:", e)
print(seen[:4])
"#,
        )
        .await;
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(
            r.stdout,
            "'GET '\nPOST x\ntoo many\n200\nzipped\n\
             ['a', 'b', 'c'] [b'a\\n', b'b\\n', b'c']\nlocal: invalid header value\n\
             ['POST', 303, 'GET', 200]\n"
        );
        let log = log.lock().unwrap();
        let cross = log.iter().position(|r| r.url.ends_with("/cross")).unwrap();
        assert_eq!(header(&log[cross], "Authorization"), Some("Basic dTpw"));
        assert_eq!(log[cross + 1].url, "http://93.184.216.35/plain");
        assert_eq!(header(&log[cross + 1], "Authorization"), None);
    }

    #[tokio::test]
    async fn async_client_and_timeout() {
        let (builder, _log) = server(allow_host());
        let mut bash = builder.build();
        let r = py(
            &mut bash,
            r#"
import asyncio, httpx
async def main():
    async with httpx.AsyncClient() as c:
        rs = await asyncio.gather(c.get("http://HOST/json"), c.post("http://HOST/echo", data={"a": "b"}))
        print([r.status_code for r in rs], rs[1].text)
        try:
            await c.get("http://HOST/slow", timeout=0.3)
        except httpx.ReadTimeout:
            print("timeout")
asyncio.run(main())
"#,
        )
        .await;
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(r.stdout, "[200, 200] POST a=b\ntimeout\n");
    }

    #[tokio::test]
    async fn mock_transport_needs_no_network() {
        let mut bash = Bash::builder().cpython().build();
        let r = py(
            &mut bash,
            r#"
import httpx
def handler(request):
    return httpx.Response(201, json={"path": request.url.path})
with httpx.Client(transport=httpx.MockTransport(handler)) as c:
    r = c.post("https://api.test/items")
    print(r.status_code, r.json())
"#,
        )
        .await;
        assert_eq!(r.stdout, "201 {'path': '/items'}\n", "{}", r.stderr);
    }
}
