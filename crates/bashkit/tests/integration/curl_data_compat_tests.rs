//! curl data-option compatibility and request-body limit coverage.

#![cfg(feature = "http_client")]

use async_trait::async_trait;
use bashkit::{
    Bash, FileSystem, HttpResponse, HttpTransport, HttpTransportError, HttpTransportRequest,
    InMemoryFs, NetworkAllowlist,
};
use std::sync::{Arc, Mutex};

const URL: &str = "http://93.184.216.34/echo";
const BODY_LIMIT: usize = 10_000_000;

#[derive(Clone)]
struct CaptureTransport {
    requests: Arc<Mutex<Vec<HttpTransportRequest>>>,
}

#[async_trait]
impl HttpTransport for CaptureTransport {
    async fn execute(
        &self,
        request: HttpTransportRequest,
    ) -> std::result::Result<HttpResponse, HttpTransportError> {
        self.requests.lock().unwrap().push(request);
        Ok(HttpResponse {
            status: 200,
            headers: vec![],
            body: b"ok".to_vec(),
        })
    }
}

fn bash_with_capture(
    fs: Option<Arc<dyn FileSystem>>,
) -> (Bash, Arc<Mutex<Vec<HttpTransportRequest>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let transport = CaptureTransport {
        requests: requests.clone(),
    };
    let mut builder = Bash::builder()
        .network(NetworkAllowlist::allow_all())
        .http_transport(Arc::new(transport));
    if let Some(fs) = fs {
        builder = builder.fs(fs);
    }
    (builder.build(), requests)
}

#[tokio::test]
async fn repeated_mixed_data_options_preserve_order_and_default_content_type() {
    let (mut bash, requests) = bash_with_capture(None);
    let result = bash
        .exec(&format!(
            "curl -s -d a=1 --data-raw @literal --data-binary b=2 --data-urlencode 'q=a b*' {URL}"
        ))
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    let requests = requests.lock().unwrap();
    let request = requests.last().unwrap();
    assert_eq!(request.method.as_str(), "POST");
    assert_eq!(
        request.body.as_deref(),
        Some(b"a=1&@literal&b=2&q=a+b%2A".as_slice())
    );
    assert!(request.headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("content-type") && value == "application/x-www-form-urlencoded"
    }));
}

#[tokio::test]
async fn get_aggregates_data_into_existing_query_without_a_body() {
    let (mut bash, requests) = bash_with_capture(None);
    let result = bash
        .exec(&format!(
            "curl -s -G '{URL}?fixed=1#frag' -d a=1 --data-urlencode 'q=a b*'"
        ))
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    let requests = requests.lock().unwrap();
    let request = requests.last().unwrap();
    assert_eq!(request.method.as_str(), "GET");
    assert_eq!(request.url, format!("{URL}?fixed=1&a=1&q=a+b%2a#frag"));
    assert_eq!(request.body, None);
}

#[tokio::test]
async fn file_variants_apply_their_own_at_file_rules_in_order() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(std::path::Path::new("/payload"), b"a=1\r\nb=2\n")
        .await
        .unwrap();
    let (mut bash, requests) = bash_with_capture(Some(fs));
    let result = bash
        .exec(&format!(
            "curl -s -d @/payload --data-binary @/payload --data-urlencode p@/payload {URL}"
        ))
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    let requests = requests.lock().unwrap();
    assert_eq!(
        requests.last().unwrap().body.as_deref(),
        Some(b"a=1b=2&a=1\r\nb=2\n&p=a%3D1%0D%0Ab%3D2%0A".as_slice())
    );
}

#[tokio::test]
async fn aggregate_body_limit_counts_join_separators() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(std::path::Path::new("/left"), &vec![b'a'; BODY_LIMIT / 2])
        .await
        .unwrap();
    fs.write_file(std::path::Path::new("/right"), &vec![b'b'; BODY_LIMIT / 2])
        .await
        .unwrap();
    let (mut bash, requests) = bash_with_capture(Some(fs));
    let result = bash
        .exec(&format!("curl -s --data-binary @/left -d @/right {URL}"))
        .await
        .unwrap();

    assert_eq!(result.exit_code, 2);
    assert!(result.stderr.contains("request body too large"));
    assert!(requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn aggregate_body_at_exact_limit_is_allowed() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(std::path::Path::new("/left"), &vec![b'a'; BODY_LIMIT / 2])
        .await
        .unwrap();
    fs.write_file(
        std::path::Path::new("/right"),
        &vec![b'b'; BODY_LIMIT / 2 - 1],
    )
    .await
    .unwrap();
    let (mut bash, requests) = bash_with_capture(Some(fs));
    let result = bash
        .exec(&format!(
            "curl -s --data-binary @/left --data-binary @/right {URL}"
        ))
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(
        requests
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .body
            .as_ref()
            .unwrap()
            .len(),
        BODY_LIMIT
    );
}

#[tokio::test]
async fn missing_files_fail_for_each_file_aware_variant() {
    for option in ["-d", "--data-binary", "--data-urlencode"] {
        let (mut bash, requests) = bash_with_capture(None);
        let result = bash
            .exec(&format!("curl -s {option} @/missing {URL}"))
            .await
            .unwrap();

        assert_eq!(result.exit_code, 26, "option {option}: {}", result.stderr);
        assert!(result.stderr.contains("Failed reading data file /missing"));
        assert!(requests.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn data_raw_treats_at_prefix_as_literal_and_custom_content_type_wins() {
    let (mut bash, requests) = bash_with_capture(None);
    let result = bash
        .exec(&format!(
            "curl -s -H 'Content-Type: application/json' --data-raw @missing {URL}"
        ))
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    let requests = requests.lock().unwrap();
    let request = requests.last().unwrap();
    assert_eq!(request.body.as_deref(), Some(b"@missing".as_slice()));
    let content_types: Vec<_> = request
        .headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.as_str())
        .collect();
    assert_eq!(content_types, ["application/json"]);
}

#[tokio::test]
async fn stdin_data_is_resolved_in_order() {
    let (mut bash, requests) = bash_with_capture(None);
    let result = bash
        .exec(&format!(
            "printf 'a\\nb\\n' | curl -s --data-binary @- -d c=3 {URL}"
        ))
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(
        requests.lock().unwrap().last().unwrap().body.as_deref(),
        Some(b"a\nb\n&c=3".as_slice())
    );
}

#[tokio::test]
async fn empty_repeated_parts_still_contribute_separators() {
    let (mut bash, requests) = bash_with_capture(None);
    let result = bash
        .exec(&format!("curl -s -d '' -d a=1 -d '' {URL}"))
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(
        requests.lock().unwrap().last().unwrap().body.as_deref(),
        Some(b"&a=1&".as_slice())
    );
}

#[tokio::test]
async fn get_rejects_non_utf8_binary_query_data() {
    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(std::path::Path::new("/binary"), &[0xff])
        .await
        .unwrap();
    let (mut bash, requests) = bash_with_capture(Some(fs));
    let result = bash
        .exec(&format!("curl -s -G --data-binary @/binary {URL}"))
        .await
        .unwrap();

    assert_eq!(result.exit_code, 3);
    assert!(result.stderr.contains("valid URL query"));
    assert!(requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn get_preserves_explicit_method_regardless_of_option_order() {
    for command in [
        format!("curl -s -X POST -G -d a=1 {URL}"),
        format!("curl -s -G -X POST -d a=1 {URL}"),
    ] {
        let (mut bash, requests) = bash_with_capture(None);
        let result = bash.exec(&command).await.unwrap();

        assert_eq!(result.exit_code, 0, "{}", result.stderr);
        let requests = requests.lock().unwrap();
        let request = requests.last().unwrap();
        assert_eq!(request.method.as_str(), "POST");
        assert_eq!(request.url, format!("{URL}?a=1"));
        assert_eq!(request.body, None);
    }
}

#[tokio::test]
async fn long_data_options_accept_equals_syntax() {
    let (mut bash, requests) = bash_with_capture(None);
    let result = bash
        .exec(&format!(
            "curl -s --data=a=1 --data-raw=@literal --data-binary=b=2 --data-urlencode='q=a b' {URL}"
        ))
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(
        requests.lock().unwrap().last().unwrap().body.as_deref(),
        Some(b"a=1&@literal&b=2&q=a+b".as_slice())
    );
}

#[derive(Debug, PartialEq, Eq)]
struct RequestSummary {
    method: String,
    target: String,
    body: Vec<u8>,
    content_type: Option<String>,
}

fn summarize_bashkit_request(request: &HttpTransportRequest) -> RequestSummary {
    let parsed = url::Url::parse(&request.url).unwrap();
    let target = match parsed.query() {
        Some(query) => format!("{}?{}", parsed.path(), query),
        None => parsed.path().to_string(),
    };
    RequestSummary {
        method: request.method.as_str().to_string(),
        target,
        body: request.body.clone().unwrap_or_default(),
        content_type: request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.clone()),
    }
}

fn run_real_curl(args: &[&str]) -> RequestSummary {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::process::Command;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let owned_args: Vec<String> = args
        .iter()
        .map(|arg| arg.replace("{URL}", &format!("http://{address}/echo")))
        .collect();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 8192];
        let header_end = loop {
            let count = stream.read(&mut buffer).unwrap();
            assert_ne!(count, 0, "curl closed before sending complete headers");
            request.extend_from_slice(&buffer[..count]);
            if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8(request[..header_end].to_vec()).unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length: ")
                    .or_else(|| line.strip_prefix("content-length: "))
            })
            .map_or(0, |value| value.trim().parse().unwrap());
        while request.len() - header_end < content_length {
            let count = stream.read(&mut buffer).unwrap();
            assert_ne!(count, 0, "curl closed before sending complete body");
            request.extend_from_slice(&buffer[..count]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();

        let mut lines = headers.lines();
        let mut request_line = lines.next().unwrap().split_whitespace();
        let method = request_line.next().unwrap().to_string();
        let target = request_line.next().unwrap().to_string();
        let content_type = lines.find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-type")
                    .then(|| value.trim().to_string())
            })
        });
        RequestSummary {
            method,
            target,
            body: request[header_end..header_end + content_length].to_vec(),
            content_type,
        }
    });

    let output = Command::new("curl")
        .arg("-sS")
        .args(&owned_args)
        .output()
        .expect("real curl must be installed for differential tests");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    server.join().unwrap()
}

#[tokio::test]
async fn differential_real_curl_matches_mixed_post_data() {
    let expected = run_real_curl(&[
        "-d",
        "a=1",
        "--data-raw",
        "@literal",
        "--data-binary",
        "b=2",
        "--data-urlencode",
        "q=a b*",
        "--data-urlencode",
        "symbols= !'()*~",
        "{URL}",
    ]);
    let (mut bash, requests) = bash_with_capture(None);
    let result = bash
        .exec(&format!(
            "curl -s -d a=1 --data-raw @literal --data-binary b=2 --data-urlencode 'q=a b*' --data-urlencode \"symbols= !'()*~\" {URL}"
        ))
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(
        summarize_bashkit_request(requests.lock().unwrap().last().unwrap()),
        expected
    );
}

#[tokio::test]
async fn differential_real_curl_matches_get_query_aggregation() {
    let expected = run_real_curl(&[
        "-G",
        "{URL}?fixed=1",
        "-d",
        "a=1",
        "--data-urlencode",
        "q=a b*",
    ]);
    let (mut bash, requests) = bash_with_capture(None);
    let result = bash
        .exec(&format!(
            "curl -s -G '{URL}?fixed=1' -d a=1 --data-urlencode 'q=a b*'"
        ))
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(
        summarize_bashkit_request(requests.lock().unwrap().last().unwrap()),
        expected
    );
}

#[tokio::test]
async fn differential_real_curl_matches_each_at_file_variant() {
    let temp = tempfile::tempdir().unwrap();
    let real_path = temp.path().join("payload");
    std::fs::write(&real_path, b"a=1\r\nb=2\n").unwrap();
    let real_arg = format!("@{}", real_path.display());
    let named_real_arg = format!("p@{}", real_path.display());
    let expected = run_real_curl(&[
        "-d",
        &real_arg,
        "--data-binary",
        &real_arg,
        "--data-urlencode",
        &named_real_arg,
        "{URL}",
    ]);

    let fs = Arc::new(InMemoryFs::new());
    fs.write_file(std::path::Path::new("/payload"), b"a=1\r\nb=2\n")
        .await
        .unwrap();
    let (mut bash, requests) = bash_with_capture(Some(fs));
    let result = bash
        .exec(&format!(
            "curl -s -d @/payload --data-binary @/payload --data-urlencode p@/payload {URL}"
        ))
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(
        summarize_bashkit_request(requests.lock().unwrap().last().unwrap()),
        expected
    );
}
