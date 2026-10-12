//! Tar file-list allocation regression. The allocator is process-wide,
//! so this test needs its own binary to exclude unrelated test allocations.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use bashkit::{Bash, Error, ExecOptions, ExecutionLimits, LimitExceeded, StreamData};
use std::path::Path;

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
async fn tar_files_from_rejects_before_per_line_allocation_amplification() {
    for options in [
        "-T /list",
        "--files-from=/list",
        "--files-from /list",
        "-T /list -T /list",
        "-T -",
    ] {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_live_intermediate_bytes(65_536))
            .build();
        bash.fs()
            .write_file(Path::new("/list"), "x\n".repeat(20_000).as_bytes())
            .await
            .unwrap();
        bash.exec("true").await.unwrap();
        let stdin = ExecOptions::new().stdin(StreamData::from("x\n".repeat(20_000)));
        let before = LIVE.load(Ordering::Relaxed);
        PEAK.store(before, Ordering::Relaxed);
        let result = bash
            .exec_with_options(&format!("tar -cf /out.tar {options}"), stdin)
            .await;
        let peak = PEAK.load(Ordering::Relaxed).saturating_sub(before);
        assert!(
            matches!(
                result,
                Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
            ),
            "{result:?}"
        );
        eprintln!("{options}: peak heap growth {peak} bytes");
        // Includes interpreter overhead and the VFS read; the old collector
        // retained over 800 KiB before its first live-budget rejection.
        assert!(peak < 256 * 1024, "unbudgeted list amplification: {peak}");
        assert!(!bash.fs().exists(Path::new("/out.tar")).await.unwrap());
        assert_eq!(
            bash.exec("echo recovered").await.unwrap().stdout,
            "recovered\n"
        );
    }
}
