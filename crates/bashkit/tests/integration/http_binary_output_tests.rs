//! HTTP downloads retain bytes through files, shell streams, and decompression.
#![cfg(feature = "http_client")]

use async_trait::async_trait;
use base64::Engine;
use bashkit::{
    Bash, FileSystem, HttpResponse, HttpTransport, HttpTransportError, HttpTransportRequest,
    InMemoryFs, NetworkAllowlist,
};
use std::{io::Write, path::Path, sync::Arc};

const URL: &str = "http://93.184.216.34/archive";
const BINARY: &[u8] = &[0x1f, 0x8b, 0x08, 0x00, 0xff, 0x80, b'\n', 0x00];

struct FixtureTransport {
    body: Vec<u8>,
    headers: Vec<(String, String)>,
    status: u16,
}

#[async_trait]
impl HttpTransport for FixtureTransport {
    async fn execute(&self, _: HttpTransportRequest) -> Result<HttpResponse, HttpTransportError> {
        Ok(HttpResponse {
            status: self.status,
            headers: self.headers.clone(),
            body: self.body.clone(),
        })
    }
}

fn shell(body: Vec<u8>, headers: Vec<(String, String)>, status: u16) -> (Bash, Arc<InMemoryFs>) {
    let fs = Arc::new(InMemoryFs::new());
    let bash = Bash::builder()
        .fs(fs.clone())
        .network(NetworkAllowlist::allow_all())
        .http_transport(Arc::new(FixtureTransport {
            body,
            headers,
            status,
        }))
        .build();
    (bash, fs)
}

#[tokio::test]
async fn downloads_and_redirection_keep_non_utf8_and_nul_bytes() {
    for command in [
        format!("curl -s -o /archive {URL}"),
        format!("curl -s {URL} > /archive"),
        format!("wget -q -O /archive {URL}"),
    ] {
        let (mut bash, fs) = shell(BINARY.to_vec(), vec![], 200);
        let result = bash.exec(&command).await.unwrap();
        assert_eq!(result.exit_code, 0, "{}", result.stderr);
        assert_eq!(
            fs.read_file(Path::new("/archive")).await.unwrap(),
            BINARY,
            "{command}"
        );
    }
}

#[tokio::test]
async fn stdout_and_pipelines_keep_binary_response_bytes() {
    for command in [format!("curl -s {URL}"), format!("wget -q -O - {URL}")] {
        let (mut bash, _) = shell(BINARY.to_vec(), vec![], 200);
        let result = bash.exec(&command).await.unwrap();
        assert_eq!(result.exit_code, 0, "{}", result.stderr);
        assert_eq!(result.stdout.as_bytes(), BINARY, "{command}");
        let piped = bash.exec(&format!("{command} | base64")).await.unwrap();
        assert_eq!(piped.exit_code, 0, "{}", piped.stderr);
        assert_eq!(
            piped.stdout.trim(),
            base64::engine::general_purpose::STANDARD.encode(BINARY)
        );
    }
}

#[tokio::test]
async fn curl_write_out_counts_original_download_bytes() {
    let (mut bash, fs) = shell(BINARY.to_vec(), vec![], 200);
    let result = bash
        .exec(&format!(
            "curl -s -o /archive -w '%{{size_download}}' {URL}"
        ))
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, BINARY.len().to_string());
    assert_eq!(fs.read_file(Path::new("/archive")).await.unwrap(), BINARY);
}

#[tokio::test]
async fn compressed_binary_downloads_preserve_decompressed_bytes() {
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(BINARY).unwrap();
    let body = gzip.finish().unwrap();
    for suffix in ["-o /archive", "> /archive"] {
        let (mut bash, fs) = shell(
            body.clone(),
            vec![("Content-Encoding".into(), "gzip".into())],
            200,
        );
        let result = bash
            .exec(&format!("curl -s --compressed {URL} {suffix}"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0, "{}", result.stderr);
        assert_eq!(fs.read_file(Path::new("/archive")).await.unwrap(), BINARY);
    }
}

#[tokio::test]
async fn failed_http_download_does_not_overwrite_existing_binary_file() {
    let (mut bash, fs) = shell(BINARY.to_vec(), vec![], 404);
    fs.write_file(Path::new("/archive"), b"existing")
        .await
        .unwrap();
    let result = bash
        .exec(&format!("curl -s -f -o /archive {URL}"))
        .await
        .unwrap();
    assert_eq!(result.exit_code, 22);
    assert_eq!(
        fs.read_file(Path::new("/archive")).await.unwrap(),
        b"existing"
    );
}

#[tokio::test]
async fn downloaded_gzip_archive_can_be_extracted() {
    let payload = b"a downloaded skill companion file\n";
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(payload).unwrap();
    let (mut bash, _) = shell(gzip.finish().unwrap(), vec![], 200);
    let result = bash
        .exec(&format!(
            "curl -s -o /package.gz {URL} && gzip -dc /package.gz"
        ))
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout.as_bytes(), payload);
}
