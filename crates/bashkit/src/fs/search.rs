// SearchCapable is a separate opt-in trait — FileSystem unchanged.
// Builtins (grep) check via downcast at runtime, fall back to linear scan.

//! Optional indexed search support for filesystem implementations.
//!
//! The [`SearchCapable`] trait allows filesystem implementations to provide
//! optimized content and filename search. Commands like `grep` check for this
//! at runtime via [`Any`] downcast and fall back to linear scanning when
//! unavailable.
//!
//! # Implementing SearchCapable
//!
//! ```rust,ignore
//! use bashkit::fs::{FileSystem, SearchCapable, SearchProvider, SearchQuery, SearchResult};
//!
//! struct IndexedFs { /* ... */ }
//!
//! impl SearchCapable for IndexedFs {
//!     fn search_provider(&self, path: &Path) -> Option<Box<dyn SearchProvider>> {
//!         Some(Box::new(MyProvider::new(path)))
//!     }
//! }
//! ```

use std::path::{Path, PathBuf};

use crate::error::Result;

/// Optional trait for filesystems that support indexed search.
///
/// Builtins check for this via downcast and use it when available.
/// Not implementing this trait has zero cost — builtins fall back
/// to linear file enumeration.
pub trait SearchCapable: super::FileSystem {
    /// Returns a search provider scoped to the given path.
    /// Returns `None` if no index covers this path.
    fn search_provider(&self, path: &Path) -> Option<Box<dyn SearchProvider>>;
}

/// Provides content and filename search within a filesystem scope.
pub trait SearchProvider: Send + Sync {
    /// Execute a content search query.
    fn search(&self, query: &SearchQuery) -> Result<SearchResults>;

    /// Report what this provider can do.
    fn capabilities(&self) -> SearchCapabilities;
}

/// Parameters for a search query.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// Pattern to search for.
    pub pattern: String,
    /// Whether the pattern is a regex (vs literal string).
    pub is_regex: bool,
    /// Case-insensitive matching.
    pub case_insensitive: bool,
    /// Root path to scope the search.
    pub root: PathBuf,
    /// Optional glob filter for filenames (e.g., `"*.rs"`).
    pub glob_filter: Option<String>,
    /// Maximum number of results to return.
    pub max_results: Option<usize>,
}

/// Results from a search query.
#[derive(Debug, Clone, Default)]
pub struct SearchResults {
    /// Matching lines.
    pub matches: Vec<SearchMatch>,
    /// Whether results were truncated due to `max_results`.
    pub truncated: bool,
}

/// A single match from a search.
#[derive(Debug, Clone)]
pub struct SearchMatch {
    /// Path to the file containing the match.
    pub path: PathBuf,
    /// 1-based line number within the file.
    pub line_number: usize,
    /// Content of the matching line (without trailing newline).
    pub line_content: String,
}

/// Describes what a search provider supports.
#[derive(Debug, Clone, Copy, Default)]
pub struct SearchCapabilities {
    /// Supports regex patterns.
    pub regex: bool,
    /// Supports glob-based file filtering.
    pub glob_filter: bool,
    /// Supports content (full-text) search.
    pub content_search: bool,
    /// Supports filename search.
    pub filename_search: bool,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFs};

    /// Mock searchable filesystem for testing.
    struct MockSearchFs {
        inner: InMemoryFs,
    }

    impl MockSearchFs {
        fn new() -> Self {
            Self {
                inner: InMemoryFs::new(),
            }
        }
    }

    #[async_trait::async_trait]
    impl FileSystem for MockSearchFs {
        async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
            self.inner.read_file(path).await
        }
        async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
            self.inner.write_file(path, content).await
        }
        async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
            self.inner.append_file(path, content).await
        }
        async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
            self.inner.mkdir(path, recursive).await
        }
        async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
            self.inner.remove(path, recursive).await
        }
        async fn stat(&self, path: &Path) -> Result<crate::fs::Metadata> {
            self.inner.stat(path).await
        }
        async fn read_dir(&self, path: &Path) -> Result<Vec<crate::fs::DirEntry>> {
            self.inner.read_dir(path).await
        }
        async fn exists(&self, path: &Path) -> Result<bool> {
            self.inner.exists(path).await
        }
        async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
            self.inner.rename(from, to).await
        }
        async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
            self.inner.copy(from, to).await
        }
        async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
            self.inner.symlink(target, link).await
        }
        async fn read_link(&self, path: &Path) -> Result<std::path::PathBuf> {
            self.inner.read_link(path).await
        }
        async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
            self.inner.chmod(path, mode).await
        }
        fn as_search_capable(&self) -> Option<&dyn SearchCapable> {
            Some(self)
        }
    }

    struct MockProvider {
        results: Vec<SearchMatch>,
    }

    impl SearchProvider for MockProvider {
        fn search(&self, _query: &SearchQuery) -> Result<SearchResults> {
            Ok(SearchResults {
                matches: self.results.clone(),
                truncated: false,
            })
        }
        fn capabilities(&self) -> SearchCapabilities {
            SearchCapabilities {
                regex: true,
                glob_filter: true,
                content_search: true,
                filename_search: false,
            }
        }
    }

    impl SearchCapable for MockSearchFs {
        fn search_provider(&self, _path: &Path) -> Option<Box<dyn SearchProvider>> {
            Some(Box::new(MockProvider {
                results: vec![SearchMatch {
                    path: PathBuf::from("/test.txt"),
                    line_number: 1,
                    line_content: "hello world".to_string(),
                }],
            }))
        }
    }

    #[test]
    fn search_query_defaults() {
        let q = SearchQuery {
            pattern: "test".into(),
            is_regex: false,
            case_insensitive: false,
            root: PathBuf::from("/"),
            glob_filter: None,
            max_results: None,
        };
        assert_eq!(q.pattern, "test");
        assert!(!q.is_regex);
    }

    #[test]
    fn search_capabilities_default() {
        let c = SearchCapabilities::default();
        assert!(!c.regex);
        assert!(!c.glob_filter);
        assert!(!c.content_search);
        assert!(!c.filename_search);
    }

    #[test]
    fn mock_provider_returns_results() {
        let provider = MockProvider {
            results: vec![SearchMatch {
                path: PathBuf::from("/a.txt"),
                line_number: 5,
                line_content: "found it".into(),
            }],
        };
        let r = provider
            .search(&SearchQuery {
                pattern: "found".into(),
                is_regex: false,
                case_insensitive: false,
                root: PathBuf::from("/"),
                glob_filter: None,
                max_results: None,
            })
            .unwrap();
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].line_number, 5);
        assert!(!r.truncated);
    }

    #[test]
    fn mock_searchable_fs_provides_search() {
        let fs = MockSearchFs::new();
        let provider = fs.search_provider(Path::new("/")).unwrap();
        assert!(provider.capabilities().content_search);
        let r = provider
            .search(&SearchQuery {
                pattern: "hello".into(),
                is_regex: false,
                case_insensitive: false,
                root: PathBuf::from("/"),
                glob_filter: None,
                max_results: None,
            })
            .unwrap();
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].line_content, "hello world");
    }

    #[test]
    fn as_search_capable_returns_provider() {
        let fs = MockSearchFs::new();
        // MockSearchFs implements SearchCapable, so as_search_capable returns Some
        let sc = fs.as_search_capable().unwrap();
        let provider = sc.search_provider(Path::new("/")).unwrap();
        assert!(provider.capabilities().content_search);
    }

    #[test]
    fn non_searchable_fs_returns_none() {
        let fs = InMemoryFs::new();
        // InMemoryFs does NOT implement SearchCapable — returns None
        assert!(fs.as_search_capable().is_none());
    }

    #[test]
    fn search_results_default_is_empty() {
        let r = SearchResults::default();
        assert!(r.matches.is_empty());
        assert!(!r.truncated);
    }

    #[test]
    fn search_match_debug() {
        let m = SearchMatch {
            path: PathBuf::from("/test.txt"),
            line_number: 42,
            line_content: "hello".into(),
        };
        let dbg = format!("{:?}", m);
        assert!(dbg.contains("test.txt"));
        assert!(dbg.contains("42"));
    }
}
