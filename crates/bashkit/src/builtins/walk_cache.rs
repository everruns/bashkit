//! Bounded `FileSystem::walk` prefetch for tree walkers (`find`, `du`,
//! `ls -R`, `tree`).
//! THREAT[TM-DOS-140]: walks over millions of files hold one batch.
//!
//! Decisions:
//! - Listings come in bounded batches (`WalkOptions::max_entries`), never the
//!   whole tree: a directory missing from the cache triggers another `walk`
//!   rooted at it, so memory stays at about one batch however large the tree,
//!   and bulk backends still answer each batch in one query.
//! - Listings are removed as they are consumed. If an answer does not hold the
//!   requested directory (path spelling mismatch), bulk walking is turned off
//!   for the run and plain `read_dir` takes over, so a mismatch can never turn
//!   into one batch-sized walk per directory.
//! - Callers that skip hidden directories pass `skip_hidden`, and callers
//!   with an entry budget cap `max_entries` at what is left of it, so the
//!   default `read_dir` walk never reads a directory the caller would not.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::fs::{DirEntry, FileSystem, WalkOptions};

/// Directory listings fetched ahead in bounded `walk` batches.
pub(crate) struct WalkCache {
    listings: HashMap<PathBuf, Vec<DirEntry>>,
    enabled: bool,
    max_entries: usize,
    skip_hidden: bool,
}

impl WalkCache {
    /// `enabled = false` makes every lookup a plain `read_dir`.
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            listings: HashMap::new(),
            enabled,
            max_entries: WalkOptions::DEFAULT_MAX_ENTRIES,
            skip_hidden: false,
        }
    }

    /// Never descend into dot directories below the listed one.
    pub(crate) fn skip_hidden(mut self, skip: bool) -> Self {
        self.skip_hidden = skip;
        self
    }

    /// Cap each batch below the default (e.g. at an entry budget's remainder).
    pub(crate) fn set_max_entries(&mut self, entries: usize) {
        self.max_entries = entries.clamp(1, WalkOptions::DEFAULT_MAX_ENTRIES);
    }

    /// Stop prefetching (an action may change or cut the traversal).
    pub(crate) fn disable(&mut self) {
        self.enabled = false;
        self.listings.clear();
    }

    /// Listing of `dir`. `max_depth` counts directory levels still to list
    /// from `dir` (`None` = unlimited; `dir` itself is level 1).
    pub(crate) async fn list(
        &mut self,
        fs: &dyn FileSystem,
        dir: &Path,
        max_depth: Option<usize>,
    ) -> Result<Vec<DirEntry>> {
        if let Some(list) = self.listings.remove(dir) {
            return Ok(list);
        }
        if self.enabled && max_depth != Some(0) {
            let opts = WalkOptions::new(max_depth)
                .max_entries(self.max_entries)
                .skip_hidden(self.skip_hidden);
            if let Ok(batch) = fs.walk(dir, opts).await {
                let mut own = None;
                for listing in batch {
                    if listing.path == dir {
                        own = Some(listing.entries);
                    } else {
                        self.listings.insert(listing.path, listing.entries);
                    }
                }
                match own {
                    Some(list) => return Ok(list),
                    None => self.disable(),
                }
            }
        }
        fs.read_dir(dir).await
    }

    /// Entries currently held (for tests and assertions).
    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        self.listings.values().map(Vec::len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;

    async fn tree(dirs: usize, files: usize) -> InMemoryFs {
        let fs = InMemoryFs::new();
        for d in 0..dirs {
            for f in 0..files {
                let p = PathBuf::from(format!("/t/d{d}/f{f}"));
                fs.mkdir(p.parent().unwrap(), true).await.unwrap();
                fs.write_file(&p, b"x").await.unwrap();
            }
        }
        fs
    }

    #[tokio::test]
    async fn batches_stay_bounded_and_cover_every_directory() {
        let fs = tree(50, 20).await;
        let mut cache = WalkCache::new(true);
        cache.max_entries = 100;
        let mut seen = 0;
        let mut peak = 0;
        let mut stack = vec![PathBuf::from("/t")];
        while let Some(dir) = stack.pop() {
            let list = cache.list(&fs, &dir, None).await.unwrap();
            peak = peak.max(cache.held());
            for e in list {
                seen += 1;
                if e.metadata.file_type.is_dir() {
                    stack.push(dir.join(&e.name));
                }
            }
        }
        assert_eq!(seen, 50 + 50 * 20);
        // One batch plus at most one listing over the bound.
        assert!(peak <= 100 + 50, "peak {peak}");
        assert_eq!(cache.held(), 0);
    }

    #[tokio::test]
    async fn missing_directory_reports_read_dir_error() {
        let fs = tree(1, 1).await;
        let mut cache = WalkCache::new(true);
        assert!(cache.list(&fs, Path::new("/nope"), None).await.is_err());
    }

    #[tokio::test]
    async fn disabled_cache_holds_nothing() {
        let fs = tree(5, 5).await;
        let mut cache = WalkCache::new(false);
        let list = cache.list(&fs, Path::new("/t"), None).await.unwrap();
        assert_eq!(list.len(), 5);
        assert_eq!(cache.held(), 0);
    }
}
