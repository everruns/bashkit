//! Call-counting filesystem for round-trip tests and benches.
//!
//! Decisions:
//! - Models a remote backend (database, object store, network share) where
//!   every `FileSystem` call is one round trip. Tests assert call counts;
//!   benches add a simulated per-call latency.
//! - Latency is a busy-wait, not `sleep`: tokio timers round up to 1 ms, which
//!   would hide sub-millisecond settings and skew bench numbers.
//! - The optional content index is a private map kept next to the inner
//!   filesystem (writes go to both). Searching it is not a round trip of the
//!   walked tree, the same way a server-side index answers in one query.
//! - `native_walk` makes `walk` one counted call (a prefix query) instead of
//!   the `read_dir`-per-directory default, to show what a bulk backend saves.
//! - `walk_page` caps entries per `walk` answer below the caller's
//!   `max_entries` (a backend page size), and `walk_peak_entries` records the
//!   largest answer, so tests can prove walkers hold a bounded batch.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;

use crate::error::Result;
use crate::fs::{
    DirEntry, DirListing, FileSystem, FileSystemExt, FsLimits, FsUsage, InMemoryFs, Metadata,
    SearchCapabilities, SearchCapable, SearchMatch, SearchProvider, SearchQuery, SearchResults,
    WalkOptions, walk_with_read_dir,
};
use crate::time_compat::SystemTime;

/// Path -> content map the simulated index searches.
type Index = Arc<RwLock<BTreeMap<PathBuf, Vec<u8>>>>;

/// Per-operation call counters, shared with index providers.
#[derive(Default)]
struct Counters {
    /// Simulated per-call latency, in nanoseconds.
    latency_ns: AtomicU64,
    read_file: AtomicU64,
    write: AtomicU64,
    stat: AtomicU64,
    lstat: AtomicU64,
    read_dir: AtomicU64,
    exists: AtomicU64,
    read_link: AtomicU64,
    walk: AtomicU64,
    search: AtomicU64,
    other: AtomicU64,
    /// Most entries returned by one `walk` (a peak, not a call count).
    walk_peak_entries: AtomicU64,
}

/// Snapshot of the call counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CallCounts {
    /// `read_file` calls.
    pub read_file: u64,
    /// `write_file`, `append_file`, `mkdir`, `remove`, `rename`, `copy`,
    /// `symlink`, `chmod`, `set_modified_time` calls.
    pub write: u64,
    /// `stat` calls.
    pub stat: u64,
    /// `lstat` calls.
    pub lstat: u64,
    /// `read_dir` calls.
    pub read_dir: u64,
    /// `exists` calls.
    pub exists: u64,
    /// `read_link` calls.
    pub read_link: u64,
    /// Native `walk` calls (only counted with `native_walk`).
    pub walk: u64,
    /// Index `search` calls.
    pub search: u64,
    /// Anything else.
    pub other: u64,
    /// Most entries any single `walk` answer held (not a round trip).
    pub walk_peak_entries: u64,
}

impl CallCounts {
    /// Every round trip.
    pub fn total(&self) -> u64 {
        self.read_file
            + self.write
            + self.stat
            + self.lstat
            + self.read_dir
            + self.exists
            + self.read_link
            + self.walk
            + self.search
            + self.other
    }

    /// `stat` + `lstat` + `exists`: per-path metadata lookups.
    pub fn metadata(&self) -> u64 {
        self.stat + self.lstat + self.exists
    }
}

impl fmt::Display for CallCounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "total={} read_file={} read_dir={} stat={} lstat={} exists={} read_link={} walk={} search={} write={} other={}",
            self.total(),
            self.read_file,
            self.read_dir,
            self.stat,
            self.lstat,
            self.exists,
            self.read_link,
            self.walk,
            self.search,
            self.write,
            self.other
        )
    }
}

impl Counters {
    fn hit(&self, counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
        let ns = self.latency_ns.load(Ordering::Relaxed);
        if ns > 0 {
            let latency = Duration::from_nanos(ns);
            let start = crate::time_compat::Instant::now();
            while start.elapsed() < latency {
                std::hint::spin_loop();
            }
        }
    }
}

/// A [`FileSystem`] over [`InMemoryFs`] that counts every call.
pub struct CountingFs {
    inner: InMemoryFs,
    counters: Arc<Counters>,
    index: Option<Index>,
    native_walk: bool,
    walk_page: Option<usize>,
}

impl CountingFs {
    /// Plain counting filesystem: no index, no bulk walk, no latency.
    pub fn new() -> Self {
        Self {
            inner: InMemoryFs::new(),
            counters: Arc::new(Counters::default()),
            index: None,
            native_walk: false,
            walk_page: None,
        }
    }

    /// Answer content searches from an index (`SearchCapable`).
    pub fn with_index(mut self) -> Self {
        self.index = Some(Arc::new(RwLock::new(BTreeMap::new())));
        self
    }

    /// Answer `walk` in one counted call instead of one `read_dir` per directory.
    pub fn with_native_walk(mut self) -> Self {
        self.native_walk = true;
        self
    }

    /// Return at most `entries` entries per `walk` answer (a backend page
    /// size), even when the caller allows more.
    pub fn with_walk_page(mut self, entries: usize) -> Self {
        self.walk_page = Some(entries);
        self
    }

    /// Busy-wait this long on every call.
    pub fn with_latency(self, latency: Duration) -> Self {
        self.set_latency(latency);
        self
    }

    /// Change the simulated per-call latency (e.g. after seeding).
    pub fn set_latency(&self, latency: Duration) {
        let ns = u64::try_from(latency.as_nanos()).unwrap_or(u64::MAX);
        self.counters.latency_ns.store(ns, Ordering::Relaxed);
    }

    /// Use these limits for the inner in-memory filesystem.
    pub fn with_limits(mut self, limits: FsLimits) -> Self {
        self.inner = InMemoryFs::with_limits(limits);
        self
    }

    /// Current counters.
    pub fn counts(&self) -> CallCounts {
        let c = &self.counters;
        let get = |a: &AtomicU64| a.load(Ordering::Relaxed);
        CallCounts {
            read_file: get(&c.read_file),
            write: get(&c.write),
            stat: get(&c.stat),
            lstat: get(&c.lstat),
            read_dir: get(&c.read_dir),
            exists: get(&c.exists),
            read_link: get(&c.read_link),
            walk: get(&c.walk),
            search: get(&c.search),
            other: get(&c.other),
            walk_peak_entries: get(&c.walk_peak_entries),
        }
    }

    /// Zero every counter.
    pub fn reset(&self) {
        let c = &self.counters;
        for a in [
            &c.read_file,
            &c.write,
            &c.stat,
            &c.lstat,
            &c.read_dir,
            &c.exists,
            &c.read_link,
            &c.walk,
            &c.search,
            &c.other,
            &c.walk_peak_entries,
        ] {
            a.store(0, Ordering::Relaxed);
        }
    }

    fn hit(&self, counter: &AtomicU64) {
        self.counters.hit(counter);
    }

    fn index_put(&self, path: &Path, content: Option<&[u8]>, append: bool) {
        let Some(index) = &self.index else { return };
        let path = crate::fs::normalize_path(path);
        let mut map = index.write().unwrap_or_else(|e| e.into_inner());
        match content {
            Some(bytes) if append => map.entry(path).or_default().extend_from_slice(bytes),
            Some(bytes) => {
                map.insert(path, bytes.to_vec());
            }
            None => {
                let prefix = path.clone();
                map.retain(|p, _| !p.starts_with(&prefix));
            }
        }
    }
}

impl Default for CountingFs {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FileSystemExt for CountingFs {
    fn usage(&self) -> FsUsage {
        self.inner.usage()
    }
    fn limits(&self) -> FsLimits {
        self.inner.limits()
    }
    fn backend_kind(&self) -> &'static str {
        "counting"
    }
}

#[async_trait]
impl FileSystem for CountingFs {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.hit(&self.counters.read_file);
        self.inner.read_file(path).await
    }
    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.hit(&self.counters.write);
        self.inner.write_file(path, content).await?;
        self.index_put(path, Some(content), false);
        Ok(())
    }
    async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        self.hit(&self.counters.write);
        self.inner.append_file(path, content).await?;
        self.index_put(path, Some(content), true);
        Ok(())
    }
    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        self.hit(&self.counters.write);
        self.inner.mkdir(path, recursive).await
    }
    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        self.hit(&self.counters.write);
        self.inner.remove(path, recursive).await?;
        self.index_put(path, None, false);
        Ok(())
    }
    async fn stat(&self, path: &Path) -> Result<Metadata> {
        self.hit(&self.counters.stat);
        self.inner.stat(path).await
    }
    async fn lstat(&self, path: &Path) -> Result<Metadata> {
        self.hit(&self.counters.lstat);
        self.inner.lstat(path).await
    }
    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        self.hit(&self.counters.read_dir);
        self.inner.read_dir(path).await
    }
    async fn exists(&self, path: &Path) -> Result<bool> {
        self.hit(&self.counters.exists);
        self.inner.exists(path).await
    }
    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        self.hit(&self.counters.write);
        self.inner.rename(from, to).await?;
        if let Some(index) = &self.index {
            let mut map = index.write().unwrap_or_else(|e| e.into_inner());
            let from = crate::fs::normalize_path(from);
            let to = crate::fs::normalize_path(to);
            let moved: Vec<PathBuf> = map
                .keys()
                .filter(|p| p.starts_with(&from))
                .cloned()
                .collect();
            for old in moved {
                if let (Some(content), Ok(rest)) = (map.remove(&old), old.strip_prefix(&from)) {
                    map.insert(crate::fs::vfs_join(&to, rest), content);
                }
            }
        }
        Ok(())
    }
    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        self.hit(&self.counters.write);
        self.inner.copy(from, to).await?;
        if let Some(index) = &self.index {
            let mut map = index.write().unwrap_or_else(|e| e.into_inner());
            if let Some(content) = map.get(&crate::fs::normalize_path(from)).cloned() {
                map.insert(crate::fs::normalize_path(to), content);
            }
        }
        Ok(())
    }
    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        self.hit(&self.counters.write);
        self.inner.symlink(target, link).await
    }
    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        self.hit(&self.counters.read_link);
        self.inner.read_link(path).await
    }
    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        self.hit(&self.counters.write);
        self.inner.chmod(path, mode).await
    }
    async fn set_modified_time(&self, path: &Path, time: SystemTime) -> Result<()> {
        self.hit(&self.counters.write);
        self.inner.set_modified_time(path, time).await
    }
    async fn walk(&self, root: &Path, mut opts: WalkOptions) -> Result<Vec<DirListing>> {
        if let Some(page) = self.walk_page {
            opts.max_entries = opts.max_entries.min(page);
        }
        let listings = if self.native_walk {
            self.hit(&self.counters.walk);
            walk_with_read_dir(&self.inner, root, opts).await?
        } else {
            walk_with_read_dir(self, root, opts).await?
        };
        let held: usize = listings.iter().map(|l| l.entries.len()).sum();
        self.counters
            .walk_peak_entries
            .fetch_max(u64::try_from(held).unwrap_or(u64::MAX), Ordering::Relaxed);
        Ok(listings)
    }
    fn as_search_capable(&self) -> Option<&dyn SearchCapable> {
        self.index.as_ref().map(|_| self as &dyn SearchCapable)
    }
}

impl SearchCapable for CountingFs {
    fn search_provider(&self, _path: &Path) -> Option<Box<dyn SearchProvider>> {
        let index = self.index.clone()?;
        Some(Box::new(IndexProvider {
            index,
            counters: Arc::clone(&self.counters),
        }))
    }
}

/// Searches the index map; counts against the owning `CountingFs`.
struct IndexProvider {
    index: Index,
    counters: Arc<Counters>,
}

impl SearchProvider for IndexProvider {
    fn search(&self, query: &SearchQuery) -> Result<SearchResults> {
        self.counters.hit(&self.counters.search);
        let pattern = if query.is_regex {
            query.pattern.clone()
        } else {
            regex::escape(&query.pattern)
        };
        let re = regex::RegexBuilder::new(&pattern)
            .case_insensitive(query.case_insensitive)
            .build()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let root = crate::fs::normalize_path(&query.root);
        let map = self.index.read().unwrap_or_else(|e| e.into_inner());
        let mut matches = Vec::new();
        for (path, content) in map.range(root.clone()..) {
            if !path.starts_with(&root) {
                break;
            }
            let text = String::from_utf8_lossy(content);
            for (i, line) in text.lines().enumerate() {
                if re.is_match(line) {
                    matches.push(SearchMatch {
                        path: path.clone(),
                        line_number: i + 1,
                        line_content: line.to_string(),
                    });
                    if query.max_results.is_some_and(|m| matches.len() >= m) {
                        return Ok(SearchResults {
                            matches,
                            truncated: true,
                        });
                    }
                }
            }
        }
        Ok(SearchResults {
            matches,
            truncated: false,
        })
    }

    fn capabilities(&self) -> SearchCapabilities {
        SearchCapabilities {
            regex: true,
            glob_filter: false,
            content_search: true,
            filename_search: false,
        }
    }
}
