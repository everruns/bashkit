//! Pagination peak allocation is measured in its own binary because the
//! counting allocator is process-wide; unrelated test allocations must not
//! contaminate the producer bound.
use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use bashkit::Bash;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// Tracks live heap bytes and their high-water mark.
struct Counting;

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
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[tokio::test(flavor = "current_thread")]
async fn pr_repeated_header_peak_is_bounded_by_live_budget() {
    const BUDGET: usize = 131_072;
    let mut bash = Bash::builder()
        .limits(bashkit::ExecutionLimits::new().max_live_intermediate_bytes(BUDGET as u64))
        .build();
    bash.fs()
        .write_file(Path::new("/lines"), &vec![b'\n'; 256])
        .await
        .unwrap();
    bash.exec("echo warm").await.unwrap();
    let script = format!("pr -l 11 -D DATE -h '{}' /lines", "H".repeat(1024));
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let result = bash.exec(&script).await;
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(before);
    // Include transient realloc overlap and dispatch/parser storage outside
    // pr's owned buffers. The old repeated-header String exceeded this bound.
    assert!(peak < BUDGET * 3, "peak {peak} exceeds 3x live budget");
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("live intermediate")
    );
    eprintln!("pr: peak {peak} B with {BUDGET} B live budget");
    assert_eq!(
        bash.exec("echo recovered").await.unwrap().stdout,
        "recovered\n"
    );

    // Empty files produce no pages. Their date headers must not be eagerly
    // duplicated while retaining the inputs.
    let mut bash = Bash::builder()
        .limits(bashkit::ExecutionLimits::new().max_live_intermediate_bytes(BUDGET as u64))
        .build();
    bash.fs()
        .write_file(Path::new("/empty"), b"")
        .await
        .unwrap();
    bash.exec("echo warm").await.unwrap();
    let script = format!("pr -D '{}' {}", "D".repeat(4096), "/empty ".repeat(256));
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let result = bash.exec(&script).await;
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(before);
    assert!(peak < BUDGET * 3, "date peak {peak} exceeds 3x live budget");
    assert!(result.unwrap().stdout.is_empty());
    eprintln!("pr empty files: peak {peak} B with {BUDGET} B live budget");
}
