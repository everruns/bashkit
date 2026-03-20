// SearchCapable is a separate opt-in trait — FileSystem unchanged.
// Builtins (grep) check via as_search_capable() at runtime, fall back to linear scan.

//! Optional indexed search support for filesystem implementations.
//!
//! The [`SearchCapable`] trait allows filesystem implementations to provide
//! optimized content and filename search. Commands like `grep` check for this
//! at runtime via [`FileSystem::as_search_capable`] and fall back to linear
//! scanning when unavailable.
//!
//! # Implementing SearchCapable
//!
//! ```rust
//! use bashkit::{SearchCapable, SearchProvider, SearchQuery, SearchResults,
//!     SearchCapabilities, SearchMatch};
//! use bashkit::{async_trait, FileSystem, FileSystemExt, InMemoryFs, Result};
//! use std::path::{Path, PathBuf};
//!
//! struct IndexedFs {
//!     inner: InMemoryFs,
//! }
//!
//! impl IndexedFs {
//!     fn new() -> Self {
//!         Self { inner: InMemoryFs::new() }
//!     }
//! }
//!
//! #[async_trait]
//! impl FileSystemExt for IndexedFs {}
//!
//! #[async_trait]
//! impl FileSystem for IndexedFs {
//!     async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
//!         self.inner.read_file(path).await
//!     }
//!     async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
//!         self.inner.write_file(path, content).await
//!     }
//!     async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
//!         self.inner.append_file(path, content).await
//!     }
//!     async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
//!         self.inner.mkdir(path, recursive).await
//!     }
//!     async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
//!         self.inner.remove(path, recursive).await
//!     }
//!     async fn stat(&self, path: &Path) -> Result<bashkit::Metadata> {
//!         self.inner.stat(path).await
//!     }
//!     async fn read_dir(&self, path: &Path) -> Result<Vec<bashkit::DirEntry>> {
//!         self.inner.read_dir(path).await
//!     }
//!     async fn exists(&self, path: &Path) -> Result<bool> {
//!         self.inner.exists(path).await
//!     }
//!     async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
//!         self.inner.rename(from, to).await
//!     }
//!     async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
//!         self.inner.copy(from, to).await
//!     }
//!     async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
//!         self.inner.symlink(target, link).await
//!     }
//!     async fn read_link(&self, path: &Path) -> Result<PathBuf> {
//!         self.inner.read_link(path).await
//!     }
//!     async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
//!         self.inner.chmod(path, mode).await
//!     }
//!     fn as_search_capable(&self) -> Option<&dyn SearchCapable> {
//!         Some(self)
//!     }
//! }
//!
//! struct MyProvider;
//!
//! impl SearchProvider for MyProvider {
//!     fn search(&self, _query: &SearchQuery) -> Result<SearchResults> {
//!         Ok(SearchResults::default())
//!     }
//!     fn capabilities(&self) -> SearchCapabilities {
//!         SearchCapabilities {
//!             regex: true,
//!             glob_filter: true,
//!             content_search: true,
//!             filename_search: false,
//!         }
//!     }
//! }
//!
//! impl SearchCapable for IndexedFs {
//!     fn search_provider(&self, _path: &Path) -> Option<Box<dyn SearchProvider>> {
//!         Some(Box::new(MyProvider))
//!     }
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<()> {
//! let fs = std::sync::Arc::new(IndexedFs::new());
//! let mut bash = bashkit::Bash::builder().fs(fs.clone()).build();
//!
//! // The grep builtin will check as_search_capable() and use indexed search
//! // when available, falling back to linear scan otherwise.
//! let sc = fs.as_search_capable().unwrap();
//! let provider = sc.search_provider(Path::new("/")).unwrap();
//! assert!(provider.capabilities().content_search);
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};

use crate::error::Result;

/// Optional trait for filesystems that support indexed search.
///
/// Builtins check for this via [`FileSystem::as_search_capable`](super::FileSystem::as_search_capable)
/// and use it when available. Not implementing this trait has zero cost —
/// builtins fall back to linear file enumeration.
///
/// `SearchCapable` is a supertrait of [`FileSystem`](super::FileSystem),
/// meaning any type implementing `SearchCapable` must also implement
/// `FileSystem`.
pub trait SearchCapable: super::FileSystem {
    /// Returns a search provider scoped to the given path.
    /// Returns `None` if no index covers this path.
    fn search_provider(&self, path: &Path) -> Option<Box<dyn SearchProvider>>;
}

/// Provides content and filename search within a filesystem scope.
///
/// Implementations are returned by [`SearchCapable::search_provider`] and
/// execute queries against an index or other optimized data structure.
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
    use crate::fs::{FileSystem, FileSystemExt, InMemoryFs};

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
    impl FileSystemExt for MockSearchFs {}

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
        let sc = fs.as_search_capable().unwrap();
        let provider = sc.search_provider(Path::new("/")).unwrap();
        assert!(provider.capabilities().content_search);
    }

    #[test]
    fn non_searchable_fs_returns_none() {
        let fs = InMemoryFs::new();
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

    // --- Additional edge-case tests ---

    #[test]
    fn search_query_with_all_options() {
        let q = SearchQuery {
            pattern: r"\bfoo\b".into(),
            is_regex: true,
            case_insensitive: true,
            root: PathBuf::from("/src"),
            glob_filter: Some("*.rs".into()),
            max_results: Some(100),
        };
        assert!(q.is_regex);
        assert!(q.case_insensitive);
        assert_eq!(q.root, PathBuf::from("/src"));
        assert_eq!(q.glob_filter.as_deref(), Some("*.rs"));
        assert_eq!(q.max_results, Some(100));
    }

    #[test]
    fn search_capabilities_all_enabled() {
        let c = SearchCapabilities {
            regex: true,
            glob_filter: true,
            content_search: true,
            filename_search: true,
        };
        assert!(c.regex);
        assert!(c.glob_filter);
        assert!(c.content_search);
        assert!(c.filename_search);
    }

    #[test]
    fn search_results_truncated() {
        let r = SearchResults {
            matches: vec![SearchMatch {
                path: PathBuf::from("/a.txt"),
                line_number: 1,
                line_content: "hit".into(),
            }],
            truncated: true,
        };
        assert!(r.truncated);
        assert_eq!(r.matches.len(), 1);
    }

    #[test]
    fn search_match_clone() {
        let m = SearchMatch {
            path: PathBuf::from("/b.txt"),
            line_number: 10,
            line_content: "cloned".into(),
        };
        let c = m.clone();
        assert_eq!(c.path, m.path);
        assert_eq!(c.line_number, m.line_number);
        assert_eq!(c.line_content, m.line_content);
    }

    #[test]
    fn search_results_clone() {
        let r = SearchResults {
            matches: vec![SearchMatch {
                path: PathBuf::from("/c.txt"),
                line_number: 3,
                line_content: "data".into(),
            }],
            truncated: false,
        };
        let c = r.clone();
        assert_eq!(c.matches.len(), 1);
        assert_eq!(c.matches[0].line_content, "data");
    }

    #[test]
    fn search_provider_no_content_search() {
        struct LimitedProvider;
        impl SearchProvider for LimitedProvider {
            fn search(&self, _query: &SearchQuery) -> Result<SearchResults> {
                Ok(SearchResults::default())
            }
            fn capabilities(&self) -> SearchCapabilities {
                SearchCapabilities {
                    regex: false,
                    glob_filter: false,
                    content_search: false,
                    filename_search: true,
                }
            }
        }
        let p = LimitedProvider;
        assert!(!p.capabilities().content_search);
        assert!(p.capabilities().filename_search);
    }

    #[test]
    fn search_provider_returns_none_for_path() {
        struct SelectiveSearchFs {
            inner: InMemoryFs,
        }

        #[async_trait::async_trait]
        impl FileSystemExt for SelectiveSearchFs {}

        #[async_trait::async_trait]
        impl FileSystem for SelectiveSearchFs {
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

        impl SearchCapable for SelectiveSearchFs {
            fn search_provider(&self, path: &Path) -> Option<Box<dyn SearchProvider>> {
                // Only provide search for /indexed/ paths
                if path.starts_with("/indexed") {
                    Some(Box::new(MockProvider { results: vec![] }))
                } else {
                    None
                }
            }
        }

        let fs = SelectiveSearchFs {
            inner: InMemoryFs::new(),
        };
        // Path-scoped: /indexed returns provider, /other returns None
        assert!(fs.search_provider(Path::new("/indexed")).is_some());
        assert!(fs.search_provider(Path::new("/other")).is_none());
    }

    #[test]
    fn search_provider_error_result() {
        struct ErrorProvider;
        impl SearchProvider for ErrorProvider {
            fn search(&self, _query: &SearchQuery) -> Result<SearchResults> {
                Err(crate::Error::Io(std::io::Error::other("index corrupted")))
            }
            fn capabilities(&self) -> SearchCapabilities {
                SearchCapabilities {
                    content_search: true,
                    ..SearchCapabilities::default()
                }
            }
        }
        let p = ErrorProvider;
        let result = p.search(&SearchQuery {
            pattern: "x".into(),
            is_regex: false,
            case_insensitive: false,
            root: PathBuf::from("/"),
            glob_filter: None,
            max_results: None,
        });
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("index corrupted"));
    }

    #[test]
    fn search_capabilities_debug() {
        let c = SearchCapabilities::default();
        let dbg = format!("{:?}", c);
        assert!(dbg.contains("SearchCapabilities"));
    }

    #[test]
    fn search_query_debug() {
        let q = SearchQuery {
            pattern: "hello".into(),
            is_regex: false,
            case_insensitive: false,
            root: PathBuf::from("/"),
            glob_filter: None,
            max_results: None,
        };
        let dbg = format!("{:?}", q);
        assert!(dbg.contains("hello"));
    }
}
