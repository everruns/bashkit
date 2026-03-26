//! Tests for /dev/urandom raw byte handling

use bashkit::Bash;

/// Issue #811: /dev/urandom should return raw bytes, not UTF-8 replacement chars
#[tokio::test]
async fn urandom_no_replacement_chars() {
    let mut bash = Bash::new();
    // Read 100 bytes and check output via od
    let result = bash
        .exec("head -c 100 /dev/urandom | od -A n -t x1 | tr -d ' \\n'")
        .await
        .unwrap();
    let hex = result.stdout.trim();
    // Should not contain the UTF-8 replacement character pattern efbfbd
    assert!(
        !hex.contains("efbfbd"),
        "Output should not contain UTF-8 replacement chars: {}",
        &hex[..hex.len().min(60)]
    );
}
