//! zip/unzip - Archive files in a simple zip-like format
//!
//! Since we operate on a virtual filesystem without the `zip` crate,
//! this uses a simple custom binary format:
//!   Header: b"BKZIP\x01" (6 bytes)
//!   For each entry:
//!     - path_len: u32 LE
//!     - path: UTF-8 bytes
//!     - data_len: u32 LE
//!     - data: raw bytes
//!   Footer: b"BKEND" (5 bytes)
//!
//! Usage:
//!   zip archive.zip file1 file2...
//!   zip -r archive.zip dir/
//!   unzip archive.zip
//!   unzip -l archive.zip           # list contents
//!   unzip -d DIR archive.zip       # extract to directory
//!   unzip -o archive.zip           # overwrite existing

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

const MAGIC: &[u8] = b"BKZIP\x01";
const FOOTER: &[u8] = b"BKEND";

/// zip command - create zip archives
pub struct Zip;

/// unzip command - extract zip archives
pub struct Unzip;

struct ZipOptions {
    archive: String,
    files: Vec<String>,
    recursive: bool,
}

struct UnzipOptions {
    archive: String,
    list_only: bool,
    extract_dir: Option<String>,
    overwrite: bool,
}

fn parse_zip_args(args: &[String]) -> std::result::Result<ZipOptions, String> {
    let mut recursive = false;
    let mut positional = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-r" => recursive = true,
            _ if !arg.starts_with('-') => positional.push(arg.clone()),
            _ => {} // ignore unknown
        }
    }

    if positional.is_empty() {
        return Err("zip: missing archive name".to_string());
    }
    if positional.len() < 2 {
        return Err("zip: missing files to add".to_string());
    }

    let archive = positional.remove(0);
    Ok(ZipOptions {
        archive,
        files: positional,
        recursive,
    })
}

fn parse_unzip_args(args: &[String]) -> std::result::Result<UnzipOptions, String> {
    let mut list_only = false;
    let mut extract_dir = None;
    let mut overwrite = false;
    let mut positional = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-l" => list_only = true,
            "-o" => overwrite = true,
            "-d" => {
                i += 1;
                if i < args.len() {
                    extract_dir = Some(args[i].clone());
                } else {
                    return Err("unzip: -d requires a directory argument".to_string());
                }
            }
            _ if !args[i].starts_with('-') => positional.push(args[i].clone()),
            _ => {} // ignore unknown
        }
        i += 1;
    }

    if positional.is_empty() {
        return Err("unzip: missing archive name".to_string());
    }

    Ok(UnzipOptions {
        archive: positional.remove(0),
        list_only,
        extract_dir,
        overwrite,
    })
}

/// Entry in our simple archive format
struct ArchiveEntry {
    path: String,
    data: Vec<u8>,
}

/// Encode entries into our archive format
fn encode_archive(entries: &[ArchiveEntry]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC);

    for entry in entries {
        let path_bytes = entry.path.as_bytes();
        buf.extend_from_slice(&(path_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(path_bytes);
        buf.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
        buf.extend_from_slice(&entry.data);
    }

    buf.extend_from_slice(FOOTER);
    buf
}

/// Decode entries from our archive format
fn decode_archive(data: &[u8]) -> std::result::Result<Vec<ArchiveEntry>, String> {
    if data.len() < MAGIC.len() + FOOTER.len() {
        return Err("not a valid archive (too small)".to_string());
    }
    if &data[..MAGIC.len()] != MAGIC {
        return Err("not a valid archive (bad magic)".to_string());
    }
    if &data[data.len() - FOOTER.len()..] != FOOTER {
        return Err("not a valid archive (bad footer)".to_string());
    }

    let payload = &data[MAGIC.len()..data.len() - FOOTER.len()];
    let mut entries = Vec::new();
    let mut pos = 0;

    while pos < payload.len() {
        if pos + 4 > payload.len() {
            return Err("truncated archive (path length)".to_string());
        }
        let path_len = u32::from_le_bytes(
            payload[pos..pos + 4]
                .try_into()
                .map_err(|_| "bad path length bytes".to_string())?,
        ) as usize;
        pos += 4;

        if pos + path_len > payload.len() {
            return Err("truncated archive (path data)".to_string());
        }
        let path = String::from_utf8(payload[pos..pos + path_len].to_vec())
            .map_err(|_| "invalid UTF-8 in path".to_string())?;
        pos += path_len;

        if pos + 4 > payload.len() {
            return Err("truncated archive (data length)".to_string());
        }
        let data_len = u32::from_le_bytes(
            payload[pos..pos + 4]
                .try_into()
                .map_err(|_| "bad data length bytes".to_string())?,
        ) as usize;
        pos += 4;

        if pos + data_len > payload.len() {
            return Err("truncated archive (file data)".to_string());
        }
        let file_data = payload[pos..pos + data_len].to_vec();
        pos += data_len;

        entries.push(ArchiveEntry {
            path,
            data: file_data,
        });
    }

    Ok(entries)
}

/// Recursively collect files from directory
async fn collect_files_recursive(
    fs: &std::sync::Arc<dyn crate::fs::FileSystem>,
    dir: &std::path::Path,
    prefix: &str,
) -> Vec<(String, Vec<u8>)> {
    let mut result = Vec::new();
    let mut dirs = vec![(dir.to_path_buf(), prefix.to_string())];

    while let Some((current, current_prefix)) = dirs.pop() {
        if let Ok(entries) = fs.read_dir(&current).await {
            for entry in entries {
                let path = current.join(&entry.name);
                let entry_prefix = if current_prefix.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{}/{}", current_prefix, entry.name)
                };
                if entry.metadata.file_type.is_dir() {
                    dirs.push((path, entry_prefix));
                } else if entry.metadata.file_type.is_file()
                    && let Ok(data) = fs.read_file(&path).await
                {
                    result.push((entry_prefix, data));
                }
            }
        }
    }

    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

#[async_trait]
impl Builtin for Zip {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let opts = match parse_zip_args(ctx.args) {
            Ok(o) => o,
            Err(e) => return Ok(ExecResult::err(format!("{}\n", e), 1)),
        };

        let mut entries = Vec::new();
        let mut output = String::new();

        for file_arg in &opts.files {
            let path = resolve_path(ctx.cwd, file_arg);

            // Check if it's a directory
            if let Ok(meta) = ctx.fs.stat(&path).await
                && meta.file_type.is_dir()
            {
                if !opts.recursive {
                    return Ok(ExecResult::err(
                        format!("zip: {}: is a directory (use -r for recursive)\n", file_arg),
                        1,
                    ));
                }
                let dir_files = collect_files_recursive(&ctx.fs, &path, file_arg).await;
                for (rel_path, data) in dir_files {
                    output.push_str(&format!("  adding: {}\n", rel_path));
                    entries.push(ArchiveEntry {
                        path: rel_path,
                        data,
                    });
                }
                continue;
            }

            // It's a file
            match ctx.fs.read_file(&path).await {
                Ok(data) => {
                    output.push_str(&format!("  adding: {}\n", file_arg));
                    entries.push(ArchiveEntry {
                        path: file_arg.clone(),
                        data,
                    });
                }
                Err(e) => {
                    return Ok(ExecResult::err(format!("zip: {}: {}\n", file_arg, e), 1));
                }
            }
        }

        let archive_data = encode_archive(&entries);
        let archive_path = resolve_path(ctx.cwd, &opts.archive);
        ctx.fs.write_file(&archive_path, &archive_data).await?;

        Ok(ExecResult::ok(output))
    }
}

#[async_trait]
impl Builtin for Unzip {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let opts = match parse_unzip_args(ctx.args) {
            Ok(o) => o,
            Err(e) => return Ok(ExecResult::err(format!("{}\n", e), 1)),
        };

        let archive_path = resolve_path(ctx.cwd, &opts.archive);
        let archive_data = match ctx.fs.read_file(&archive_path).await {
            Ok(d) => d,
            Err(e) => {
                return Ok(ExecResult::err(
                    format!("unzip: {}: {}\n", opts.archive, e),
                    1,
                ));
            }
        };

        let entries = match decode_archive(&archive_data) {
            Ok(e) => e,
            Err(e) => {
                return Ok(ExecResult::err(
                    format!("unzip: {}: {}\n", opts.archive, e),
                    1,
                ));
            }
        };

        let mut output = String::new();

        if opts.list_only {
            output.push_str("  Length      Name\n");
            output.push_str("---------  ----------\n");
            let mut total_size = 0usize;
            for entry in &entries {
                output.push_str(&format!("{:>9}  {}\n", entry.data.len(), entry.path));
                total_size += entry.data.len();
            }
            output.push_str("---------  ----------\n");
            output.push_str(&format!("{:>9}  {} file(s)\n", total_size, entries.len()));
            return Ok(ExecResult::ok(output));
        }

        let extract_base = if let Some(ref dir) = opts.extract_dir {
            let dir_path = resolve_path(ctx.cwd, dir);
            // Create extraction directory
            ctx.fs.mkdir(&dir_path, true).await?;
            dir_path
        } else {
            ctx.cwd.clone()
        };

        for entry in &entries {
            // Strip leading '/' so Path::join doesn't discard the extract base
            let entry_path = entry.path.strip_prefix('/').unwrap_or(&entry.path);
            let target = extract_base.join(entry_path);

            // Check if file exists and overwrite not set
            if !opts.overwrite
                && let Ok(true) = ctx.fs.exists(&target).await
            {
                output.push_str(&format!(
                    "skipping: {} (already exists, use -o to overwrite)\n",
                    entry.path
                ));
                continue;
            }

            // Ensure parent directory exists
            if let Some(parent) = target.parent()
                && parent != std::path::Path::new("/")
                && parent != std::path::Path::new("")
            {
                ctx.fs.mkdir(parent, true).await?;
            }

            ctx.fs.write_file(&target, &entry.data).await?;
            output.push_str(&format!("  inflating: {}\n", entry.path));
        }

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFs};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    async fn run_zip(args: &[&str], files: &[(&str, &[u8])]) -> (ExecResult, Arc<InMemoryFs>) {
        let fs = Arc::new(InMemoryFs::new());
        let fs_trait = fs.clone() as Arc<dyn FileSystem>;
        for (path, content) in files {
            let p = Path::new(path);
            if let Some(parent) = p.parent()
                && parent != Path::new("/")
            {
                let _ = fs_trait.mkdir(parent, true).await;
            }
            fs_trait.write_file(p, content).await.unwrap();
        }

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs_trait,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Zip.execute(ctx).await.unwrap();
        (result, fs)
    }

    async fn run_unzip(args: &[&str], fs: Arc<InMemoryFs>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs_dyn = fs as Arc<dyn FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs_dyn,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        Unzip.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_zip_and_unzip_basic() {
        let (result, fs) = run_zip(
            &["/archive.zip", "/a.txt", "/b.txt"],
            &[("/a.txt", b"hello"), ("/b.txt", b"world")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("adding: /a.txt"));

        // Now extract to /out/
        let result = run_unzip(&["-d", "/out", "/archive.zip"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("inflating"));
    }

    #[tokio::test]
    async fn test_zip_missing_archive() {
        let fs = Arc::new(InMemoryFs::new());
        let fs_trait = fs.clone() as Arc<dyn FileSystem>;
        let args: Vec<String> = vec![];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs_trait,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };
        let result = Zip.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing archive"));
    }

    #[tokio::test]
    async fn test_zip_missing_files_arg() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let args = vec!["archive.zip".to_string()];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };
        let result = Zip.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing files"));
    }

    #[tokio::test]
    async fn test_zip_file_not_found() {
        let (result, _fs) = run_zip(&["/archive.zip", "/nonexistent.txt"], &[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("/nonexistent.txt"));
    }

    #[tokio::test]
    async fn test_zip_directory_without_recursive() {
        let (result, _fs) =
            run_zip(&["/archive.zip", "/dir"], &[("/dir/file.txt", b"content")]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("is a directory"));
    }

    #[tokio::test]
    async fn test_zip_recursive_directory() {
        let (result, fs) = run_zip(
            &["-r", "/archive.zip", "/dir"],
            &[("/dir/a.txt", b"aaa"), ("/dir/sub/b.txt", b"bbb")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("adding:"));

        // Verify archive exists
        let fs_trait = fs as Arc<dyn FileSystem>;
        assert!(fs_trait.exists(Path::new("/archive.zip")).await.unwrap());
    }

    #[tokio::test]
    async fn test_unzip_list() {
        let (_, fs) = run_zip(&["/archive.zip", "/a.txt"], &[("/a.txt", b"hello world")]).await;

        let result = run_unzip(&["-l", "/archive.zip"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/a.txt"));
        assert!(result.stdout.contains("11")); // "hello world" is 11 bytes
        assert!(result.stdout.contains("1 file(s)"));
    }

    #[tokio::test]
    async fn test_unzip_extract_dir() {
        let (_, fs) = run_zip(
            &["/archive.zip", "/a.txt", "/b.txt"],
            &[("/a.txt", b"aaa"), ("/b.txt", b"bbb")],
        )
        .await;

        let result = run_unzip(&["-d", "/extracted", "/archive.zip"], fs.clone()).await;
        assert_eq!(result.exit_code, 0);
        let fs_trait = fs as Arc<dyn FileSystem>;
        let content = fs_trait
            .read_file(Path::new("/extracted/a.txt"))
            .await
            .unwrap();
        assert_eq!(&content, b"aaa");
    }

    #[tokio::test]
    async fn test_unzip_skip_existing() {
        let (_, fs) = run_zip(&["/archive.zip", "/a.txt"], &[("/a.txt", b"archived")]).await;

        // Write a different file at the target location
        let fs_trait = fs.clone() as Arc<dyn FileSystem>;
        fs_trait
            .write_file(Path::new("/a.txt"), b"existing")
            .await
            .unwrap();

        let result = run_unzip(&["/archive.zip"], fs.clone()).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("skipping"));

        // Original file should be preserved
        let content = fs_trait.read_file(Path::new("/a.txt")).await.unwrap();
        assert_eq!(&content, b"existing");
    }

    #[tokio::test]
    async fn test_unzip_overwrite() {
        let (_, fs) = run_zip(&["/archive.zip", "/a.txt"], &[("/a.txt", b"archived")]).await;

        // Write different content
        let fs_trait = fs.clone() as Arc<dyn FileSystem>;
        fs_trait
            .write_file(Path::new("/a.txt"), b"existing")
            .await
            .unwrap();

        let result = run_unzip(&["-o", "/archive.zip"], fs.clone()).await;
        assert_eq!(result.exit_code, 0);

        let content = fs_trait.read_file(Path::new("/a.txt")).await.unwrap();
        assert_eq!(&content, b"archived");
    }

    #[tokio::test]
    async fn test_unzip_missing_archive() {
        let fs = Arc::new(InMemoryFs::new());
        let result = run_unzip(&["/nonexistent.zip"], fs).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("unzip:"));
    }

    #[tokio::test]
    async fn test_unzip_invalid_archive() {
        let fs = Arc::new(InMemoryFs::new());
        let fs_trait = fs.clone() as Arc<dyn FileSystem>;
        fs_trait
            .write_file(Path::new("/bad.zip"), b"not a zip file")
            .await
            .unwrap();
        let result = run_unzip(&["/bad.zip"], fs).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("not a valid archive"));
    }

    #[tokio::test]
    async fn test_encode_decode_roundtrip() {
        let entries = vec![
            ArchiveEntry {
                path: "hello.txt".to_string(),
                data: b"hello world".to_vec(),
            },
            ArchiveEntry {
                path: "dir/nested.txt".to_string(),
                data: b"nested content".to_vec(),
            },
        ];
        let encoded = encode_archive(&entries);
        let decoded = decode_archive(&encoded).unwrap();
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].path, "hello.txt");
        assert_eq!(decoded[0].data, b"hello world");
        assert_eq!(decoded[1].path, "dir/nested.txt");
        assert_eq!(decoded[1].data, b"nested content");
    }

    #[tokio::test]
    async fn test_decode_empty_data() {
        let result = decode_archive(b"");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unzip_no_args() {
        let fs = Arc::new(InMemoryFs::new());
        let fs_trait = fs.clone() as Arc<dyn FileSystem>;
        let args: Vec<String> = vec![];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs_trait,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };
        let result = Unzip.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing archive"));
    }
}
