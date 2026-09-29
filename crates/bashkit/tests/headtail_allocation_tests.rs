//! TM-DOS-117: a bounded `head`/`tail` line request must not allocate memory
//! proportional to the input.
//!
//! Decision: this lives in its own test binary because a `#[global_allocator]`
//! is process-wide state. Sharing a binary with unrelated tests would let their
//! allocations land in this measurement, and the counter is what gives the
//! threat a regression test at all — `headtail` takes no budget lease, so a
//! `max_live_intermediate_bytes` assertion cannot observe the offset vector.

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

/// Bytes of newline-dense input. One line per byte is the worst case for a
/// per-line offset index: 1 MiB of input implies ~1M offsets (~8 MiB).
const INPUT_LEN: usize = 1_000_000;

async fn peak_bytes_for(script: &str) -> usize {
    let mut bash = Bash::builder().build();
    bash.fs()
        .write_file(Path::new("/dense"), &vec![b'\n'; INPUT_LEN])
        .await
        .unwrap();
    // Warm up so runtime and lazy statics are not attributed to the request.
    bash.exec("head -n 1 /dense").await.unwrap();

    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let result = bash.exec(script).await.unwrap();
    assert_eq!(result.exit_code, 0, "{script} failed: {}", result.stderr);
    PEAK.load(Ordering::Relaxed).saturating_sub(before)
}

/// A request whose *output* is one line must stay within a small multiple of
/// the input it reads, not gain a machine word per line. The pre-fix
/// `Vec<usize>` index added ~8 MiB on top of the 1 MiB file, so 2x input sits
/// far below the regression and far above reading the file once.
///
/// Only selections with bounded output belong here: `head -n -1` over this
/// input legitimately emits ~1 MB, so its peak tracks output, not line count.
#[tokio::test(flavor = "current_thread")]
async fn tm_dos_117_bounded_line_request_does_not_allocate_per_line() {
    for script in ["head -n 1 /dense", "tail -n 1 /dense"] {
        let peak = peak_bytes_for(script).await;
        assert!(
            peak < INPUT_LEN * 2,
            "{script}: peak allocation {peak} B exceeded 2x input ({} B); \
             a per-line offset index is back",
            INPUT_LEN * 2
        );
        eprintln!("{script}: peak {peak} B");
    }
}
