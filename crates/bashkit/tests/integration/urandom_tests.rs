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

/// Issue #811: head -c N /dev/urandom should return exactly N chars
/// (each original byte maps to one char in the Latin-1 model)
#[tokio::test]
async fn urandom_head_char_count() {
    let mut bash = Bash::new();
    for n in [1, 4, 8, 16, 32] {
        let result = bash
            .exec(&format!("head -c {n} /dev/urandom | wc -m"))
            .await
            .unwrap();
        let count: usize = result.stdout.trim().parse().unwrap_or(0);
        assert_eq!(
            count, n,
            "head -c {n} /dev/urandom | wc -m should produce exactly {n} chars"
        );
    }
}

/// Issue #811: tr -dc 'a-z0-9' < /dev/urandom | head -c 8 should produce 8 alphanumeric chars
#[tokio::test]
async fn urandom_tr_filter_alphanumeric() {
    let mut bash = Bash::new();
    let result = bash
        .exec("LC_ALL=C tr -dc 'a-z0-9' < /dev/urandom | head -c 8")
        .await
        .unwrap();
    let output = result.stdout.trim();
    assert_eq!(
        output.len(),
        8,
        "Should produce exactly 8 chars, got {}: {:?}",
        output.len(),
        output
    );
    assert!(
        output
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
        "All chars should be a-z0-9, got: {:?}",
        output
    );
}
