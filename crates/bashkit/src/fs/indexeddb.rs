//! IndexedDB filesystem backend for wasm32.
//!
//! [`IndexedDbFs`] implements [`FsBackend`] using the browser's IndexedDB API
//! via the `rexie` crate. It persists files and directories across page reloads
//! in browser environments.
//!
//! # Usage
//!
//! ```rust,ignore
//! use bashkit::{FsBackend, PosixFs, IndexedDbFs};
//! use std::sync::Arc;
//!
//! let backend = IndexedDbFs::new("bashkit_fs");
//! let fs = Arc::new(PosixFs::new(backend));
//! ```
//!
//! # Safety
//!
//! This module uses [`AssertSend`] to wrap futures that contain `wasm_bindgen`
//! closure types. On `wasm32-unknown-unknown` there is only a single thread, so
//! asserting `Send` is sound. The module is gated to `wasm32` via `cfg` when the
//! `indexeddb` feature is enabled.

use crate::time::{Duration, SystemTime, UNIX_EPOCH};
use async_trait::async_trait;
use rexie::{ObjectStore, Rexie, TransactionMode};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use wasm_bindgen::JsValue;

use super::backend::FsBackend;
use super::normalize_path;
use super::traits::{DirEntry, Metadata};
use crate::error::Result;

const STORE_NAME: &str = "entries";

/// Wrapper that asserts a future is `Send`.
///
/// # Safety
///
/// On `wasm32-unknown-unknown` there is only one thread, so all types are
/// effectively `Send`. This wrapper is only used within the IndexedDB backend
/// which is compiled exclusively for that target.
struct AssertSend<F>(F);

unsafe impl<F> Send for AssertSend<F> {}
unsafe impl<F> Sync for AssertSend<F> {}

impl<F: Future> Future for AssertSend<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: We are projecting from Pin<&mut AssertSend<F>> to Pin<&mut F>.
        // AssertSend is a newtype wrapper with the same memory layout.
        unsafe { self.map_unchecked_mut(|s| &mut s.0).poll(cx) }
    }
}

/// Wrap a future so that it satisfies `Send` bounds.
///
/// This is a synchronous constructor — it immediately wraps `f` in
/// [`AssertSend`] and returns it, so the caller's async generator never
/// holds the unwrapped `f` across an await point.
fn run<F: Future>(f: F) -> AssertSend<F> {
    AssertSend(f)
}

/// Stored representation of a filesystem entry in IndexedDB.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct DbEntry {
    path: String,
    kind: DbEntryKind,
    content: Option<Vec<u8>>,
    mode: u32,
    modified: f64,
    created: f64,
    target: Option<String>,
    size: u64,
}

/// Kind of filesystem entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DbEntryKind {
    File,
    Directory,
    Symlink,
}

/// IndexedDB filesystem backend.
///
/// Stores files, directories, and symlinks in the browser's IndexedDB.
/// Each operation opens the database, performs the work, and closes it.
/// This avoids `Send`/`Sync` issues with `rexie`'s internal closure types.
#[derive(Clone, Debug)]
pub struct IndexedDbFs {
    db_name: String,
}

impl IndexedDbFs {
    /// Create a new IndexedDB filesystem with the given database name.
    pub fn new(db_name: impl Into<String>) -> Self {
        Self {
            db_name: db_name.into(),
        }
    }

    fn now_ms() -> f64 {
        let now = SystemTime::now();
        let dur = now.duration_since(UNIX_EPOCH).unwrap_or_default();
        dur.as_millis() as f64
    }

    fn system_time_to_ms(time: SystemTime) -> f64 {
        let dur = time.duration_since(UNIX_EPOCH).unwrap_or_default();
        dur.as_millis() as f64
    }

    fn ms_to_system_time(ms: f64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms.max(0.0) as u64)
    }

    fn entry_to_metadata(entry: &DbEntry) -> Metadata {
        use super::traits::FileType;
        let file_type = match entry.kind {
            DbEntryKind::File => FileType::File,
            DbEntryKind::Directory => FileType::Directory,
            DbEntryKind::Symlink => FileType::Symlink,
        };
        Metadata {
            file_type,
            size: entry.size,
            mode: entry.mode,
            modified: Self::ms_to_system_time(entry.modified),
            created: Self::ms_to_system_time(entry.created),
        }
    }

    fn is_direct_child(parent: &Path, child_path: &str) -> Option<String> {
        let parent_str = parent.to_str()?;
        let child = Path::new(child_path);
        if child.parent()?.as_os_str() == parent_str {
            child.file_name()?.to_str().map(|s| s.to_string())
        } else {
            None
        }
    }

    /// Open the IndexedDB and ensure the root directory `/` exists.
    async fn open_db(db_name: &str) -> Result<Rexie> {
        let db = Rexie::builder(db_name)
            .version(1)
            .add_object_store(ObjectStore::new(STORE_NAME).key_path("path"))
            .build()
            .await
            .map_err(|e| IoError::other(format!("indexeddb open: {e}")))?;

        // Invisible root node — ensure `/` always exists
        let tx = db
            .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
            .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
        let store = tx
            .store(STORE_NAME)
            .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

        let root_key: JsValue = "/".into();
        if store
            .get(root_key.clone())
            .await
            .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?
            .is_none()
        {
            let root = DbEntry {
                path: "/".to_string(),
                kind: DbEntryKind::Directory,
                content: None,
                mode: 0o755,
                modified: Self::now_ms(),
                created: Self::now_ms(),
                target: None,
                size: 0,
            };
            let js = serde_wasm_bindgen::to_value(&root)
                .map_err(|e| IoError::other(format!("serialize: {e}")))?;
            store
                .add(&js, None)
                .await
                .map_err(|e| IoError::other(format!("indexeddb add: {e}")))?;
        }

        tx.done()
            .await
            .map_err(|e| IoError::other(format!("indexeddb commit: {e}")))?;
        Ok(db)
    }
}

fn path_to_js(path: &Path) -> std::io::Result<JsValue> {
    path.to_str()
        .ok_or_else(|| IoError::other("non-UTF-8 path"))
        .map(|s| s.into())
}

fn path_to_string(path: &Path) -> std::io::Result<String> {
    path.to_str()
        .ok_or_else(|| IoError::other("non-UTF-8 path"))
        .map(|s| s.to_string())
}

#[async_trait]
impl FsBackend for IndexedDbFs {
    async fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let path = normalize_path(path);
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadOnly)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let js_key: JsValue = path_to_js(&path)?;
            let js_value = store
                .get(js_key)
                .await
                .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?
                .ok_or_else(|| IoError::from(ErrorKind::NotFound))?;

            let entry: DbEntry = serde_wasm_bindgen::from_value(js_value)
                .map_err(|e| IoError::other(format!("deserialize: {e}")))?;

            Ok(entry.content.unwrap_or_default())
        })
        .await
    }

    async fn write(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = normalize_path(path);
        let content = content.to_vec();
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let js_key: JsValue = path_to_js(&path)?;
            let now = Self::now_ms();

            let content_len = content.len() as u64;
            let entry = if let Some(js_value) = store
                .get(js_key.clone())
                .await
                .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?
            {
                let mut existing: DbEntry = serde_wasm_bindgen::from_value(js_value)
                    .map_err(|e| IoError::other(format!("deserialize: {e}")))?;
                existing.content = Some(content);
                existing.modified = now;
                existing.size = content_len;
                existing
            } else {
                DbEntry {
                    path: path_to_string(&path)?,
                    kind: DbEntryKind::File,
                    content: Some(content),
                    mode: 0o644,
                    modified: now,
                    created: now,
                    target: None,
                    size: content_len,
                }
            };

            let js_entry = serde_wasm_bindgen::to_value(&entry)
                .map_err(|e| IoError::other(format!("serialize: {e}")))?;
            store
                .put(&js_entry, None)
                .await
                .map_err(|e| IoError::other(format!("indexeddb put: {e}")))?;

            tx.done()
                .await
                .map_err(|e| IoError::other(format!("indexeddb tx done: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn append(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path = normalize_path(path);
        let content = content.to_vec();
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let js_key: JsValue = path_to_js(&path)?;
            let mut existing_content = Vec::new();
            let mut mode = 0o644;
            let mut created = Self::now_ms();

            if let Some(js_value) = store
                .get(js_key.clone())
                .await
                .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?
            {
                let existing: DbEntry = serde_wasm_bindgen::from_value(js_value)
                    .map_err(|e| IoError::other(format!("deserialize: {e}")))?;
                if let Some(content) = existing.content {
                    existing_content = content;
                }
                mode = existing.mode;
                created = existing.created;
            }

            existing_content.extend_from_slice(&content);

            let entry = DbEntry {
                path: path_to_string(&path)?,
                kind: DbEntryKind::File,
                content: Some(existing_content.clone()),
                mode,
                modified: Self::now_ms(),
                created,
                target: None,
                size: existing_content.len() as u64,
            };

            let js_entry = serde_wasm_bindgen::to_value(&entry)
                .map_err(|e| IoError::other(format!("serialize: {e}")))?;
            store
                .put(&js_entry, None)
                .await
                .map_err(|e| IoError::other(format!("indexeddb put: {e}")))?;

            tx.done()
                .await
                .map_err(|e| IoError::other(format!("indexeddb tx done: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = normalize_path(path);
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let now = Self::now_ms();

            if recursive {
                let mut current = PathBuf::from("/");
                for component in path.components().skip(1) {
                    current.push(component);
                    let js_key: JsValue = path_to_js(&current)?;
                    if store
                        .get(js_key.clone())
                        .await
                        .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?
                        .is_none()
                    {
                        let entry = DbEntry {
                            path: path_to_string(&current)?,
                            kind: DbEntryKind::Directory,
                            content: None,
                            mode: 0o755,
                            modified: now,
                            created: now,
                            target: None,
                            size: 0,
                        };
                        let js_entry = serde_wasm_bindgen::to_value(&entry)
                            .map_err(|e| IoError::other(format!("serialize: {e}")))?;
                        store
                            .add(&js_entry, None)
                            .await
                            .map_err(|e| IoError::other(format!("indexeddb add: {e}")))?;
                    }
                }
            } else {
                let js_key: JsValue = path_to_js(&path)?;
                if store
                    .get(js_key.clone())
                    .await
                    .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?
                    .is_none()
                {
                    let entry = DbEntry {
                        path: path_to_string(&path)?,
                        kind: DbEntryKind::Directory,
                        content: None,
                        mode: 0o755,
                        modified: now,
                        created: now,
                        target: None,
                        size: 0,
                    };
                    let js_entry = serde_wasm_bindgen::to_value(&entry)
                        .map_err(|e| IoError::other(format!("serialize: {e}")))?;
                    store
                        .add(&js_entry, None)
                        .await
                        .map_err(|e| IoError::other(format!("indexeddb add: {e}")))?;
                }
            }

            tx.done()
                .await
                .map_err(|e| IoError::other(format!("indexeddb tx done: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
        let path = normalize_path(path);
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let js_key: JsValue = path_to_js(&path)?;
            let existing = store
                .get(js_key.clone())
                .await
                .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?;

            if existing.is_none() {
                return Err(IoError::from(ErrorKind::NotFound).into());
            }

            if recursive {
                let path_s = path_to_string(&path)?;
                let prefix = format!("{}/", path_s);
                let all = store
                    .get_all(None, None)
                    .await
                    .map_err(|e| IoError::other(format!("indexeddb get_all: {e}")))?;
                for js_value in all {
                    let entry: DbEntry = serde_wasm_bindgen::from_value(js_value)
                        .map_err(|e| IoError::other(format!("deserialize: {e}")))?;
                    if entry.path == path_s || entry.path.starts_with(&prefix) {
                        store
                            .delete(entry.path.into())
                            .await
                            .map_err(|e| IoError::other(format!("indexeddb delete: {e}")))?;
                    }
                }
            } else {
                store
                    .delete(js_key)
                    .await
                    .map_err(|e| IoError::other(format!("indexeddb delete: {e}")))?;
            }

            tx.done()
                .await
                .map_err(|e| IoError::other(format!("indexeddb tx done: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn stat(&self, path: &Path) -> Result<Metadata> {
        let path = normalize_path(path);
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadOnly)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let js_key: JsValue = path_to_js(&path)?;
            let js_value = store
                .get(js_key)
                .await
                .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?
                .ok_or_else(|| IoError::from(ErrorKind::NotFound))?;

            let entry: DbEntry = serde_wasm_bindgen::from_value(js_value)
                .map_err(|e| IoError::other(format!("deserialize: {e}")))?;

            Ok(Self::entry_to_metadata(&entry))
        })
        .await
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let path = normalize_path(path);
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadOnly)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let js_key: JsValue = path_to_js(&path)?;
            if let Some(js_value) = store
                .get(js_key.clone())
                .await
                .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?
            {
                let entry: DbEntry = serde_wasm_bindgen::from_value(js_value)
                    .map_err(|e| IoError::other(format!("deserialize: {e}")))?;
                if !matches!(entry.kind, DbEntryKind::Directory) {
                    return Err(IoError::from(ErrorKind::NotFound).into());
                }
            } else {
                return Err(IoError::from(ErrorKind::NotFound).into());
            }

            let mut entries = Vec::new();
            let all = store
                .get_all(None, None)
                .await
                .map_err(|e| IoError::other(format!("indexeddb get_all: {e}")))?;
            for js_value in all {
                let entry: DbEntry = serde_wasm_bindgen::from_value(js_value)
                    .map_err(|e| IoError::other(format!("deserialize: {e}")))?;
                if let Some(name) = Self::is_direct_child(&path, &entry.path) {
                    entries.push(DirEntry {
                        name,
                        metadata: Self::entry_to_metadata(&entry),
                    });
                }
            }

            Ok(entries)
        })
        .await
    }

    async fn exists(&self, path: &Path) -> Result<bool> {
        let path = normalize_path(path);
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadOnly)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let js_key: JsValue = path_to_js(&path)?;
            let existing = store
                .get(js_key)
                .await
                .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?;
            Ok(existing.is_some())
        })
        .await
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let from = normalize_path(from);
        let to = normalize_path(to);
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let from_js: JsValue = path_to_js(&from)?;
            let js_value = store
                .get(from_js.clone())
                .await
                .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?
                .ok_or_else(|| IoError::from(ErrorKind::NotFound))?;
            let mut entry: DbEntry = serde_wasm_bindgen::from_value(js_value)
                .map_err(|e| IoError::other(format!("deserialize: {e}")))?;

            store
                .delete(from_js)
                .await
                .map_err(|e| IoError::other(format!("indexeddb delete: {e}")))?;

            let from_s = path_to_string(&from)?;
            let to_s = path_to_string(&to)?;
            let from_prefix = format!("{}/", from_s);
            let to_prefix = format!("{}/", to_s);
            let all = store
                .get_all(None, None)
                .await
                .map_err(|e| IoError::other(format!("indexeddb get_all: {e}")))?;
            for js_value in all {
                let mut child: DbEntry = serde_wasm_bindgen::from_value(js_value)
                    .map_err(|e| IoError::other(format!("deserialize: {e}")))?;
                if child.path.starts_with(&from_prefix) {
                    let new_path = to_prefix.clone() + &child.path[from_prefix.len()..];
                    store
                        .delete(child.path.clone().into())
                        .await
                        .map_err(|e| IoError::other(format!("indexeddb delete: {e}")))?;
                    child.path = new_path;
                    let js_child = serde_wasm_bindgen::to_value(&child)
                        .map_err(|e| IoError::other(format!("serialize: {e}")))?;
                    store
                        .add(&js_child, None)
                        .await
                        .map_err(|e| IoError::other(format!("indexeddb add: {e}")))?;
                }
            }

            entry.path = path_to_string(&to)?;
            entry.modified = Self::now_ms();
            let js_entry = serde_wasm_bindgen::to_value(&entry)
                .map_err(|e| IoError::other(format!("serialize: {e}")))?;
            store
                .add(&js_entry, None)
                .await
                .map_err(|e| IoError::other(format!("indexeddb add: {e}")))?;

            tx.done()
                .await
                .map_err(|e| IoError::other(format!("indexeddb tx done: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        let from = normalize_path(from);
        let to = normalize_path(to);
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let from_js: JsValue = path_to_js(&from)?;
            let js_value = store
                .get(from_js)
                .await
                .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?
                .ok_or_else(|| IoError::from(ErrorKind::NotFound))?;
            let mut entry: DbEntry = serde_wasm_bindgen::from_value(js_value)
                .map_err(|e| IoError::other(format!("deserialize: {e}")))?;

            entry.path = path_to_string(&to)?;
            entry.created = Self::now_ms();
            entry.modified = Self::now_ms();

            let js_entry = serde_wasm_bindgen::to_value(&entry)
                .map_err(|e| IoError::other(format!("serialize: {e}")))?;
            store
                .add(&js_entry, None)
                .await
                .map_err(|e| IoError::other(format!("indexeddb add: {e}")))?;

            tx.done()
                .await
                .map_err(|e| IoError::other(format!("indexeddb tx done: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
        let target = normalize_path(target);
        let link = normalize_path(link);
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let now = Self::now_ms();
            let entry = DbEntry {
                path: path_to_string(&link)?,
                kind: DbEntryKind::Symlink,
                content: None,
                mode: 0o777,
                modified: now,
                created: now,
                target: Some(path_to_string(&target)?),
                size: 0,
            };

            let js_entry = serde_wasm_bindgen::to_value(&entry)
                .map_err(|e| IoError::other(format!("serialize: {e}")))?;
            store
                .add(&js_entry, None)
                .await
                .map_err(|e| IoError::other(format!("indexeddb add: {e}")))?;

            tx.done()
                .await
                .map_err(|e| IoError::other(format!("indexeddb tx done: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn read_link(&self, path: &Path) -> Result<PathBuf> {
        let path = normalize_path(path);
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadOnly)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let js_key: JsValue = path_to_js(&path)?;
            let js_value = store
                .get(js_key)
                .await
                .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?
                .ok_or_else(|| IoError::from(ErrorKind::NotFound))?;

            let entry: DbEntry = serde_wasm_bindgen::from_value(js_value)
                .map_err(|e| IoError::other(format!("deserialize: {e}")))?;

            match entry.target {
                Some(target) => Ok(PathBuf::from(target)),
                None => Err(IoError::other("not a symlink").into()),
            }
        })
        .await
    }

    async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
        let path = normalize_path(path);
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let js_key: JsValue = path_to_js(&path)?;
            let js_value = store
                .get(js_key.clone())
                .await
                .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?
                .ok_or_else(|| IoError::from(ErrorKind::NotFound))?;

            let mut entry: DbEntry = serde_wasm_bindgen::from_value(js_value)
                .map_err(|e| IoError::other(format!("deserialize: {e}")))?;
            entry.mode = mode;

            let js_entry = serde_wasm_bindgen::to_value(&entry)
                .map_err(|e| IoError::other(format!("serialize: {e}")))?;
            store
                .put(&js_entry, None)
                .await
                .map_err(|e| IoError::other(format!("indexeddb put: {e}")))?;

            tx.done()
                .await
                .map_err(|e| IoError::other(format!("indexeddb tx done: {e}")))?;
            Ok(())
        })
        .await
    }

    async fn set_modified_time(&self, path: &Path, time: SystemTime) -> Result<()> {
        let path = normalize_path(path);
        let db_name = self.db_name.clone();
        run(async move {
            let db = IndexedDbFs::open_db(&db_name).await?;
            let tx = db
                .transaction(&[STORE_NAME], TransactionMode::ReadWrite)
                .map_err(|e| IoError::other(format!("indexeddb tx: {e}")))?;
            let store = tx
                .store(STORE_NAME)
                .map_err(|e| IoError::other(format!("indexeddb store: {e}")))?;

            let js_key: JsValue = path_to_js(&path)?;
            let js_value = store
                .get(js_key.clone())
                .await
                .map_err(|e| IoError::other(format!("indexeddb get: {e}")))?
                .ok_or_else(|| IoError::from(ErrorKind::NotFound))?;

            let mut entry: DbEntry = serde_wasm_bindgen::from_value(js_value)
                .map_err(|e| IoError::other(format!("deserialize: {e}")))?;
            entry.modified = Self::system_time_to_ms(time);

            let js_entry = serde_wasm_bindgen::to_value(&entry)
                .map_err(|e| IoError::other(format!("serialize: {e}")))?;
            store
                .put(&js_entry, None)
                .await
                .map_err(|e| IoError::other(format!("indexeddb put: {e}")))?;

            tx.done()
                .await
                .map_err(|e| IoError::other(format!("indexeddb tx done: {e}")))?;
            Ok(())
        })
        .await
    }
}
