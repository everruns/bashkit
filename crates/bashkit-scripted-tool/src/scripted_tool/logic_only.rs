//! The logic-only shell: the restricted shell `ScriptedTool` runs scripts in.
//!
//! Important decision: core has no named restricted mode. This module
//! assembles one from public building blocks: every [`ShellFeatures`] switch
//! off, a default-builtin allowlist, and a filesystem that rejects every
//! call. Bash stays a control-flow and data-transformation language;
//! registered tool commands and custom builtins are the only way out.

use bashkit::{BashBuilder, DirEntry, FileSystem, FileSystemExt, Metadata, Result, ShellFeatures};
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

type SystemTime = bashkit::time::SystemTime;

/// Restrict `builder` to the logic-only shell.
pub(crate) fn apply(builder: BashBuilder) -> BashBuilder {
    builder
        .fs(Arc::new(DisabledFs))
        // No synthetic /etc, /proc or /usr/bin over the disabled filesystem.
        .rootfs(false)
        .shell_features(ShellFeatures::none())
        .builtin_filter(builtin_allowed)
}

/// Default builtins kept in the logic-only shell: shell logic and
/// stdin-to-stdout transforms. Anything that reads or writes files is out.
pub(crate) fn builtin_allowed(name: &str) -> bool {
    matches!(
        name,
        // Core shell/data flow
        "echo"
            | "true"
            | "false"
            | "exit"
            | "break"
            | "continue"
            | "return"
            | "test"
            | "["
            | "printf"
            | "export"
            | "read"
            | "set"
            | "unset"
            | "shift"
            | "local"
            | ":"
            | "readonly"
            | "times"
            | "eval"
            // Text and data transforms that work from stdin
            | "grep"
            | "sed"
            | "awk"
            | "head"
            | "tail"
            | "sort"
            | "uniq"
            | "cut"
            | "tr"
            | "wc"
            | "nl"
            | "paste"
            | "column"
            | "comm"
            | "strings"
            | "tac"
            | "rev"
            | "fold"
            | "expand"
            | "unexpand"
            | "join"
            | "split"
            | "jq"
            | "seq"
            | "expr"
            | "bc"
            | "numfmt"
            // Shell state, introspection, and structured transforms
            | "env"
            | "printenv"
            | "type"
            | "which"
            | "hash"
            | "alias"
            | "unalias"
            | "trap"
            | "caller"
            | "mapfile"
            | "readarray"
            | "shopt"
            | "clear"
            | "envsubst"
            | "assert"
            | "log"
            | "retry"
            | "semver"
            | "verify"
            | "compgen"
            | "csv"
            | "help"
            | "iconv"
            | "json"
            | "parallel"
            | "template"
            | "tomlq"
            | "yq"
            | "timeout"
            | "xargs"
            | "wait"
    )
}

/// Filesystem that rejects every operation.
///
/// Important decision: the interpreter and builtin context require an
/// `Arc<dyn FileSystem>`, so the logic-only shell installs one whose every
/// real operation fails, and scripts cannot use a VFS as storage or input.
pub(crate) struct DisabledFs;

fn disabled_fs_error() -> bashkit::Error {
    IoError::new(ErrorKind::PermissionDenied, "filesystem access disabled").into()
}

#[async_trait::async_trait]
impl FileSystemExt for DisabledFs {}

#[async_trait::async_trait]
impl FileSystem for DisabledFs {
    async fn read_file(&self, _path: &Path) -> Result<Vec<u8>> {
        Err(disabled_fs_error())
    }

    async fn write_file(&self, _path: &Path, _content: &[u8]) -> Result<()> {
        Err(disabled_fs_error())
    }

    async fn append_file(&self, _path: &Path, _content: &[u8]) -> Result<()> {
        Err(disabled_fs_error())
    }

    async fn mkdir(&self, _path: &Path, _recursive: bool) -> Result<()> {
        Err(disabled_fs_error())
    }

    async fn remove(&self, _path: &Path, _recursive: bool) -> Result<()> {
        Err(disabled_fs_error())
    }

    async fn stat(&self, _path: &Path) -> Result<Metadata> {
        Err(disabled_fs_error())
    }

    async fn read_dir(&self, _path: &Path) -> Result<Vec<DirEntry>> {
        Err(disabled_fs_error())
    }

    async fn exists(&self, _path: &Path) -> Result<bool> {
        Ok(false)
    }

    async fn rename(&self, _from: &Path, _to: &Path) -> Result<()> {
        Err(disabled_fs_error())
    }

    async fn copy(&self, _from: &Path, _to: &Path) -> Result<()> {
        Err(disabled_fs_error())
    }

    async fn symlink(&self, _target: &Path, _link: &Path) -> Result<()> {
        Err(disabled_fs_error())
    }

    async fn read_link(&self, _path: &Path) -> Result<PathBuf> {
        Err(disabled_fs_error())
    }

    async fn chmod(&self, _path: &Path, _mode: u32) -> Result<()> {
        Err(disabled_fs_error())
    }

    async fn set_modified_time(&self, _path: &Path, _time: SystemTime) -> Result<()> {
        Err(disabled_fs_error())
    }
}
