//! WASM-specific smoke tests for bashkit.
//!
//! Run with: wasm-pack test --headless --chrome

use wasm_bindgen_test::*;
wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn system_time_now_does_not_panic() {
    let _now = bashkit::time::SystemTime::now();
}

#[wasm_bindgen_test]
fn unix_epoch_is_before_now() {
    let epoch = bashkit::time::UNIX_EPOCH;
    let now = bashkit::time::SystemTime::now();
    let duration = now
        .duration_since(epoch)
        .expect("now should be after epoch");
    assert!(duration.as_secs() > 1_000_000_000, "expected year 2001+");
}

#[wasm_bindgen_test]
fn chrono_roundtrip_utc() {
    let now = bashkit::time::SystemTime::now();
    let dt = bashkit::time::to_chrono_utc(now);
    let back = bashkit::time::from_chrono(dt);

    let diff = now
        .duration_since(back)
        .unwrap_or_else(|e| e.duration())
        .as_millis();
    assert!(diff < 2, "roundtrip drift should be < 2ms, got {}ms", diff);
}

#[wasm_bindgen_test]
fn duration_arithmetic() {
    let a = bashkit::time::Duration::from_secs(10);
    let b = bashkit::time::Duration::from_secs(5);
    assert_eq!((a + b).as_secs(), 15);
}

// ---------------------------------------------------------------------------
// IndexedDB filesystem tests
// ---------------------------------------------------------------------------

use bashkit::{FileSystem, FsBackend, IndexedDbFs, PosixFs};
use std::path::Path;
use std::sync::Arc;

/// Unique DB name per test to avoid collisions.
fn db_name(test: &str) -> String {
    format!("bashkit_test_{}", test)
}

#[wasm_bindgen_test]
async fn indexeddb_fs_write_and_read_file() {
    let fs = IndexedDbFs::new(db_name("write_read"));
    fs.write(Path::new("/tmp/test.txt"), b"hello world")
        .await
        .unwrap();
    let content = fs.read(Path::new("/tmp/test.txt")).await.unwrap();
    assert_eq!(content, b"hello world");
}

#[wasm_bindgen_test]
async fn indexeddb_fs_append_file() {
    let fs = IndexedDbFs::new(db_name("append"));
    fs.write(Path::new("/tmp/log.txt"), b"line1\n")
        .await
        .unwrap();
    fs.append(Path::new("/tmp/log.txt"), b"line2\n")
        .await
        .unwrap();
    let content = fs.read(Path::new("/tmp/log.txt")).await.unwrap();
    assert_eq!(content, b"line1\nline2\n");
}

#[wasm_bindgen_test]
async fn indexeddb_fs_mkdir_and_exists() {
    let fs = IndexedDbFs::new(db_name("mkdir"));
    fs.mkdir(Path::new("/data/nested"), true).await.unwrap();
    assert!(fs.exists(Path::new("/data")).await.unwrap());
    assert!(fs.exists(Path::new("/data/nested")).await.unwrap());
    assert!(!fs.exists(Path::new("/data/nested/missing")).await.unwrap());
}

#[wasm_bindgen_test]
async fn indexeddb_fs_read_dir() {
    let fs = IndexedDbFs::new(db_name("read_dir"));
    fs.mkdir(Path::new("/tmp/sub"), true).await.unwrap();
    fs.write(Path::new("/tmp/a.txt"), b"a").await.unwrap();
    fs.write(Path::new("/tmp/b.txt"), b"b").await.unwrap();

    let entries = fs.read_dir(Path::new("/tmp")).await.unwrap();
    let mut names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
    names.sort();
    assert_eq!(names, vec!["a.txt", "b.txt", "sub"]);
}

#[wasm_bindgen_test]
async fn indexeddb_fs_remove_file() {
    let fs = IndexedDbFs::new(db_name("remove_file"));
    fs.write(Path::new("/tmp/del.txt"), b"x").await.unwrap();
    assert!(fs.exists(Path::new("/tmp/del.txt")).await.unwrap());
    fs.remove(Path::new("/tmp/del.txt"), false).await.unwrap();
    assert!(!fs.exists(Path::new("/tmp/del.txt")).await.unwrap());
}

#[wasm_bindgen_test]
async fn indexeddb_fs_remove_dir_recursive() {
    let fs = IndexedDbFs::new(db_name("remove_dir"));
    fs.mkdir(Path::new("/tmp/deep/nested"), true).await.unwrap();
    fs.write(Path::new("/tmp/deep/file.txt"), b"x")
        .await
        .unwrap();
    fs.remove(Path::new("/tmp/deep"), true).await.unwrap();
    assert!(!fs.exists(Path::new("/tmp/deep")).await.unwrap());
    assert!(!fs.exists(Path::new("/tmp/deep/nested")).await.unwrap());
    assert!(!fs.exists(Path::new("/tmp/deep/file.txt")).await.unwrap());
}

#[wasm_bindgen_test]
async fn indexeddb_fs_stat() {
    let fs = IndexedDbFs::new(db_name("stat"));
    fs.write(Path::new("/tmp/stats.txt"), b"12345")
        .await
        .unwrap();
    let meta = fs.stat(Path::new("/tmp/stats.txt")).await.unwrap();
    assert!(meta.file_type.is_file());
    assert_eq!(meta.size, 5);
    assert_eq!(meta.mode, 0o644);
}

#[wasm_bindgen_test]
async fn indexeddb_fs_rename_file() {
    let fs = IndexedDbFs::new(db_name("rename_file"));
    fs.write(Path::new("/tmp/old.txt"), b"data").await.unwrap();
    fs.rename(Path::new("/tmp/old.txt"), Path::new("/tmp/new.txt"))
        .await
        .unwrap();
    assert!(!fs.exists(Path::new("/tmp/old.txt")).await.unwrap());
    assert!(fs.exists(Path::new("/tmp/new.txt")).await.unwrap());
    assert_eq!(fs.read(Path::new("/tmp/new.txt")).await.unwrap(), b"data");
}

#[wasm_bindgen_test]
async fn indexeddb_fs_copy_file() {
    let fs = IndexedDbFs::new(db_name("copy_file"));
    fs.write(Path::new("/tmp/src.txt"), b"copy me")
        .await
        .unwrap();
    fs.copy(Path::new("/tmp/src.txt"), Path::new("/tmp/dst.txt"))
        .await
        .unwrap();
    assert_eq!(
        fs.read(Path::new("/tmp/src.txt")).await.unwrap(),
        b"copy me"
    );
    assert_eq!(
        fs.read(Path::new("/tmp/dst.txt")).await.unwrap(),
        b"copy me"
    );
}

#[wasm_bindgen_test]
async fn indexeddb_fs_symlink() {
    let fs = IndexedDbFs::new(db_name("symlink"));
    fs.symlink(Path::new("/tmp/target.txt"), Path::new("/tmp/link.txt"))
        .await
        .unwrap();
    let target = fs.read_link(Path::new("/tmp/link.txt")).await.unwrap();
    assert_eq!(target, Path::new("/tmp/target.txt"));
}

#[wasm_bindgen_test]
async fn indexeddb_fs_chmod() {
    let fs = IndexedDbFs::new(db_name("chmod"));
    fs.write(Path::new("/tmp/perms.txt"), b"x").await.unwrap();
    fs.chmod(Path::new("/tmp/perms.txt"), 0o755).await.unwrap();
    let meta = fs.stat(Path::new("/tmp/perms.txt")).await.unwrap();
    assert_eq!(meta.mode, 0o755);
}

#[wasm_bindgen_test]
async fn indexeddb_fs_posix_wrapper() {
    let backend = IndexedDbFs::new(db_name("posix"));
    let fs = Arc::new(PosixFs::new(backend));

    // Create parent dir first — IndexedDB fs doesn't auto-create parents
    fs.mkdir(Path::new("/tmp"), false).await.unwrap();

    // POSIX semantics: write -> read roundtrip
    fs.write_file(Path::new("/tmp/posix.txt"), b"posix")
        .await
        .unwrap();
    let content = fs.read_file(Path::new("/tmp/posix.txt")).await.unwrap();
    assert_eq!(content, b"posix");

    // POSIX semantics: cannot write to a directory
    fs.mkdir(Path::new("/tmp/dir"), false).await.unwrap();
    let result = fs.write_file(Path::new("/tmp/dir"), b"x").await;
    assert!(result.is_err(), "writing to a directory should fail");

    // verify_filesystem_requirements smoke test
    bashkit::verify_filesystem_requirements(&*fs).await.unwrap();
}

// ---------------------------------------------------------------------------
// HTTP client tests
// ---------------------------------------------------------------------------

use bashkit::{HttpClient, HttpHandler, HttpResponse, Method, NetworkAllowlist};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

struct MockHandler {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[async_trait::async_trait]
impl HttpHandler for MockHandler {
    async fn request(
        &self,
        _method: &str,
        _url: &str,
        _body: Option<&[u8]>,
        _headers: &[(String, String)],
    ) -> Result<HttpResponse, String> {
        Ok(HttpResponse {
            status: self.status,
            headers: self.headers.clone(),
            body: self.body.clone(),
        })
    }
}

struct EchoHandler;

#[async_trait::async_trait]
impl HttpHandler for EchoHandler {
    async fn request(
        &self,
        method: &str,
        url: &str,
        body: Option<&[u8]>,
        headers: &[(String, String)],
    ) -> Result<HttpResponse, String> {
        let mut body_out = Vec::new();
        body_out.extend_from_slice(format!("{} {}\n", method, url).as_bytes());
        for (k, v) in headers {
            body_out.extend_from_slice(format!("{}:{}\n", k, v).as_bytes());
        }
        if let Some(b) = body {
            body_out.extend_from_slice(b);
        }
        Ok(HttpResponse {
            status: 200,
            headers: vec![("Content-Type".to_string(), "text/plain".to_string())],
            body: body_out,
        })
    }
}

#[wasm_bindgen_test]
fn http_client_new() {
    let client = HttpClient::new(NetworkAllowlist::allow_all());
    assert_eq!(client.max_response_bytes(), 10 * 1024 * 1024);
}

#[wasm_bindgen_test]
fn http_client_with_timeout() {
    let client = HttpClient::with_timeout(NetworkAllowlist::allow_all(), Duration::from_secs(60));
    assert_eq!(client.max_response_bytes(), 10 * 1024 * 1024);
}

#[wasm_bindgen_test]
fn http_client_with_config() {
    let client =
        HttpClient::with_config(NetworkAllowlist::allow_all(), Duration::from_secs(5), 1024);
    assert_eq!(client.max_response_bytes(), 1024);
}

#[wasm_bindgen_test]
async fn http_blocked_by_empty_allowlist() {
    let client = HttpClient::new(NetworkAllowlist::new());
    let result = client.get("https://example.com").await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("access denied"),
        "expected access denied, got: {}",
        msg
    );
}

#[wasm_bindgen_test]
async fn http_allowed_by_allowlist() {
    let mut client = HttpClient::new(NetworkAllowlist::new().allow("https://example.com"));
    client.set_handler(Box::new(MockHandler {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    }));
    let result = client.get("https://example.com").await;
    assert!(result.is_ok());
    let resp = result.unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, b"ok");
}

#[wasm_bindgen_test]
async fn http_blocked_by_allowlist() {
    let mut client = HttpClient::new(NetworkAllowlist::new().allow("https://allowed.com"));
    client.set_handler(Box::new(MockHandler {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    }));
    let result = client.get("https://blocked.com").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("access denied"));
}

#[wasm_bindgen_test]
async fn http_get_method() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(EchoHandler));
    let result = client.get("https://example.com/path").await.unwrap();
    assert_eq!(result.status, 200);
    let text = result.body_string();
    assert!(text.starts_with("GET https://example.com/path"));
}

#[wasm_bindgen_test]
async fn http_post_method() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(EchoHandler));
    let result = client
        .post("https://example.com/path", Some(b"hello"))
        .await
        .unwrap();
    assert_eq!(result.status, 200);
    let text = result.body_string();
    assert!(text.starts_with("POST https://example.com/path"));
    assert!(text.ends_with("hello"));
}

#[wasm_bindgen_test]
async fn http_put_method() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(EchoHandler));
    let result = client
        .put("https://example.com/path", Some(b"world"))
        .await
        .unwrap();
    assert_eq!(result.status, 200);
    let text = result.body_string();
    assert!(text.starts_with("PUT https://example.com/path"));
    assert!(text.ends_with("world"));
}

#[wasm_bindgen_test]
async fn http_delete_method() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(EchoHandler));
    let result = client.delete("https://example.com/path").await.unwrap();
    assert_eq!(result.status, 200);
    assert!(
        result
            .body_string()
            .starts_with("DELETE https://example.com/path")
    );
}

#[wasm_bindgen_test]
async fn http_head_method() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(MockHandler {
        status: 204,
        headers: vec![("X-Test".to_string(), "1".to_string())],
        body: vec![],
    }));
    let result = client.head("https://example.com/path").await.unwrap();
    assert_eq!(result.status, 204);
    assert!(result.headers.iter().any(|(k, _)| k == "X-Test"));
}

#[wasm_bindgen_test]
async fn http_request_with_headers() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(EchoHandler));
    let result = client
        .request_with_headers(
            Method::Get,
            "https://example.com",
            None,
            &[("Authorization".to_string(), "Bearer token".to_string())],
        )
        .await
        .unwrap();
    let text = result.body_string();
    assert!(text.contains("Authorization:Bearer token"));
}

#[wasm_bindgen_test]
async fn http_request_with_timeout() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(MockHandler {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    }));
    let result = client
        .request_with_timeout(Method::Get, "https://example.com", None, &[], Some(5))
        .await;
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
async fn http_request_with_timeouts() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(MockHandler {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    }));
    let result = client
        .request_with_timeouts(
            Method::Get,
            "https://example.com",
            None,
            &[],
            Some(5),
            Some(2),
        )
        .await;
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
async fn http_response_body_string() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(MockHandler {
        status: 200,
        headers: vec![],
        body: b"hello world".to_vec(),
    }));
    let result = client.get("https://example.com").await.unwrap();
    assert_eq!(result.body_string(), "hello world");
}

#[wasm_bindgen_test]
async fn http_response_is_success() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(MockHandler {
        status: 201,
        headers: vec![],
        body: vec![],
    }));
    let result = client.get("https://example.com").await.unwrap();
    assert!(result.is_success());
}

#[wasm_bindgen_test]
async fn http_response_is_not_success() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(MockHandler {
        status: 404,
        headers: vec![],
        body: b"not found".to_vec(),
    }));
    let result = client.get("https://example.com").await.unwrap();
    assert!(!result.is_success());
}

#[wasm_bindgen_test]
async fn http_max_response_bytes_enforced() {
    let mut client =
        HttpClient::with_config(NetworkAllowlist::allow_all(), Duration::from_secs(30), 4);
    client.set_handler(Box::new(MockHandler {
        status: 200,
        headers: vec![],
        body: b"too-large".to_vec(),
    }));
    let result = client.get("https://example.com").await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("response too large")
    );
}

#[wasm_bindgen_test]
async fn http_before_http_hook() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(EchoHandler));
    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = count.clone();
    client.set_before_http(vec![Box::new(move |mut event| {
        count_clone.fetch_add(1, Ordering::SeqCst);
        event
            .headers
            .push(("X-Hook".to_string(), "fired".to_string()));
        bashkit::hooks::HookAction::Continue(event)
    })]);
    let result = client.get("https://example.com").await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(result.body_string().contains("X-Hook:fired"));
}

#[wasm_bindgen_test]
async fn http_after_http_hook() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(MockHandler {
        status: 200,
        headers: vec![("X-Original".to_string(), "1".to_string())],
        body: b"ok".to_vec(),
    }));
    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = count.clone();
    client.set_after_http(vec![Box::new(move |event| {
        count_clone.fetch_add(1, Ordering::SeqCst);
        bashkit::hooks::HookAction::Continue(event)
    })]);
    let _ = client.get("https://example.com").await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[wasm_bindgen_test]
async fn http_before_http_hook_can_cancel() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(EchoHandler));
    client.set_before_http(vec![Box::new(|_event| {
        bashkit::hooks::HookAction::Cancel("nope".to_string())
    })]);
    let result = client.get("https://example.com").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cancelled"));
}

#[wasm_bindgen_test]
async fn http_get_blocks_private_ip() {
    let client = HttpClient::new(NetworkAllowlist::allow_all());
    let result = client.get("http://10.0.0.1/secret").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("private IP"));
}

#[wasm_bindgen_test]
async fn http_get_blocks_loopback() {
    let client = HttpClient::new(NetworkAllowlist::allow_all());
    let result = client.get("http://127.0.0.1/").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("private IP"));
}

#[wasm_bindgen_test]
async fn http_get_allows_public_via_handler() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(MockHandler {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    }));
    let result = client.get("https://example.com/").await;
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
async fn http_get_rejects_no_host() {
    let client = HttpClient::new(NetworkAllowlist::allow_all());
    let result = client.get("file:///etc/passwd").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no host"));
}

#[wasm_bindgen_test]
async fn http_get_rejects_invalid_url() {
    let client = HttpClient::new(NetworkAllowlist::allow_all());
    let result = client.get("definitely::not::a::url").await;
    assert!(result.is_err());
}

#[wasm_bindgen_test]
async fn http_request_with_headers_blocked_by_allowlist() {
    let client = HttpClient::new(NetworkAllowlist::new());
    let result = client
        .request_with_headers(Method::Get, "https://example.com", None, &[])
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("access denied"));
}

#[wasm_bindgen_test]
async fn http_request_with_timeout_blocked_by_allowlist() {
    let client = HttpClient::new(NetworkAllowlist::new());
    let result = client
        .request_with_timeout(Method::Get, "https://example.com", None, &[], Some(5))
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("access denied"));
}

#[wasm_bindgen_test]
async fn http_before_http_hook_cannot_bypass_allowlist() {
    let mut client = HttpClient::new(NetworkAllowlist::new().allow("https://allowed.com"));
    client.set_handler(Box::new(MockHandler {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    }));
    client.set_before_http(vec![Box::new(|mut event| {
        event.url = "https://blocked.com".to_string();
        bashkit::hooks::HookAction::Continue(event)
    })]);
    let result = client
        .request_with_headers(Method::Get, "https://allowed.com", None, &[])
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("access denied"));
}

#[wasm_bindgen_test]
async fn http_empty_body_ok() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(EchoHandler));
    let result = client.post("https://example.com", None).await.unwrap();
    assert_eq!(result.status, 200);
}

#[wasm_bindgen_test]
async fn http_handler_receives_headers() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(EchoHandler));
    let result = client
        .request_with_headers(
            Method::Get,
            "https://example.com",
            None,
            &[
                ("Accept".to_string(), "application/json".to_string()),
                ("X-Custom".to_string(), "value".to_string()),
            ],
        )
        .await
        .unwrap();
    let text = result.body_string();
    assert!(text.contains("Accept:application/json"));
    assert!(text.contains("X-Custom:value"));
}

#[wasm_bindgen_test]
async fn http_multiple_before_hooks_fire_in_order() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(EchoHandler));
    let order = Arc::new(AtomicUsize::new(0));
    let order1 = order.clone();
    let order2 = order.clone();
    client.set_before_http(vec![
        Box::new(move |event| {
            order1.fetch_add(1, Ordering::SeqCst);
            bashkit::hooks::HookAction::Continue(event)
        }),
        Box::new(move |event| {
            order2.fetch_add(10, Ordering::SeqCst);
            bashkit::hooks::HookAction::Continue(event)
        }),
    ]);
    let _ = client.get("https://example.com").await.unwrap();
    assert_eq!(order.load(Ordering::SeqCst), 11);
}

#[wasm_bindgen_test]
async fn http_multiple_after_hooks_fire_in_order() {
    let mut client = HttpClient::new(NetworkAllowlist::allow_all());
    client.set_handler(Box::new(MockHandler {
        status: 200,
        headers: vec![],
        body: b"ok".to_vec(),
    }));
    let order = Arc::new(AtomicUsize::new(0));
    let order1 = order.clone();
    let order2 = order.clone();
    client.set_after_http(vec![
        Box::new(move |event| {
            order1.fetch_add(1, Ordering::SeqCst);
            bashkit::hooks::HookAction::Continue(event)
        }),
        Box::new(move |event| {
            order2.fetch_add(10, Ordering::SeqCst);
            bashkit::hooks::HookAction::Continue(event)
        }),
    ]);
    let _ = client.get("https://example.com").await.unwrap();
    assert_eq!(order.load(Ordering::SeqCst), 11);
}

#[wasm_bindgen_test]
async fn http_v6_loopback_blocked() {
    let client = HttpClient::new(NetworkAllowlist::allow_all());
    let result = client.get("http://[::1]/").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("private IP"));
}

#[wasm_bindgen_test]
async fn http_v4_mapped_v6_blocked() {
    let client = HttpClient::new(NetworkAllowlist::allow_all());
    let result = client.get("http://[::ffff:10.0.0.1]/").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("private IP"));
}
