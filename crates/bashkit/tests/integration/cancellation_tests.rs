// Integration tests for cancellation support (issue #541)

use bashkit::Bash;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn cancel_aborts_execution() {
    let mut bash = Bash::new();
    let token = bash.cancellation_token();

    // Pre-set cancellation flag
    token.store(true, Ordering::Relaxed);

    let result = bash.exec("echo hello").await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.to_string(), "execution cancelled");
}

#[tokio::test]
async fn cancel_resets_manually() {
    let mut bash = Bash::new();
    let token = bash.cancellation_token();

    // Cancel first execution
    token.store(true, Ordering::Relaxed);
    let result = bash.exec("echo first").await;
    assert!(result.is_err());

    // Reset flag and verify next execution succeeds
    token.store(false, Ordering::Relaxed);
    let result = bash.exec("echo second").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().stdout.trim(), "second");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_mid_execution() {
    let mut bash = Bash::new();
    let token = bash.cancellation_token();

    // Spawn a task that cancels after a short delay
    let cancel_token = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        cancel_token.store(true, Ordering::Relaxed);
    });

    // Run a long script — should be cancelled before completion
    let result = bash.exec("for i in $(seq 1 10000); do echo $i; done").await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), "execution cancelled");

    // Clean up
    token.store(false, Ordering::Relaxed);
}

#[tokio::test]
async fn uncancelled_execution_succeeds() {
    let mut bash = Bash::new();
    let _token = bash.cancellation_token();

    // Getting the token without setting it should not affect execution
    let result = bash.exec("echo works").await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().stdout.trim(), "works");
}

#[tokio::test]
async fn cancellation_token_is_shared() {
    let bash = Bash::new();
    let token1 = bash.cancellation_token();
    let token2 = bash.cancellation_token();

    // Both tokens point to the same AtomicBool
    token1.store(true, Ordering::Relaxed);
    assert!(token2.load(Ordering::Relaxed));
}
