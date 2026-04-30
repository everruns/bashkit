// Decision: cross-addon filesystem interop uses only a versioned repr(C)
// handle + vtable. Rust trait objects stay inside the exporting addon.

use crate::{
    DirEntry, Error as BashError, FileSystem, FileSystemExt, FileType, Metadata,
    Result as BashResult, async_trait,
};
use std::ffi::c_void;
use std::future::Future;
use std::io::{self, Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::str;
use std::sync::Arc;
use std::sync::mpsc::sync_channel;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::{Builder, Runtime};

pub const BASHKIT_FS_ABI_VERSION_V1: u32 = 1;

pub type BashkitFsAbiStatus = u32;
pub const BASHKIT_FS_ABI_STATUS_OK: BashkitFsAbiStatus = 0;
pub const BASHKIT_FS_ABI_STATUS_ERR: BashkitFsAbiStatus = 1;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BashkitFsAbiStrRef {
    pub ptr: *const u8,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BashkitFsAbiOwnedBytes {
    pub ptr: *mut u8,
    pub len: usize,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Default)]
pub enum BashkitFsAbiErrorKind {
    #[default]
    Other = 0,
    NotFound = 1,
    AlreadyExists = 2,
    PermissionDenied = 3,
    InvalidInput = 4,
    IsADirectory = 5,
    NotADirectory = 6,
    DirectoryNotEmpty = 7,
    Unsupported = 8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BashkitFsAbiError {
    pub kind: BashkitFsAbiErrorKind,
    pub message: BashkitFsAbiOwnedBytes,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default)]
pub enum BashkitFsAbiFileType {
    #[default]
    File = 0,
    Directory = 1,
    Symlink = 2,
    Fifo = 3,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BashkitFsAbiMetadata {
    pub file_type: BashkitFsAbiFileType,
    pub _reserved: [u8; 7],
    pub size: u64,
    pub mode: u32,
    pub modified_secs: i64,
    pub modified_nanos: u32,
    pub created_secs: i64,
    pub created_nanos: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BashkitFsAbiDirEntry {
    pub name: BashkitFsAbiOwnedBytes,
    pub metadata: BashkitFsAbiMetadata,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BashkitFsAbiOwnedDirEntries {
    pub ptr: *mut BashkitFsAbiDirEntry,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BashkitFsAbiVTableV1 {
    pub read_file: unsafe extern "C" fn(
        instance: *const c_void,
        path: BashkitFsAbiStrRef,
        out: *mut BashkitFsAbiOwnedBytes,
        err: *mut BashkitFsAbiError,
    ) -> BashkitFsAbiStatus,
    pub write_file: unsafe extern "C" fn(
        instance: *const c_void,
        path: BashkitFsAbiStrRef,
        content: BashkitFsAbiStrRef,
        err: *mut BashkitFsAbiError,
    ) -> BashkitFsAbiStatus,
    pub append_file: unsafe extern "C" fn(
        instance: *const c_void,
        path: BashkitFsAbiStrRef,
        content: BashkitFsAbiStrRef,
        err: *mut BashkitFsAbiError,
    ) -> BashkitFsAbiStatus,
    pub mkdir: unsafe extern "C" fn(
        instance: *const c_void,
        path: BashkitFsAbiStrRef,
        recursive: bool,
        err: *mut BashkitFsAbiError,
    ) -> BashkitFsAbiStatus,
    pub remove: unsafe extern "C" fn(
        instance: *const c_void,
        path: BashkitFsAbiStrRef,
        recursive: bool,
        err: *mut BashkitFsAbiError,
    ) -> BashkitFsAbiStatus,
    pub stat: unsafe extern "C" fn(
        instance: *const c_void,
        path: BashkitFsAbiStrRef,
        out: *mut BashkitFsAbiMetadata,
        err: *mut BashkitFsAbiError,
    ) -> BashkitFsAbiStatus,
    pub read_dir: unsafe extern "C" fn(
        instance: *const c_void,
        path: BashkitFsAbiStrRef,
        out: *mut BashkitFsAbiOwnedDirEntries,
        err: *mut BashkitFsAbiError,
    ) -> BashkitFsAbiStatus,
    pub exists: unsafe extern "C" fn(
        instance: *const c_void,
        path: BashkitFsAbiStrRef,
        out: *mut bool,
        err: *mut BashkitFsAbiError,
    ) -> BashkitFsAbiStatus,
    pub rename: unsafe extern "C" fn(
        instance: *const c_void,
        from: BashkitFsAbiStrRef,
        to: BashkitFsAbiStrRef,
        err: *mut BashkitFsAbiError,
    ) -> BashkitFsAbiStatus,
    pub copy: unsafe extern "C" fn(
        instance: *const c_void,
        from: BashkitFsAbiStrRef,
        to: BashkitFsAbiStrRef,
        err: *mut BashkitFsAbiError,
    ) -> BashkitFsAbiStatus,
    pub symlink: unsafe extern "C" fn(
        instance: *const c_void,
        target: BashkitFsAbiStrRef,
        link: BashkitFsAbiStrRef,
        err: *mut BashkitFsAbiError,
    ) -> BashkitFsAbiStatus,
    pub read_link: unsafe extern "C" fn(
        instance: *const c_void,
        path: BashkitFsAbiStrRef,
        out: *mut BashkitFsAbiOwnedBytes,
        err: *mut BashkitFsAbiError,
    ) -> BashkitFsAbiStatus,
    pub chmod: unsafe extern "C" fn(
        instance: *const c_void,
        path: BashkitFsAbiStrRef,
        mode: u32,
        err: *mut BashkitFsAbiError,
    ) -> BashkitFsAbiStatus,
    pub free_bytes: unsafe extern "C" fn(instance: *const c_void, bytes: BashkitFsAbiOwnedBytes),
    pub free_dir_entries:
        unsafe extern "C" fn(instance: *const c_void, entries: BashkitFsAbiOwnedDirEntries),
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BashkitFsAbiHandleV1 {
    pub abi_version: u32,
    pub _reserved: u32,
    pub instance: *const c_void,
    pub retain: unsafe extern "C" fn(instance: *const c_void),
    pub release: unsafe extern "C" fn(instance: *const c_void),
    pub vtable: *const BashkitFsAbiVTableV1,
}

#[repr(C)]
pub struct BashkitFsAbiOwnedHandleV1 {
    pub handle: BashkitFsAbiHandleV1,
}

impl BashkitFsAbiOwnedHandleV1 {
    pub fn as_handle(&self) -> &BashkitFsAbiHandleV1 {
        &self.handle
    }
}

impl Drop for BashkitFsAbiOwnedHandleV1 {
    fn drop(&mut self) {
        unsafe {
            (self.handle.release)(self.handle.instance);
        }
    }
}

unsafe impl Send for BashkitFsAbiHandleV1 {}
unsafe impl Sync for BashkitFsAbiHandleV1 {}
unsafe impl Send for BashkitFsAbiOwnedHandleV1 {}
unsafe impl Sync for BashkitFsAbiOwnedHandleV1 {}

struct ExportState {
    fs: Arc<dyn FileSystem>,
    rt: Runtime,
}

impl ExportState {
    fn run<T, Fut>(&self, fut: Fut) -> io::Result<T>
    where
        T: Send + 'static,
        Fut: Future<Output = BashResult<T>> + Send + 'static,
    {
        let (tx, rx) = sync_channel(1);
        self.rt.handle().spawn(async move {
            let _ = tx.send(fut.await.map_err(bash_error_to_io));
        });
        rx.recv().map_err(|_| {
            IoError::new(
                ErrorKind::BrokenPipe,
                "filesystem interop runtime stopped before operation completed",
            )
        })?
    }
}

pub fn export_filesystem(fs: Arc<dyn FileSystem>) -> io::Result<BashkitFsAbiOwnedHandleV1> {
    let rt = Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let state = Arc::new(ExportState { fs, rt });
    let instance = Arc::into_raw(state).cast::<c_void>();
    Ok(BashkitFsAbiOwnedHandleV1 {
        handle: BashkitFsAbiHandleV1 {
            abi_version: BASHKIT_FS_ABI_VERSION_V1,
            _reserved: 0,
            instance,
            retain: retain_export_state,
            release: release_export_state,
            vtable: &EXPORT_VTABLE,
        },
    })
}

pub fn import_filesystem(handle: &BashkitFsAbiHandleV1) -> io::Result<Arc<dyn FileSystem>> {
    Ok(Arc::new(ImportedFileSystem::from_handle(handle)?) as Arc<dyn FileSystem>)
}

pub fn import_owned_filesystem(
    handle: &BashkitFsAbiOwnedHandleV1,
) -> io::Result<Arc<dyn FileSystem>> {
    import_filesystem(handle.as_handle())
}

pub struct ImportedFileSystem {
    handle: BashkitFsAbiHandleV1,
}

impl ImportedFileSystem {
    pub fn from_handle(handle: &BashkitFsAbiHandleV1) -> io::Result<Self> {
        if handle.abi_version != BASHKIT_FS_ABI_VERSION_V1 {
            return Err(IoError::new(
                ErrorKind::InvalidData,
                format!("unsupported filesystem ABI version: {}", handle.abi_version),
            ));
        }
        if handle.instance.is_null() {
            return Err(IoError::new(
                ErrorKind::InvalidData,
                "filesystem handle instance must not be null",
            ));
        }
        if handle.vtable.is_null() {
            return Err(IoError::new(
                ErrorKind::InvalidData,
                "filesystem handle vtable must not be null",
            ));
        }
        unsafe {
            (handle.retain)(handle.instance);
        }
        Ok(Self { handle: *handle })
    }

    fn vtable(&self) -> &BashkitFsAbiVTableV1 {
        unsafe { &*self.handle.vtable }
    }

    fn call(
        &self,
        f: impl FnOnce(&BashkitFsAbiVTableV1, *mut BashkitFsAbiError) -> BashkitFsAbiStatus,
    ) -> BashResult<()> {
        let mut err = BashkitFsAbiError::default();
        let status = f(self.vtable(), &mut err);
        if status == BASHKIT_FS_ABI_STATUS_OK {
            return Ok(());
        }
        Err(abi_error_to_io(self.vtable(), self.handle.instance, err).into())
    }

    fn take_bytes(&self, bytes: BashkitFsAbiOwnedBytes) -> io::Result<Vec<u8>> {
        let result = owned_bytes_to_vec(bytes);
        unsafe {
            (self.vtable().free_bytes)(self.handle.instance, bytes);
        }
        result
    }

    fn take_dir_entries(&self, entries: BashkitFsAbiOwnedDirEntries) -> BashResult<Vec<DirEntry>> {
        let result = owned_dir_entries_to_vec(entries);
        unsafe {
            (self.vtable().free_dir_entries)(self.handle.instance, entries);
        }
        result.map_err(Into::into)
    }
}

unsafe impl Send for ImportedFileSystem {}
unsafe impl Sync for ImportedFileSystem {}

impl Drop for ImportedFileSystem {
    fn drop(&mut self) {
        unsafe {
            (self.handle.release)(self.handle.instance);
        }
    }
}

#[async_trait]
impl FileSystemExt for ImportedFileSystem {}

#[async_trait]
impl FileSystem for ImportedFileSystem {
    async fn read_file(&self, path: &Path) -> BashResult<Vec<u8>> {
        let path = str_ref_from_path(path)?;
        let mut out = BashkitFsAbiOwnedBytes::default();
        self.call(|vtable, err| unsafe {
            (vtable.read_file)(self.handle.instance, path, &mut out, err)
        })?;
        self.take_bytes(out).map_err(Into::into)
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> BashResult<()> {
        let path = str_ref_from_path(path)?;
        let content = BashkitFsAbiStrRef {
            ptr: content.as_ptr(),
            len: content.len(),
        };
        self.call(|vtable, err| unsafe {
            (vtable.write_file)(self.handle.instance, path, content, err)
        })
    }

    async fn append_file(&self, path: &Path, content: &[u8]) -> BashResult<()> {
        let path = str_ref_from_path(path)?;
        let content = BashkitFsAbiStrRef {
            ptr: content.as_ptr(),
            len: content.len(),
        };
        self.call(|vtable, err| unsafe {
            (vtable.append_file)(self.handle.instance, path, content, err)
        })
    }

    async fn mkdir(&self, path: &Path, recursive: bool) -> BashResult<()> {
        let path = str_ref_from_path(path)?;
        self.call(|vtable, err| unsafe {
            (vtable.mkdir)(self.handle.instance, path, recursive, err)
        })
    }

    async fn remove(&self, path: &Path, recursive: bool) -> BashResult<()> {
        let path = str_ref_from_path(path)?;
        self.call(|vtable, err| unsafe {
            (vtable.remove)(self.handle.instance, path, recursive, err)
        })
    }

    async fn stat(&self, path: &Path) -> BashResult<Metadata> {
        let path = str_ref_from_path(path)?;
        let mut out = BashkitFsAbiMetadata::default();
        self.call(|vtable, err| unsafe {
            (vtable.stat)(self.handle.instance, path, &mut out, err)
        })?;
        abi_metadata_to_metadata(out).map_err(Into::into)
    }

    async fn read_dir(&self, path: &Path) -> BashResult<Vec<DirEntry>> {
        let path = str_ref_from_path(path)?;
        let mut out = BashkitFsAbiOwnedDirEntries::default();
        self.call(|vtable, err| unsafe {
            (vtable.read_dir)(self.handle.instance, path, &mut out, err)
        })?;
        self.take_dir_entries(out)
    }

    async fn exists(&self, path: &Path) -> BashResult<bool> {
        let path = str_ref_from_path(path)?;
        let mut out = false;
        self.call(|vtable, err| unsafe {
            (vtable.exists)(self.handle.instance, path, &mut out, err)
        })?;
        Ok(out)
    }

    async fn rename(&self, from: &Path, to: &Path) -> BashResult<()> {
        let from = str_ref_from_path(from)?;
        let to = str_ref_from_path(to)?;
        self.call(|vtable, err| unsafe { (vtable.rename)(self.handle.instance, from, to, err) })
    }

    async fn copy(&self, from: &Path, to: &Path) -> BashResult<()> {
        let from = str_ref_from_path(from)?;
        let to = str_ref_from_path(to)?;
        self.call(|vtable, err| unsafe { (vtable.copy)(self.handle.instance, from, to, err) })
    }

    async fn symlink(&self, target: &Path, link: &Path) -> BashResult<()> {
        let target = str_ref_from_path(target)?;
        let link = str_ref_from_path(link)?;
        self.call(|vtable, err| unsafe {
            (vtable.symlink)(self.handle.instance, target, link, err)
        })
    }

    async fn read_link(&self, path: &Path) -> BashResult<PathBuf> {
        let path = str_ref_from_path(path)?;
        let mut out = BashkitFsAbiOwnedBytes::default();
        self.call(|vtable, err| unsafe {
            (vtable.read_link)(self.handle.instance, path, &mut out, err)
        })?;
        let bytes = self.take_bytes(out)?;
        let text = String::from_utf8(bytes)
            .map_err(|e| IoError::new(ErrorKind::InvalidData, e.to_string()))?;
        Ok(PathBuf::from(text))
    }

    async fn chmod(&self, path: &Path, mode: u32) -> BashResult<()> {
        let path = str_ref_from_path(path)?;
        self.call(|vtable, err| unsafe { (vtable.chmod)(self.handle.instance, path, mode, err) })
    }
}

fn str_ref_from_path(path: &Path) -> io::Result<BashkitFsAbiStrRef> {
    let text = path
        .to_str()
        .ok_or_else(|| IoError::new(ErrorKind::InvalidInput, "path must be valid UTF-8"))?;
    Ok(BashkitFsAbiStrRef {
        ptr: text.as_ptr(),
        len: text.len(),
    })
}

fn str_ref_to_bytes(bytes: BashkitFsAbiStrRef) -> io::Result<&'static [u8]> {
    if bytes.len == 0 {
        return Ok(&[]);
    }
    if bytes.ptr.is_null() {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            "byte pointer must not be null when len > 0",
        ));
    }
    unsafe { Ok(slice::from_raw_parts(bytes.ptr, bytes.len)) }
}

fn owned_bytes_to_vec(bytes: BashkitFsAbiOwnedBytes) -> io::Result<Vec<u8>> {
    if bytes.len == 0 {
        return Ok(Vec::new());
    }
    if bytes.ptr.is_null() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            "owned byte pointer must not be null when len > 0",
        ));
    }
    unsafe { Ok(slice::from_raw_parts(bytes.ptr.cast_const(), bytes.len).to_vec()) }
}

fn path_buf_from_abi(path: BashkitFsAbiStrRef) -> io::Result<PathBuf> {
    let text = str::from_utf8(str_ref_to_bytes(path)?)
        .map_err(|e| IoError::new(ErrorKind::InvalidInput, e.to_string()))?;
    Ok(PathBuf::from(text))
}

fn abi_metadata_to_metadata(meta: BashkitFsAbiMetadata) -> io::Result<Metadata> {
    Ok(Metadata {
        file_type: match meta.file_type {
            BashkitFsAbiFileType::File => FileType::File,
            BashkitFsAbiFileType::Directory => FileType::Directory,
            BashkitFsAbiFileType::Symlink => FileType::Symlink,
            BashkitFsAbiFileType::Fifo => FileType::Fifo,
        },
        size: meta.size,
        mode: meta.mode,
        modified: time_from_parts(meta.modified_secs, meta.modified_nanos)?,
        created: time_from_parts(meta.created_secs, meta.created_nanos)?,
    })
}

fn metadata_to_abi(metadata: Metadata) -> io::Result<BashkitFsAbiMetadata> {
    let (modified_secs, modified_nanos) = time_to_parts(metadata.modified)?;
    let (created_secs, created_nanos) = time_to_parts(metadata.created)?;
    Ok(BashkitFsAbiMetadata {
        file_type: match metadata.file_type {
            FileType::File => BashkitFsAbiFileType::File,
            FileType::Directory => BashkitFsAbiFileType::Directory,
            FileType::Symlink => BashkitFsAbiFileType::Symlink,
            FileType::Fifo => BashkitFsAbiFileType::Fifo,
        },
        _reserved: [0; 7],
        size: metadata.size,
        mode: metadata.mode,
        modified_secs,
        modified_nanos,
        created_secs,
        created_nanos,
    })
}

fn time_to_parts(time: SystemTime) -> io::Result<(i64, u32)> {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => Ok((
            i64::try_from(duration.as_secs())
                .map_err(|_| IoError::new(ErrorKind::InvalidData, "timestamp out of range"))?,
            duration.subsec_nanos(),
        )),
        Err(err) => {
            let duration = err.duration();
            let secs = i64::try_from(duration.as_secs())
                .map_err(|_| IoError::new(ErrorKind::InvalidData, "timestamp out of range"))?;
            let nanos = duration.subsec_nanos();
            if nanos == 0 {
                Ok((
                    secs.checked_neg().ok_or_else(|| {
                        IoError::new(ErrorKind::InvalidData, "timestamp out of range")
                    })?,
                    0,
                ))
            } else {
                Ok((
                    secs.checked_add(1)
                        .and_then(|value| value.checked_neg())
                        .ok_or_else(|| {
                            IoError::new(ErrorKind::InvalidData, "timestamp out of range")
                        })?,
                    1_000_000_000 - nanos,
                ))
            }
        }
    }
}

fn time_from_parts(secs: i64, nanos: u32) -> io::Result<SystemTime> {
    if nanos >= 1_000_000_000 {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            "timestamp nanoseconds out of range",
        ));
    }
    if secs >= 0 {
        return UNIX_EPOCH
            .checked_add(Duration::new(secs as u64, nanos))
            .ok_or_else(|| IoError::new(ErrorKind::InvalidData, "timestamp out of range"));
    }
    let duration = if nanos == 0 {
        Duration::new(secs.unsigned_abs(), 0)
    } else {
        Duration::new(
            secs.unsigned_abs()
                .checked_sub(1)
                .ok_or_else(|| IoError::new(ErrorKind::InvalidData, "timestamp out of range"))?,
            1_000_000_000 - nanos,
        )
    };
    UNIX_EPOCH
        .checked_sub(duration)
        .ok_or_else(|| IoError::new(ErrorKind::InvalidData, "timestamp out of range"))
}

fn bytes_from_vec(bytes: Vec<u8>) -> BashkitFsAbiOwnedBytes {
    let boxed = bytes.into_boxed_slice();
    let len = boxed.len();
    let ptr = Box::into_raw(boxed) as *mut u8;
    BashkitFsAbiOwnedBytes { ptr, len }
}

unsafe fn free_owned_bytes(bytes: BashkitFsAbiOwnedBytes) {
    if bytes.ptr.is_null() {
        return;
    }
    let raw = ptr::slice_from_raw_parts_mut(bytes.ptr, bytes.len);
    unsafe {
        drop(Box::from_raw(raw));
    }
}

fn dir_entries_from_vec(entries: Vec<DirEntry>) -> io::Result<BashkitFsAbiOwnedDirEntries> {
    let mut raw_entries = Vec::with_capacity(entries.len());
    for entry in entries {
        raw_entries.push(BashkitFsAbiDirEntry {
            name: bytes_from_vec(entry.name.into_bytes()),
            metadata: metadata_to_abi(entry.metadata)?,
        });
    }
    let boxed = raw_entries.into_boxed_slice();
    let len = boxed.len();
    let ptr = Box::into_raw(boxed) as *mut BashkitFsAbiDirEntry;
    Ok(BashkitFsAbiOwnedDirEntries { ptr, len })
}

unsafe fn free_owned_dir_entries(entries: BashkitFsAbiOwnedDirEntries) {
    if entries.ptr.is_null() {
        return;
    }
    let raw = ptr::slice_from_raw_parts_mut(entries.ptr, entries.len);
    let boxed = unsafe { Box::from_raw(raw) };
    for entry in boxed.iter() {
        unsafe {
            free_owned_bytes(entry.name);
        }
    }
}

fn owned_dir_entries_to_vec(entries: BashkitFsAbiOwnedDirEntries) -> io::Result<Vec<DirEntry>> {
    if entries.len == 0 {
        return Ok(Vec::new());
    }
    if entries.ptr.is_null() {
        return Err(IoError::new(
            ErrorKind::InvalidData,
            "directory entries pointer must not be null when len > 0",
        ));
    }
    let slice = unsafe { slice::from_raw_parts(entries.ptr.cast_const(), entries.len) };
    let mut out = Vec::with_capacity(slice.len());
    for entry in slice {
        let name = String::from_utf8(owned_bytes_to_vec(entry.name)?)
            .map_err(|e| IoError::new(ErrorKind::InvalidData, e.to_string()))?;
        out.push(DirEntry {
            name,
            metadata: abi_metadata_to_metadata(entry.metadata)?,
        });
    }
    Ok(out)
}

fn io_kind_to_abi(kind: ErrorKind) -> BashkitFsAbiErrorKind {
    match kind {
        ErrorKind::NotFound => BashkitFsAbiErrorKind::NotFound,
        ErrorKind::AlreadyExists => BashkitFsAbiErrorKind::AlreadyExists,
        ErrorKind::PermissionDenied => BashkitFsAbiErrorKind::PermissionDenied,
        ErrorKind::InvalidInput | ErrorKind::InvalidData => BashkitFsAbiErrorKind::InvalidInput,
        ErrorKind::IsADirectory => BashkitFsAbiErrorKind::IsADirectory,
        ErrorKind::NotADirectory => BashkitFsAbiErrorKind::NotADirectory,
        ErrorKind::DirectoryNotEmpty => BashkitFsAbiErrorKind::DirectoryNotEmpty,
        ErrorKind::Unsupported => BashkitFsAbiErrorKind::Unsupported,
        _ => BashkitFsAbiErrorKind::Other,
    }
}

fn abi_kind_to_io(kind: BashkitFsAbiErrorKind) -> ErrorKind {
    match kind {
        BashkitFsAbiErrorKind::NotFound => ErrorKind::NotFound,
        BashkitFsAbiErrorKind::AlreadyExists => ErrorKind::AlreadyExists,
        BashkitFsAbiErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
        BashkitFsAbiErrorKind::InvalidInput => ErrorKind::InvalidInput,
        BashkitFsAbiErrorKind::IsADirectory => ErrorKind::IsADirectory,
        BashkitFsAbiErrorKind::NotADirectory => ErrorKind::NotADirectory,
        BashkitFsAbiErrorKind::DirectoryNotEmpty => ErrorKind::DirectoryNotEmpty,
        BashkitFsAbiErrorKind::Unsupported => ErrorKind::Unsupported,
        BashkitFsAbiErrorKind::Other => ErrorKind::Other,
    }
}

fn fill_abi_error(dst: *mut BashkitFsAbiError, err: &IoError) {
    if dst.is_null() {
        return;
    }
    let message = err.to_string();
    unsafe {
        ptr::write(
            dst,
            BashkitFsAbiError {
                kind: io_kind_to_abi(err.kind()),
                message: bytes_from_vec(message.into_bytes()),
            },
        );
    }
}

fn abi_error_to_io(
    vtable: &BashkitFsAbiVTableV1,
    instance: *const c_void,
    err: BashkitFsAbiError,
) -> IoError {
    let message = if err.message.len == 0 || err.message.ptr.is_null() {
        String::new()
    } else {
        unsafe { String::from_utf8_lossy(slice::from_raw_parts(err.message.ptr, err.message.len)) }
            .into_owned()
    };
    unsafe {
        (vtable.free_bytes)(instance, err.message);
    }
    IoError::new(abi_kind_to_io(err.kind), message)
}

fn bash_error_to_io(err: BashError) -> IoError {
    match err {
        BashError::Io(io) => io,
        BashError::Cancelled => IoError::new(ErrorKind::Interrupted, "execution cancelled"),
        other => IoError::other(other.to_string()),
    }
}

fn export_state<'a>(instance: *const c_void) -> &'a ExportState {
    unsafe { &*instance.cast::<ExportState>() }
}

unsafe extern "C" fn retain_export_state(instance: *const c_void) {
    unsafe {
        Arc::increment_strong_count(instance.cast::<ExportState>());
    }
}

unsafe extern "C" fn release_export_state(instance: *const c_void) {
    unsafe {
        Arc::decrement_strong_count(instance.cast::<ExportState>());
    }
}

fn check_out_ptr<T>(ptr: *mut T) -> io::Result<()> {
    if ptr.is_null() {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            "output pointer must not be null",
        ));
    }
    Ok(())
}

fn call0(
    instance: *const c_void,
    err: *mut BashkitFsAbiError,
    f: impl FnOnce(&ExportState) -> io::Result<()>,
) -> BashkitFsAbiStatus {
    match f(export_state(instance)) {
        Ok(()) => BASHKIT_FS_ABI_STATUS_OK,
        Err(io) => {
            fill_abi_error(err, &io);
            BASHKIT_FS_ABI_STATUS_ERR
        }
    }
}

fn call<T>(
    instance: *const c_void,
    err: *mut BashkitFsAbiError,
    f: impl FnOnce(&ExportState) -> io::Result<T>,
    out: impl FnOnce(T),
) -> BashkitFsAbiStatus {
    match f(export_state(instance)) {
        Ok(value) => {
            out(value);
            BASHKIT_FS_ABI_STATUS_OK
        }
        Err(io) => {
            fill_abi_error(err, &io);
            BASHKIT_FS_ABI_STATUS_ERR
        }
    }
}

unsafe extern "C" fn export_read_file(
    instance: *const c_void,
    path: BashkitFsAbiStrRef,
    out: *mut BashkitFsAbiOwnedBytes,
    err: *mut BashkitFsAbiError,
) -> BashkitFsAbiStatus {
    call(
        instance,
        err,
        |state| {
            check_out_ptr(out)?;
            let path = path_buf_from_abi(path)?;
            let fs = Arc::clone(&state.fs);
            let bytes = state.run(async move { fs.read_file(&path).await })?;
            Ok(bytes_from_vec(bytes))
        },
        |value| unsafe {
            ptr::write(out, value);
        },
    )
}

unsafe extern "C" fn export_write_file(
    instance: *const c_void,
    path: BashkitFsAbiStrRef,
    content: BashkitFsAbiStrRef,
    err: *mut BashkitFsAbiError,
) -> BashkitFsAbiStatus {
    call0(instance, err, |state| {
        let path = path_buf_from_abi(path)?;
        let content = str_ref_to_bytes(content)?.to_vec();
        let fs = Arc::clone(&state.fs);
        state.run(async move { fs.write_file(&path, &content).await })
    })
}

unsafe extern "C" fn export_append_file(
    instance: *const c_void,
    path: BashkitFsAbiStrRef,
    content: BashkitFsAbiStrRef,
    err: *mut BashkitFsAbiError,
) -> BashkitFsAbiStatus {
    call0(instance, err, |state| {
        let path = path_buf_from_abi(path)?;
        let content = str_ref_to_bytes(content)?.to_vec();
        let fs = Arc::clone(&state.fs);
        state.run(async move { fs.append_file(&path, &content).await })
    })
}

unsafe extern "C" fn export_mkdir(
    instance: *const c_void,
    path: BashkitFsAbiStrRef,
    recursive: bool,
    err: *mut BashkitFsAbiError,
) -> BashkitFsAbiStatus {
    call0(instance, err, |state| {
        let path = path_buf_from_abi(path)?;
        let fs = Arc::clone(&state.fs);
        state.run(async move { fs.mkdir(&path, recursive).await })
    })
}

unsafe extern "C" fn export_remove(
    instance: *const c_void,
    path: BashkitFsAbiStrRef,
    recursive: bool,
    err: *mut BashkitFsAbiError,
) -> BashkitFsAbiStatus {
    call0(instance, err, |state| {
        let path = path_buf_from_abi(path)?;
        let fs = Arc::clone(&state.fs);
        state.run(async move { fs.remove(&path, recursive).await })
    })
}

unsafe extern "C" fn export_stat(
    instance: *const c_void,
    path: BashkitFsAbiStrRef,
    out: *mut BashkitFsAbiMetadata,
    err: *mut BashkitFsAbiError,
) -> BashkitFsAbiStatus {
    call(
        instance,
        err,
        |state| {
            check_out_ptr(out)?;
            let path = path_buf_from_abi(path)?;
            let fs = Arc::clone(&state.fs);
            let metadata = state.run(async move { fs.stat(&path).await })?;
            metadata_to_abi(metadata)
        },
        |value| unsafe {
            ptr::write(out, value);
        },
    )
}

unsafe extern "C" fn export_read_dir(
    instance: *const c_void,
    path: BashkitFsAbiStrRef,
    out: *mut BashkitFsAbiOwnedDirEntries,
    err: *mut BashkitFsAbiError,
) -> BashkitFsAbiStatus {
    call(
        instance,
        err,
        |state| {
            check_out_ptr(out)?;
            let path = path_buf_from_abi(path)?;
            let fs = Arc::clone(&state.fs);
            let entries = state.run(async move { fs.read_dir(&path).await })?;
            dir_entries_from_vec(entries)
        },
        |value| unsafe {
            ptr::write(out, value);
        },
    )
}

unsafe extern "C" fn export_exists(
    instance: *const c_void,
    path: BashkitFsAbiStrRef,
    out: *mut bool,
    err: *mut BashkitFsAbiError,
) -> BashkitFsAbiStatus {
    call(
        instance,
        err,
        |state| {
            check_out_ptr(out)?;
            let path = path_buf_from_abi(path)?;
            let fs = Arc::clone(&state.fs);
            state.run(async move { fs.exists(&path).await })
        },
        |value| unsafe {
            ptr::write(out, value);
        },
    )
}

unsafe extern "C" fn export_rename(
    instance: *const c_void,
    from: BashkitFsAbiStrRef,
    to: BashkitFsAbiStrRef,
    err: *mut BashkitFsAbiError,
) -> BashkitFsAbiStatus {
    call0(instance, err, |state| {
        let from = path_buf_from_abi(from)?;
        let to = path_buf_from_abi(to)?;
        let fs = Arc::clone(&state.fs);
        state.run(async move { fs.rename(&from, &to).await })
    })
}

unsafe extern "C" fn export_copy(
    instance: *const c_void,
    from: BashkitFsAbiStrRef,
    to: BashkitFsAbiStrRef,
    err: *mut BashkitFsAbiError,
) -> BashkitFsAbiStatus {
    call0(instance, err, |state| {
        let from = path_buf_from_abi(from)?;
        let to = path_buf_from_abi(to)?;
        let fs = Arc::clone(&state.fs);
        state.run(async move { fs.copy(&from, &to).await })
    })
}

unsafe extern "C" fn export_symlink(
    instance: *const c_void,
    target: BashkitFsAbiStrRef,
    link: BashkitFsAbiStrRef,
    err: *mut BashkitFsAbiError,
) -> BashkitFsAbiStatus {
    call0(instance, err, |state| {
        let target = path_buf_from_abi(target)?;
        let link = path_buf_from_abi(link)?;
        let fs = Arc::clone(&state.fs);
        state.run(async move { fs.symlink(&target, &link).await })
    })
}

unsafe extern "C" fn export_read_link(
    instance: *const c_void,
    path: BashkitFsAbiStrRef,
    out: *mut BashkitFsAbiOwnedBytes,
    err: *mut BashkitFsAbiError,
) -> BashkitFsAbiStatus {
    call(
        instance,
        err,
        |state| {
            check_out_ptr(out)?;
            let path = path_buf_from_abi(path)?;
            let fs = Arc::clone(&state.fs);
            let target = state.run(async move { fs.read_link(&path).await })?;
            Ok(bytes_from_vec(
                target.to_string_lossy().into_owned().into_bytes(),
            ))
        },
        |value| unsafe {
            ptr::write(out, value);
        },
    )
}

unsafe extern "C" fn export_chmod(
    instance: *const c_void,
    path: BashkitFsAbiStrRef,
    mode: u32,
    err: *mut BashkitFsAbiError,
) -> BashkitFsAbiStatus {
    call0(instance, err, |state| {
        let path = path_buf_from_abi(path)?;
        let fs = Arc::clone(&state.fs);
        state.run(async move { fs.chmod(&path, mode).await })
    })
}

unsafe extern "C" fn export_free_bytes(_instance: *const c_void, bytes: BashkitFsAbiOwnedBytes) {
    unsafe {
        free_owned_bytes(bytes);
    }
}

unsafe extern "C" fn export_free_dir_entries(
    _instance: *const c_void,
    entries: BashkitFsAbiOwnedDirEntries,
) {
    unsafe {
        free_owned_dir_entries(entries);
    }
}

static EXPORT_VTABLE: BashkitFsAbiVTableV1 = BashkitFsAbiVTableV1 {
    read_file: export_read_file,
    write_file: export_write_file,
    append_file: export_append_file,
    mkdir: export_mkdir,
    remove: export_remove,
    stat: export_stat,
    read_dir: export_read_dir,
    exists: export_exists,
    rename: export_rename,
    copy: export_copy,
    symlink: export_symlink,
    read_link: export_read_link,
    chmod: export_chmod,
    free_bytes: export_free_bytes,
    free_dir_entries: export_free_dir_entries,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemoryFs;

    #[test]
    fn export_import_roundtrip() {
        let rt = Builder::new_current_thread().enable_all().build().unwrap();
        let source: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        rt.block_on(async {
            source.mkdir(Path::new("/org/repo"), true).await.unwrap();
            source
                .write_file(Path::new("/org/repo/README.md"), b"interop\n")
                .await
                .unwrap();
        });

        let exported = export_filesystem(source).unwrap();
        let imported = import_owned_filesystem(&exported).unwrap();

        let bytes = rt
            .block_on(async { imported.read_file(Path::new("/org/repo/README.md")).await })
            .unwrap();
        assert_eq!(bytes, b"interop\n");
    }

    #[test]
    fn rejects_unknown_abi_version() {
        let handle = BashkitFsAbiHandleV1 {
            abi_version: 999,
            _reserved: 0,
            instance: ptr::null(),
            retain: retain_export_state,
            release: release_export_state,
            vtable: ptr::null(),
        };
        let err = match ImportedFileSystem::from_handle(&handle) {
            Ok(_) => panic!("expected ABI version check to fail"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), ErrorKind::InvalidData);
    }
}
