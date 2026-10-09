//! CDPATH allocation amplification regression. The allocator is process-wide,
//! so this test needs its own binary to exclude unrelated test allocations.

use std::alloc::{GlobalAlloc, Layout, System};
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
async fn cdpath_peak_memory_does_not_multiply_target_by_entry_count() {
    let mut bash = Bash::builder().env("CDPATH", ":".repeat(1_024)).build();
    bash.exec("pwd").await.unwrap();
    let script = format!("cd {}", "x".repeat(1_024));
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let result = bash.exec(&script).await.unwrap();
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(before);
    assert_eq!(result.exit_code, 1);
    eprintln!("CDPATH peak heap growth: {peak} bytes");
    // The eager collector retained >1 MiB. This safe input allows ample
    // interpreter overhead while rejecting candidate-count amplification.
    assert!(peak < 256 * 1_024, "CDPATH retained {peak} heap bytes");
}
