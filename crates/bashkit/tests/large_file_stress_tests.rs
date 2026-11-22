//! Large-file integrity and resource stress tests.
//!
//! Decisions: separate binary for process-wide allocation measurement;
//! current-thread tests share a serial lock. Fixtures are created before measuring,
//! so peaks report incremental command heap, including output retained in VFS.
//! A deterministic transport exercises the HTTP host boundary; a loopback HTTP
//! server also exercises reqwest with content-length and chunked bodies, without
//! internet variability. This does not benchmark external networks or TLS.
//! Default 1 MiB is CI-sized. BASHKIT_STRESS_MIB=64 scales the same assertions.
//! Timing is diagnostic only; heap has a generous linear regression ceiling.

#![cfg(all(feature = "http_client", feature = "jq"))]

use async_trait::async_trait;
use bashkit::{
    Bash, BashBuilder, ExecutionLimits, FsLimits, HttpLimits, HttpResponse, HttpTransport,
    HttpTransportError, HttpTransportRequest, InMemoryFs, NetworkAllowlist,
};
use sha2::{Digest, Sha256};
use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) };
    }

    // Count transient realloc overlap conservatively, even if the system
    // allocator could have grown in place.
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, size) };
        if !new.is_null() {
            let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
            PEAK.fetch_max(live, Ordering::Relaxed);
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const URL: &str = "http://93.184.216.34/fixture";
const ROW: &[u8] = b"{\"n\":1,\"s\":\"abcdefghijklmnopqrstuvwxyz0123456789\"}\n";

struct FixtureTransport(Arc<Vec<u8>>);

#[async_trait]
impl HttpTransport for FixtureTransport {
    async fn execute(&self, _: HttpTransportRequest) -> Result<HttpResponse, HttpTransportError> {
        // Deliberately ignore the advertised limit to exercise the host-side
        // backstop as well as successful transfers.
        Ok(HttpResponse {
            status: 200,
            headers: vec![],
            body: self.0.as_ref().clone(),
        })
    }
}

fn builder(bytes: usize) -> BashBuilder {
    Bash::builder()
        .fs(Arc::new(InMemoryFs::with_limits(FsLimits {
            max_file_size: (bytes * 4) as u64,
            max_total_bytes: (bytes * 16) as u64,
            ..Default::default()
        })))
        .limits(
            ExecutionLimits::new()
                .timeout(Duration::from_secs(120))
                .max_work_units((bytes as u64) * 256)
                .max_aggregate_input_bytes((bytes as u64) * 64)
                .max_live_intermediate_bytes((bytes as u64) * 64),
        )
        .http_limits(HttpLimits::default().max_response_bytes(bytes))
}

fn shell(bytes: usize, body: Arc<Vec<u8>>) -> Bash {
    builder(bytes)
        .network(NetworkAllowlist::allow_all())
        .http_transport(Arc::new(FixtureTransport(body)))
        .build()
}

async fn serve(mut socket: tokio::net::TcpStream, body: &[u8]) -> std::io::Result<()> {
    let mut request = Vec::new();
    let mut buf = [0; 2048];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = socket.read(&mut buf).await?;
        if n == 0 || request.len() + n > 16_384 {
            return Ok(());
        }
        request.extend_from_slice(&buf[..n]);
    }
    let chunked = request.starts_with(b"GET /chunked ");
    let framing = if chunked {
        "Transfer-Encoding: chunked".into()
    } else {
        format!("Content-Length: {}", body.len())
    };
    socket
        .write_all(format!("HTTP/1.1 200 OK\r\n{framing}\r\nConnection: close\r\n\r\n").as_bytes())
        .await?;
    if chunked {
        for chunk in body.chunks(64 << 10) {
            socket
                .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                .await?;
            socket.write_all(chunk).await?;
            socket.write_all(b"\r\n").await?;
        }
        socket.write_all(b"0\r\n\r\n").await?;
    } else {
        socket.write_all(body).await?;
    }
    socket.shutdown().await
}

async fn measure(bash: &mut Bash, script: &str, expected: &str, bytes: usize) {
    // Warm caches outside the measured interval.
    assert_eq!(bash.exec("true").await.unwrap().exit_code, 0);
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let start = Instant::now();
    let result = bash.exec(script).await.expect(script);
    let elapsed = start.elapsed();
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    eprintln!(
        "{script}\tinput={bytes}\tpeak={peak}\tms={:.2}\texit={}",
        elapsed.as_secs_f64() * 1000.0,
        result.exit_code
    );
    assert_eq!(result.exit_code, 0, "{script}: {}", result.stderr);
    assert_eq!(result.stdout.trim(), expected, "{script}");
    assert!(result.stderr.is_empty(), "{script}: {}", result.stderr);
    assert!(peak < bytes * 32 + (8 << 20), "{script}: peak={peak}");
    // Each command owns its derived files; discard between measurements.
    let cleanup = bash
        .exec("rm -f /out /encoded /decoded /packed.gz")
        .await
        .unwrap();
    assert_eq!(cleanup.exit_code, 0, "{}", cleanup.stderr);
}

#[tokio::test(flavor = "current_thread")]
async fn large_files_preserve_bytes_and_finish_with_linear_heap() {
    let _serial = SERIAL.lock().await;
    let mib = std::env::var("BASHKIT_STRESS_MIB")
        .map(|s| {
            s.parse::<usize>()
                .expect("BASHKIT_STRESS_MIB must be an integer")
        })
        .unwrap_or(1);
    assert!((1..=256).contains(&mib), "scale must be 1..=256 MiB");
    let bytes = mib * (1 << 20);
    // Incompressible deterministic bytes exercise full-size compression paths
    // as well as invalid UTF-8 and NUL handling.
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let binary = Arc::new(
        (0..bytes)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect::<Vec<_>>(),
    );
    let digest: String = Sha256::digest(binary.as_slice())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let mut bash = shell(bytes, binary.clone());
    bash.fs()
        .write_file(Path::new("/binary"), &binary)
        .await
        .unwrap();
    let rows = bytes / ROW.len();
    let json = ROW.repeat(rows);
    bash.fs()
        .write_file(Path::new("/data.jsonl"), &json)
        .await
        .unwrap();
    drop(json);
    let mut string = vec![b'x'; bytes];
    string[0] = b'"';
    string[bytes - 1] = b'"';
    bash.fs()
        .write_file(Path::new("/string.json"), &string)
        .await
        .unwrap();
    drop(string);

    for command in [
        format!("curl -s -o /out {URL}"),
        format!("curl -s {URL} > /out"),
        format!("wget -q -O /out {URL}"),
        format!("wget -q -O - {URL} > /out"),
        "cp /binary /out".into(),
    ] {
        measure(
            &mut bash,
            &format!("{command} && sha256sum /out"),
            &format!("{digest}  /out"),
            bytes,
        )
        .await;
    }
    for command in [
        format!("curl -s {URL} | sha256sum"),
        format!("wget -q -O - {URL} | sha256sum"),
        "cat /binary | sha256sum".into(),
    ] {
        measure(&mut bash, &command, &format!("{digest}  -"), bytes).await;
    }
    for command in [
        "base64 /binary > /encoded && base64 -d /encoded > /decoded && sha256sum /decoded",
        "gzip -c /binary > /packed.gz && gzip -dc /packed.gz > /decoded && sha256sum /decoded",
    ] {
        measure(&mut bash, command, &format!("{digest}  /decoded"), bytes).await;
    }
    for (command, expected) in [
        ("wc -c < /binary", bytes.to_string()),
        ("wc -l < /data.jsonl", rows.to_string()),
        ("grep -c abcdef /data.jsonl", rows.to_string()),
        ("rg -c abcdef /data.jsonl", rows.to_string()),
        ("sed -n '$=' /data.jsonl", rows.to_string()),
        ("awk 'END { print NR }' /data.jsonl", rows.to_string()),
        ("cut -d: -f1 /data.jsonl | wc -l", rows.to_string()),
        (
            "head -n 1 /data.jsonl",
            String::from_utf8_lossy(ROW).trim().into(),
        ),
        (
            "tail -n 1 /data.jsonl",
            String::from_utf8_lossy(ROW).trim().into(),
        ),
        ("sort /data.jsonl | uniq | wc -l", "1".into()),
        (
            "jq -n 'reduce inputs as $r (0; . + $r.n)' /data.jsonl",
            rows.to_string(),
        ),
        ("jq -s 'length' /data.jsonl", rows.to_string()),
        ("jq 'length' /string.json", (bytes - 2).to_string()),
        (
            "jq --stream -n 'reduce inputs as $e (0; if ($e|length)==2 and $e[0]==[\"n\"] then .+$e[1] else . end)' /data.jsonl",
            rows.to_string(),
        ),
    ] {
        measure(&mut bash, command, &expected, bytes).await;
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let served_body = binary.clone();
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            // A capped client closes early; a broken pipe is expected there.
            let _ = serve(socket, &served_body).await;
        }
    });
    let network = NetworkAllowlist::new()
        .block_private_ips(false)
        .allow(&base_url);
    let mut native = builder(bytes).network(network.clone()).build();
    for path in ["length", "chunked"] {
        for tool in ["curl -s -o", "wget -q -O"] {
            measure(
                &mut native,
                &format!("{tool} /out {base_url}/{path} && sha256sum /out"),
                &format!("{digest}  /out"),
                bytes,
            )
            .await;
        }
    }
    let mut native_limited = builder(bytes - 1).network(network).build();
    for path in ["length", "chunked"] {
        for tool in ["curl -s -o", "wget -q -O"] {
            native_limited
                .fs()
                .write_file(Path::new("/out"), b"sentinel")
                .await
                .unwrap();
            let result = native_limited
                .exec(&format!("{tool} /out {base_url}/{path}"))
                .await
                .unwrap();
            assert_ne!(
                result.exit_code, 0,
                "native {tool} {path} must reject oversized response"
            );
            assert!(result.stderr.contains("too large"), "{}", result.stderr);
            assert_eq!(
                native_limited
                    .fs()
                    .read_file(Path::new("/out"))
                    .await
                    .unwrap(),
                b"sentinel"
            );
            assert_eq!(
                native_limited.exec("echo alive").await.unwrap().stdout,
                "alive\n"
            );
        }
    }
    server.abort();

    // One byte over the HTTP cap: no output-file replacement, no poisoned shell.
    let mut limited = shell(bytes - 1, binary);
    limited
        .fs()
        .write_file(Path::new("/out"), b"sentinel")
        .await
        .unwrap();
    for (command, code) in [
        (format!("curl -s -o /out {URL}"), 63),
        (format!("wget -q -O /out {URL}"), 1),
    ] {
        let result = limited.exec(&command).await.unwrap();
        assert_eq!(result.exit_code, code, "{command}: {}", result.stderr);
        assert!(result.stderr.contains("too large"), "{}", result.stderr);
        eprintln!("HTTP cap rejection: {command}: {}", result.stderr.trim());
        assert_eq!(
            limited.fs().read_file(Path::new("/out")).await.unwrap(),
            b"sentinel"
        );
        assert_eq!(limited.exec("echo alive").await.unwrap().stdout, "alive\n");
    }

    // A small live-value budget must reject jq slurping without poisoning the
    // session.
    let mut bounded = Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_live_intermediate_bytes(256 << 10)
                .max_work_units((bytes as u64) * 256)
                .max_aggregate_input_bytes((bytes as u64) * 64),
        )
        .build();
    // Stay under the default VFS single-file cap at every scale.
    let bounded_rows = (1 << 20) / ROW.len();
    bounded
        .fs()
        .write_file(Path::new("/data"), &ROW.repeat(bounded_rows))
        .await
        .unwrap();
    let rejected = bounded.exec("jq -s length /data").await.unwrap();
    assert_ne!(rejected.exit_code, 0, "slurping must obey the live budget");
    assert!(rejected.stderr.contains("limit"), "{}", rejected.stderr);
    eprintln!("jq live-budget rejection: {}", rejected.stderr.trim());
    let recovered = bounded.exec("echo alive").await.unwrap();
    assert_eq!(recovered.stdout, "alive\n");
}

async fn regression(script: &str, data: Vec<u8>, expected: &str, factor: usize) {
    let bytes = data.len();
    let mut bash = shell(bytes, Arc::new(data.clone()));
    bash.fs()
        .write_file(Path::new("/data"), &data)
        .await
        .unwrap();
    drop(data);
    bash.exec("true").await.unwrap();
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let result = bash.exec(script).await.unwrap();
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout.trim(), expected);
    assert!(result.stderr.is_empty(), "{}", result.stderr);
    eprintln!("{script}: peak={peak} bytes, input={bytes}");
    assert!(
        peak < bytes * factor + (256 << 10),
        "{script}: peak={peak}, input={bytes}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn memory_regression_wc_bytes_does_not_decode_binary() {
    let _serial = SERIAL.lock().await;
    regression("wc -c /data", vec![0xff; 1 << 20], "1048576 /data", 1).await;
}

#[tokio::test(flavor = "current_thread")]
async fn memory_regression_jq_inputs_releases_each_tree() {
    let _serial = SERIAL.lock().await;
    let rows = (1 << 20) / ROW.len();
    regression(
        "jq -n 'reduce inputs as $r (0; . + $r.n)' /data",
        ROW.repeat(rows),
        &rows.to_string(),
        2,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn memory_regression_jq_stream_does_not_build_a_large_array() {
    let _serial = SERIAL.lock().await;
    let rows = 1 << 18;
    let data = format!("[{}0]", "0,".repeat(rows));
    regression(
        "jq --stream -n 'reduce inputs as $e (0; if ($e|length)==2 then .+1 else . end)' /data",
        data.into_bytes(),
        &(rows + 1).to_string(),
        2,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn memory_regression_curl_file_does_not_clone_the_body() {
    let _serial = SERIAL.lock().await;
    regression(
        &format!("curl -s -o /out -w '%{{http_code}}:%{{size_download}}:%{{content_type}}' {URL}"),
        vec![0xff; 1 << 20],
        "200:1048576:",
        2,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn memory_regression_wget_reserves_known_response_length() {
    let _serial = SERIAL.lock().await;
    let bytes = 1 << 20;
    let body = Arc::new(vec![0xff; bytes]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let served = body.clone();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        serve(socket, &served).await.unwrap();
    });
    let mut bash = builder(bytes)
        .network(NetworkAllowlist::new().block_private_ips(false).allow(&url))
        .build();
    bash.exec("true").await.unwrap();
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let result = bash
        .exec(&format!("wget -q -O /out {url}/length"))
        .await
        .unwrap();
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(bash.fs().read_file(Path::new("/out")).await.unwrap(), *body);
    server.await.unwrap();
    eprintln!("native wget: peak={peak}, input={bytes}");
    assert!(peak < bytes * 2 + (256 << 10), "peak={peak}, input={bytes}");
}
