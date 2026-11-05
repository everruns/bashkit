//! SPIKE (Monty 1.4): per-VM memory metering keeps concurrent tenants apart.
//!
//! Monty 1.x's default memory counter is process-wide; with it two tenants
//! that each fit their limit would fail together. `MeteredAllocator` (installed
//! in `main.rs`) charges each VM only for its own allocations.

use bashkit::{Bash, PythonLimits};
use std::time::Duration;

fn tenant(limit_mib: usize) -> Bash {
    Bash::builder()
        .python_with_limits(
            PythonLimits::default()
                .max_memory(limit_mib * 1024 * 1024)
                .max_duration(Duration::from_secs(20)),
        )
        .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
        .build()
}

/// Holds `mib` MiB live while spinning, so concurrent runs overlap.
fn hold(mib: usize) -> String {
    format!(
        "python3 -c \"x = 'a' * ({mib} * 1024 * 1024)\nn = 0\nfor i in range(3000000):\n    n += 1\nprint(len(x) // 1048576, n)\""
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_tenants_are_charged_separately() {
    // 4 tenants x 20 MiB live at once = 80 MiB process-wide, 32 MiB limit each.
    let mut handles = Vec::new();
    for _ in 0..4 {
        handles.push(tokio::spawn(async move {
            let mut bash = tenant(32);
            bash.exec(&hold(20)).await.unwrap()
        }));
    }
    for h in handles {
        let r = h.await.unwrap();
        assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
        assert_eq!(r.stdout, "20 3000000\n");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_tenant_over_limit_does_not_hurt_neighbours() {
    let greedy = tokio::spawn(async move {
        let mut bash = tenant(32);
        bash.exec(&hold(64)).await.unwrap()
    });
    let mut small = Vec::new();
    for _ in 0..3 {
        small.push(tokio::spawn(async move {
            let mut bash = tenant(32);
            bash.exec(&hold(8)).await.unwrap()
        }));
    }
    let r = greedy.await.unwrap();
    assert_ne!(r.exit_code, 0);
    assert!(r.stderr.contains("MemoryError"), "stderr: {}", r.stderr);
    for h in small {
        let r = h.await.unwrap();
        assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    }
}

#[tokio::test]
async fn memory_limit_trips_for_many_small_objects() {
    // Growth through many small allocations (not one up-front request):
    // only the per-VM probe can see this.
    let mut bash = tenant(8);
    let r = bash
        .exec("python3 -c \"x = [str(i) for i in range(2000000)]\"")
        .await
        .unwrap();
    assert_ne!(r.exit_code, 0);
    assert!(r.stderr.contains("MemoryError"), "stderr: {}", r.stderr);
}

#[tokio::test]
async fn str_join_respects_memory_limit() {
    // On 1.4 without metering this built a 1 GB string under a 64 MB limit.
    let mut bash = tenant(64);
    let r = bash
        .exec("python3 -c \"s = 'x' * 1000\nprint(len((s * 1000).join([s] * 1000)))\"")
        .await
        .unwrap();
    assert_ne!(r.exit_code, 0, "stdout: {}", r.stdout);
}
