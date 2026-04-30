//! Downstream-style NAPI fixture that validates the public filesystem external ABI.

use bashkit::interop::fs::{BashkitFsAbiHandleV1, export_filesystem};
use bashkit::{
    DirEntry, FileSystem, FileSystemExt, FileType, Metadata, Result as BashResult, async_trait,
};
use napi::bindgen_prelude::{Buffer, JsObjectValue, Object};
use napi::{Env, Unknown, sys};
use napi_derive::napi;
use std::ffi::c_void;
use std::io::{Error as IoError, ErrorKind};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::slice;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

#[derive(Debug)]
struct SeededRandomFs {
    seed: u64,
}

impl SeededRandomFs {
    fn new(seed: u64) -> Self {
        Self { seed }
    }

    fn content_for(&self, path: &Path) -> Option<Vec<u8>> {
        let path = path.to_str()?;
        match path {
            "/README.md" => {
                Some(format!("seeded random filesystem seed={}\n", self.seed).into_bytes())
            }
            "/random.txt" => Some(expected_random_text_for(self.seed, path).into_bytes()),
            "/nested/data.txt" => {
                Some(format!("nested={:016x}\n", value_for(self.seed, path)).into_bytes())
            }
            _ => None,
        }
    }

    fn metadata_for(&self, path: &Path) -> Option<Metadata> {
        let file_type = match path.to_str()? {
            "/" | "/nested" => FileType::Directory,
            "/README.md" | "/random.txt" | "/nested/data.txt" => FileType::File,
            _ => return None,
        };
        let size = if file_type.is_file() {
            self.content_for(path)?.len() as u64
        } else {
            0
        };
        let timestamp = UNIX_EPOCH + Duration::from_secs(self.seed % 86_400);
        Some(Metadata {
            file_type,
            size,
            mode: if file_type.is_dir() { 0o755 } else { 0o644 },
            modified: timestamp,
            created: timestamp,
        })
    }

    fn dir_entry(&self, name: &str, path: &str) -> BashResult<DirEntry> {
        let metadata = self
            .metadata_for(Path::new(path))
            .ok_or_else(|| IoError::new(ErrorKind::NotFound, "entry not found"))?;
        Ok(DirEntry {
            name: name.to_string(),
            metadata,
        })
    }

    fn not_found() -> bashkit::Error {
        IoError::new(ErrorKind::NotFound, "path not found").into()
    }

    fn read_only() -> bashkit::Error {
        IoError::new(
            ErrorKind::PermissionDenied,
            "random filesystem is read-only",
        )
        .into()
    }
}

#[async_trait]
impl FileSystemExt for SeededRandomFs {}

#[async_trait]
impl FileSystem for SeededRandomFs {
    async fn read_file(&self, path: &Path) -> BashResult<Vec<u8>> {
        if self
            .metadata_for(path)
            .is_some_and(|metadata| metadata.file_type.is_dir())
        {
            return Err(IoError::new(ErrorKind::IsADirectory, "is a directory").into());
        }
        self.content_for(path).ok_or_else(Self::not_found)
    }

    async fn write_file(&self, _path: &Path, _content: &[u8]) -> BashResult<()> {
        Err(Self::read_only())
    }

    async fn append_file(&self, _path: &Path, _content: &[u8]) -> BashResult<()> {
        Err(Self::read_only())
    }

    async fn mkdir(&self, _path: &Path, _recursive: bool) -> BashResult<()> {
        Err(Self::read_only())
    }

    async fn remove(&self, _path: &Path, _recursive: bool) -> BashResult<()> {
        Err(Self::read_only())
    }

    async fn stat(&self, path: &Path) -> BashResult<Metadata> {
        self.metadata_for(path).ok_or_else(Self::not_found)
    }

    async fn read_dir(&self, path: &Path) -> BashResult<Vec<DirEntry>> {
        match path.to_str() {
            Some("/") => Ok(vec![
                self.dir_entry("README.md", "/README.md")?,
                self.dir_entry("nested", "/nested")?,
                self.dir_entry("random.txt", "/random.txt")?,
            ]),
            Some("/nested") => Ok(vec![self.dir_entry("data.txt", "/nested/data.txt")?]),
            Some(_) if self.metadata_for(path).is_some() => {
                Err(IoError::new(ErrorKind::NotADirectory, "not a directory").into())
            }
            _ => Err(Self::not_found()),
        }
    }

    async fn exists(&self, path: &Path) -> BashResult<bool> {
        Ok(self.metadata_for(path).is_some())
    }

    async fn rename(&self, _from: &Path, _to: &Path) -> BashResult<()> {
        Err(Self::read_only())
    }

    async fn copy(&self, _from: &Path, _to: &Path) -> BashResult<()> {
        Err(Self::read_only())
    }

    async fn symlink(&self, _target: &Path, _link: &Path) -> BashResult<()> {
        Err(Self::read_only())
    }

    async fn read_link(&self, _path: &Path) -> BashResult<PathBuf> {
        Err(IoError::new(ErrorKind::InvalidInput, "not a symlink").into())
    }

    async fn chmod(&self, _path: &Path, _mode: u32) -> BashResult<()> {
        Err(Self::read_only())
    }
}

fn value_for(seed: u64, path: &str) -> u64 {
    let mut value = seed ^ 0x9e37_79b9_7f4a_7c15;
    for byte in path.as_bytes() {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value ^= value >> 27;
    }
    value
}

fn expected_random_text_for(seed: u64, path: &str) -> String {
    format!(
        "seed={seed}\npath={path}\nvalue={:016x}\n",
        value_for(seed, path)
    )
}

fn seed_from_i64(seed: i64) -> napi::Result<u64> {
    u64::try_from(seed).map_err(|_| napi::Error::from_reason("seed must be non-negative"))
}

fn encode_file_system_handle(handle: &BashkitFsAbiHandleV1) -> Vec<u8> {
    unsafe {
        slice::from_raw_parts(
            (handle as *const BashkitFsAbiHandleV1).cast::<u8>(),
            size_of::<BashkitFsAbiHandleV1>(),
        )
        .to_vec()
    }
}

unsafe extern "C" fn finalize_owned_file_system_handle(
    _env: sys::napi_env,
    data: *mut c_void,
    _hint: *mut c_void,
) {
    if !data.is_null() {
        unsafe {
            drop(Box::from_raw(
                data.cast::<bashkit::interop::fs::BashkitFsAbiOwnedHandleV1>(),
            ));
        }
    }
}

fn create_file_system_owner_external(
    env: &Env,
    handle: bashkit::interop::fs::BashkitFsAbiOwnedHandleV1,
) -> napi::Result<Unknown<'static>> {
    let raw_handle = Box::into_raw(Box::new(handle));
    let mut raw_owner = std::ptr::null_mut();
    let status = unsafe {
        sys::napi_create_external(
            env.raw(),
            raw_handle.cast::<c_void>(),
            Some(finalize_owned_file_system_handle),
            std::ptr::null_mut(),
            &mut raw_owner,
        )
    };
    if status != sys::Status::napi_ok {
        unsafe {
            drop(Box::from_raw(raw_handle));
        }
        return Err(napi::Error::from_reason(format!(
            "create filesystem owner external failed with napi status {status}"
        )));
    }
    Ok(unsafe { Unknown::from_raw_unchecked(env.raw(), raw_owner) })
}

#[napi(js_name = "expectedRandomText")]
pub fn expected_random_text(seed: i64, path: String) -> napi::Result<String> {
    Ok(expected_random_text_for(seed_from_i64(seed)?, &path))
}

#[napi(js_name = "createRandomFilesystemExternal")]
pub fn create_random_filesystem_external(
    env: Env,
    seed: Option<i64>,
) -> napi::Result<Object<'static>> {
    let seed = seed_from_i64(seed.unwrap_or(7))?;
    let fs: Arc<dyn FileSystem> = Arc::new(SeededRandomFs::new(seed));
    let handle = export_filesystem(fs).map_err(|err| napi::Error::from_reason(err.to_string()))?;
    let bytes = encode_file_system_handle(handle.as_handle());
    let owner = create_file_system_owner_external(&env, handle)?;
    let mut external = Object::new(&env)?;
    external.set_named_property("bytes", Buffer::from(bytes))?;
    external.set_named_property("owner", owner)?;
    Ok(external)
}
