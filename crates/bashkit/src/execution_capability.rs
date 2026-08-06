// THREAT[TM-ISO-027]. Important decision: embedders receive lease-backed handles, never a request's
// raw facilities. One atomic lease is revoked before request state is restored,
// so completion, timeout, and future cancellation have identical semantics.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

const ACTIVE: u8 = 0;
const REVOKING: u8 = 1;
const REVOKED: u8 = 2;

/// Safe, bounded summary of execution-capability cleanup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CapabilityCleanupReport {
    /// Whether the request lease was revoked.
    pub revoked: bool,
    /// Number of cleanup steps that required failure recovery.
    ///
    /// Details are intentionally omitted because lock/panic diagnostics may
    /// contain host internals.
    pub failures: u8,
}

/// Deterministic error returned by a handle used outside its execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ExecutionCapabilityError {
    /// The execution completed, was cancelled, or belongs to another request.
    #[error("execution capability revoked")]
    Revoked,
}

pub(crate) struct ExecutionScope {
    state: AtomicU8,
    revoked: tokio::sync::Notify,
    values: Mutex<Vec<Arc<dyn RevocableValue>>>,
    report: OnceLock<CapabilityCleanupReport>,
}

impl ExecutionScope {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: AtomicU8::new(ACTIVE),
            revoked: tokio::sync::Notify::new(),
            values: Mutex::new(Vec::new()),
            report: OnceLock::new(),
        })
    }

    pub(crate) fn ensure_active(&self) -> Result<(), ExecutionCapabilityError> {
        if self.state.load(Ordering::Acquire) == ACTIVE {
            Ok(())
        } else {
            Err(ExecutionCapabilityError::Revoked)
        }
    }

    pub(crate) async fn run<F>(&self, future: F) -> Result<F::Output, ExecutionCapabilityError>
    where
        F: Future,
    {
        let revoked = self.revoked.notified();
        tokio::pin!(revoked);
        revoked.as_mut().enable();
        self.ensure_active()?;
        tokio::select! {
            biased;
            _ = &mut revoked => Err(ExecutionCapabilityError::Revoked),
            output = future => {
                self.ensure_active()?;
                Ok(output)
            }
        }
    }

    pub(crate) fn begin_revoke(&self) -> bool {
        let started = self
            .state
            .compare_exchange(ACTIVE, REVOKING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        if started {
            self.revoked.notify_waiters();
        }
        started
    }

    pub(crate) fn finish_revoke(&self, failures: u8) -> CapabilityCleanupReport {
        if self.report.get().is_none() {
            let (mut values, poisoned) = match self.values.lock() {
                Ok(values) => (values, false),
                Err(poisoned) => (poisoned.into_inner(), true),
            };
            let value_failures = values.drain(..).filter(|value| value.revoke()).count();
            let failures = failures
                .saturating_add(u8::from(poisoned))
                .saturating_add(u8::try_from(value_failures).unwrap_or(u8::MAX));
            let report = CapabilityCleanupReport {
                revoked: true,
                failures,
            };
            let _ = self.report.set(report);
            self.state.store(REVOKED, Ordering::Release);
            report
        } else {
            self.report
                .get()
                .copied()
                .unwrap_or(CapabilityCleanupReport {
                    revoked: true,
                    failures,
                })
        }
    }

    #[cfg(test)]
    pub(crate) fn revoke(&self, failures: u8) -> CapabilityCleanupReport {
        let _ = self.begin_revoke();
        self.finish_revoke(failures)
    }

    fn cleanup_report(&self) -> Option<CapabilityCleanupReport> {
        self.report.get().copied()
    }

    fn register(&self, value: Arc<dyn RevocableValue>) -> bool {
        if self.ensure_active().is_err() {
            return false;
        }
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.ensure_active().is_err() {
            return false;
        }
        values.push(value);
        true
    }

    fn capability<T>(self: &Arc<Self>, value: T) -> Option<ExecutionCapability<T>>
    where
        T: Send + Sync + 'static,
    {
        let cell = Arc::new(CapabilityCell::new(value));
        if !self.register(cell.clone()) {
            return None;
        }
        Some(ExecutionCapability::new(cell, self.clone()))
    }
}

trait RevocableValue: Send + Sync {
    /// Returns whether poisoned state required recovery.
    fn revoke(&self) -> bool;
}

struct CapabilityCell<T> {
    value: RwLock<Option<Arc<T>>>,
}

impl<T> CapabilityCell<T> {
    fn new(value: T) -> Self {
        Self {
            value: RwLock::new(Some(Arc::new(value))),
        }
    }

    fn value(&self) -> Option<Arc<T>> {
        self.value
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl<T: Send + Sync> RevocableValue for CapabilityCell<T> {
    fn revoke(&self) -> bool {
        match self.value.write() {
            Ok(mut value) => {
                value.take();
                false
            }
            Err(poisoned) => {
                poisoned.into_inner().take();
                true
            }
        }
    }
}

/// Cloneable access handle tied to exactly one `Bash::exec*()` call.
///
/// Keeping this handle is safe: every access rechecks the request lease and
/// returns [`ExecutionCapabilityError::Revoked`] after completion/cancellation.
pub struct ExecutionCapability<T> {
    value: Arc<CapabilityCell<T>>,
    scope: Arc<ExecutionScope>,
}

impl<T> Clone for ExecutionCapability<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            scope: self.scope.clone(),
        }
    }
}

impl<T> ExecutionCapability<T> {
    fn new(value: Arc<CapabilityCell<T>>, scope: Arc<ExecutionScope>) -> Self {
        Self { value, scope }
    }

    /// Use the value only while its originating execution remains active.
    pub fn try_with<R>(
        &self,
        use_value: impl FnOnce(&T) -> R,
    ) -> Result<R, ExecutionCapabilityError> {
        self.scope.ensure_active()?;
        let value = self
            .value
            .value()
            .ok_or(ExecutionCapabilityError::Revoked)?;
        let output = use_value(&value);
        self.scope.ensure_active()?;
        Ok(output)
    }

    /// Poll an asynchronous operation only while this execution is active.
    ///
    /// Revocation drops the operation future and returns `Revoked`.
    pub async fn run<F>(&self, future: F) -> Result<F::Output, ExecutionCapabilityError>
    where
        F: Future,
    {
        self.scope.run(future).await
    }

    /// Return the sanitized cleanup outcome once the request is revoked.
    pub fn cleanup_report(&self) -> Option<CapabilityCleanupReport> {
        self.scope.cleanup_report()
    }

    #[cfg(feature = "scripted_tool")]
    pub(crate) fn derive<U>(&self, value: U) -> Option<ExecutionCapability<U>>
    where
        U: Send + Sync + 'static,
    {
        self.scope.capability(value)
    }
}

struct StoredCapability {
    value: Arc<dyn Any + Send + Sync>,
    revoker: Arc<dyn RevocableValue>,
}

/// Typed values bound to one execution when passed through [`crate::ExecOptions`].
#[derive(Default)]
pub struct ExecutionExtensions {
    values: HashMap<TypeId, StoredCapability>,
    scope: Option<Arc<ExecutionScope>>,
}

impl ExecutionExtensions {
    /// Create an empty execution extension bag.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a typed value, replacing any previous value of the same type.
    pub fn insert<T>(&mut self, value: T) -> Option<T>
    where
        T: Send + Sync + 'static,
    {
        let cell = Arc::new(CapabilityCell::new(value));
        let previous = self.values.insert(
            TypeId::of::<T>(),
            StoredCapability {
                value: cell.clone(),
                revoker: cell,
            },
        )?;
        drop(previous.revoker);
        let cell = Arc::downcast::<CapabilityCell<T>>(previous.value).ok()?;
        let cell = Arc::try_unwrap(cell).ok()?;
        let value = cell
            .value
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)?;
        Arc::try_unwrap(value).ok()
    }

    /// Builder-style insert.
    pub fn with<T>(mut self, value: T) -> Self
    where
        T: Send + Sync + 'static,
    {
        let _ = self.insert(value);
        self
    }

    pub(crate) fn bind(&mut self, scope: Arc<ExecutionScope>) {
        debug_assert!(self.scope.is_none(), "execution extensions bound twice");
        for value in self.values.values() {
            let registered = scope.register(value.revoker.clone());
            debug_assert!(registered, "active execution scope rejected extension");
        }
        self.scope = Some(scope);
    }

    pub(crate) fn get<T>(&self) -> Option<ExecutionCapability<T>>
    where
        T: Send + Sync + 'static,
    {
        let value = self.values.get(&TypeId::of::<T>())?.value.clone();
        let value = Arc::downcast::<CapabilityCell<T>>(value).ok()?;
        Some(ExecutionCapability::new(
            value,
            self.scope.as_ref()?.clone(),
        ))
    }

    pub(crate) fn scope(&self) -> Option<Arc<ExecutionScope>> {
        self.scope.clone()
    }

    pub(crate) fn capability<T>(&self, value: T) -> Option<ExecutionCapability<T>>
    where
        T: Send + Sync + 'static,
    {
        self.scope.as_ref()?.capability(value)
    }

    /// Return whether the bag is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Revocable VFS view supplied to scoped builtins.
pub(crate) struct ExecutionFileSystem {
    inner: Arc<dyn crate::FileSystem>,
    scope: Arc<ExecutionScope>,
}

impl ExecutionFileSystem {
    pub(crate) fn wrap(
        inner: Arc<dyn crate::FileSystem>,
        scope: Arc<ExecutionScope>,
    ) -> Arc<dyn crate::FileSystem> {
        Arc::new(Self { inner, scope })
    }

    fn revoked() -> crate::Error {
        std::io::Error::other(ExecutionCapabilityError::Revoked.to_string()).into()
    }

    async fn run<F>(&self, future: F) -> crate::Result<F::Output>
    where
        F: Future,
    {
        self.scope.run(future).await.map_err(|_| Self::revoked())
    }
}

#[async_trait::async_trait]
impl crate::FileSystemExt for ExecutionFileSystem {
    fn usage(&self) -> crate::FsUsage {
        if self.scope.ensure_active().is_ok() {
            self.inner.usage()
        } else {
            crate::FsUsage::default()
        }
    }

    async fn mkfifo(&self, path: &std::path::Path, mode: u32) -> crate::Result<()> {
        self.run(self.inner.mkfifo(path, mode)).await?
    }

    fn limits(&self) -> crate::FsLimits {
        if self.scope.ensure_active().is_ok() {
            self.inner.limits()
        } else {
            crate::FsLimits::default()
        }
    }

    fn vfs_snapshot(&self) -> Option<crate::VfsSnapshot> {
        self.scope
            .ensure_active()
            .ok()
            .and_then(|()| self.inner.vfs_snapshot())
    }

    fn vfs_restore(&self, snapshot: &crate::VfsSnapshot) -> crate::Result<()> {
        self.scope.ensure_active().map_err(|_| Self::revoked())?;
        self.inner.vfs_restore(snapshot)
    }

    fn backend_kind(&self) -> &'static str {
        "execution-scoped"
    }
}

#[async_trait::async_trait]
impl crate::FileSystem for ExecutionFileSystem {
    async fn read_file(&self, path: &std::path::Path) -> crate::Result<Vec<u8>> {
        self.run(self.inner.read_file(path)).await?
    }

    async fn write_file(&self, path: &std::path::Path, content: &[u8]) -> crate::Result<()> {
        self.run(self.inner.write_file(path, content)).await?
    }

    async fn append_file(&self, path: &std::path::Path, content: &[u8]) -> crate::Result<()> {
        self.run(self.inner.append_file(path, content)).await?
    }

    async fn mkdir(&self, path: &std::path::Path, recursive: bool) -> crate::Result<()> {
        self.run(self.inner.mkdir(path, recursive)).await?
    }

    async fn remove(&self, path: &std::path::Path, recursive: bool) -> crate::Result<()> {
        self.run(self.inner.remove(path, recursive)).await?
    }

    async fn stat(&self, path: &std::path::Path) -> crate::Result<crate::Metadata> {
        self.run(self.inner.stat(path)).await?
    }

    async fn read_dir(&self, path: &std::path::Path) -> crate::Result<Vec<crate::DirEntry>> {
        self.run(self.inner.read_dir(path)).await?
    }

    async fn exists(&self, path: &std::path::Path) -> crate::Result<bool> {
        self.run(self.inner.exists(path)).await?
    }

    async fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> crate::Result<()> {
        self.run(self.inner.rename(from, to)).await?
    }

    async fn copy(&self, from: &std::path::Path, to: &std::path::Path) -> crate::Result<()> {
        self.run(self.inner.copy(from, to)).await?
    }

    async fn symlink(&self, target: &std::path::Path, link: &std::path::Path) -> crate::Result<()> {
        self.run(self.inner.symlink(target, link)).await?
    }

    async fn read_link(&self, path: &std::path::Path) -> crate::Result<std::path::PathBuf> {
        self.run(self.inner.read_link(path)).await?
    }

    async fn chmod(&self, path: &std::path::Path, mode: u32) -> crate::Result<()> {
        self.run(self.inner.chmod(path, mode)).await?
    }

    async fn set_modified_time(
        &self,
        path: &std::path::Path,
        time: crate::time_compat::SystemTime,
    ) -> crate::Result<()> {
        self.run(self.inner.set_modified_time(path, time)).await?
    }

    // Returning the raw search provider would bypass the execution lease.
    fn as_search_capable(&self) -> Option<&dyn crate::SearchCapable> {
        None
    }
}
