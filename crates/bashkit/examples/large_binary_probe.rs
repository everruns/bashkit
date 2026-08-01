//! Probe: how does the snapshot object graph behave on large binary files?
//! Scratch tool, not part of the test suite.

use bashkit::{Bash, CheckoutPolicy, CommitOptions, FileSystem, FsLimits, InMemoryFs, ObjectId};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmHWM"))
        .and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse().ok()))
        .unwrap_or(0)
}

#[tokio::main]
async fn main() -> bashkit::Result<()> {
    for mb in [1usize, 8, 32, 64] {
        let limits = FsLimits {
            max_file_size: 512 * 1024 * 1024,
            max_total_bytes: 1024 * 1024 * 1024,
            ..Default::default()
        };
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::with_limits(limits));
        let bash = Bash::builder()
            .fs(fs)
            .limits(bashkit::ExecutionLimits::new().max_commands(1_000_000))
            .build();

        // Incompressible pseudo-random binary, written directly into the VFS.
        let bytes: Vec<u8> = {
            let mut s = 0x2545F4914F6CDD1Du64;
            (0..mb * 1024 * 1024)
                .map(|_| {
                    s ^= s << 13;
                    s ^= s >> 7;
                    s ^= s << 17;
                    (s >> 24) as u8
                })
                .collect()
        };
        bash.fs()
            .write_file(std::path::Path::new("/big.bin"), &bytes)
            .await?;

        let t = Instant::now();
        let packed = bash.commit(CommitOptions::new())?;
        let commit_ms = t.elapsed().as_secs_f64() * 1000.0;
        let id = packed.id();
        let objects = packed.object_count();
        let stored = packed.stored_bytes();
        let store: HashMap<ObjectId, Vec<u8>> = packed.into_objects().collect();

        let t = Instant::now();
        let mut target = Bash::builder()
            .fs(Arc::new(InMemoryFs::with_limits(FsLimits {
                max_file_size: 512 * 1024 * 1024,
                max_total_bytes: 1024 * 1024 * 1024,
                ..Default::default()
            })) as Arc<dyn FileSystem>)
            .build();
        target.checkout(id, &store, CheckoutPolicy::Force)?;
        let checkout_ms = t.elapsed().as_secs_f64() * 1000.0;

        // Edit 16 bytes in the middle, then measure the incremental commit.
        let mut edited = bytes.clone();
        let mid = edited.len() / 2;
        edited[mid..mid + 16].copy_from_slice(b"PATCHED-MIDDLE!!");
        bash.fs()
            .write_file(std::path::Path::new("/big.bin"), &edited)
            .await?;
        let t = Instant::now();
        let inc = bash.commit(CommitOptions::new().have(store.keys()))?;
        let inc_ms = t.elapsed().as_secs_f64() * 1000.0;

        println!(
            "{mb:>3} MB | objects {objects:>6} | stored {stored:>10} ({:.2}x) | commit {commit_ms:>8.1} ms | checkout {checkout_ms:>8.1} ms | edit16B -> {:>9} B in {inc_ms:>7.1} ms | peakRSS {} MB",
            stored as f64 / bytes.len() as f64,
            inc.stored_bytes(),
            rss_kb() / 1024,
        );
    }
    Ok(())
}
