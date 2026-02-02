//! Archive builtins - tar, gzip, gunzip

use async_trait::async_trait;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{Read, Write};
use std::path::Path;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The tar builtin - create and extract tar archives.
///
/// Usage: tar [-c|-x|-t] [-v] [-f ARCHIVE] [FILE...]
///
/// Options:
///   -c   Create archive
///   -x   Extract archive
///   -t   List archive contents
///   -v   Verbose output
///   -f   Archive file name
///   -z   Filter through gzip (for .tar.gz)
pub struct Tar;

#[async_trait]
impl Builtin for Tar {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut create = false;
        let mut extract = false;
        let mut list = false;
        let mut verbose = false;
        let mut gzip = false;
        let mut archive_file: Option<String> = None;
        let mut files: Vec<String> = Vec::new();

        // Parse arguments
        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            if arg.starts_with('-') && !arg.starts_with("--") {
                for c in arg[1..].chars() {
                    match c {
                        'c' => create = true,
                        'x' => extract = true,
                        't' => list = true,
                        'v' => verbose = true,
                        'z' => gzip = true,
                        'f' => {
                            i += 1;
                            if i >= ctx.args.len() {
                                return Ok(ExecResult::err(
                                    "tar: option requires an argument -- 'f'\n".to_string(),
                                    2,
                                ));
                            }
                            archive_file = Some(ctx.args[i].clone());
                        }
                        _ => {
                            return Ok(ExecResult::err(
                                format!("tar: invalid option -- '{}'\n", c),
                                2,
                            ));
                        }
                    }
                }
            } else {
                files.push(arg.clone());
            }
            i += 1;
        }

        // Check for exactly one of -c, -x, -t
        let mode_count = [create, extract, list].iter().filter(|&&x| x).count();
        if mode_count == 0 {
            return Ok(ExecResult::err(
                "tar: You must specify one of -c, -x, or -t\n".to_string(),
                2,
            ));
        }
        if mode_count > 1 {
            return Ok(ExecResult::err(
                "tar: You may not specify more than one of -c, -x, -t\n".to_string(),
                2,
            ));
        }

        let archive_name = archive_file.unwrap_or_else(|| "-".to_string());

        if create {
            if files.is_empty() {
                return Ok(ExecResult::err(
                    "tar: Cowardly refusing to create an empty archive\n".to_string(),
                    2,
                ));
            }
            create_tar(&ctx, &archive_name, &files, verbose, gzip).await
        } else if extract {
            extract_tar(&ctx, &archive_name, verbose, gzip).await
        } else {
            list_tar(&ctx, &archive_name, verbose, gzip).await
        }
    }
}

/// Simple tar header (512 bytes)
const TAR_BLOCK_SIZE: usize = 512;

/// Create a tar archive
async fn create_tar(
    ctx: &Context<'_>,
    archive_name: &str,
    files: &[String],
    verbose: bool,
    gzip: bool,
) -> Result<ExecResult> {
    let mut output_data: Vec<u8> = Vec::new();
    let mut verbose_output = String::new();

    for file in files {
        let path = resolve_path(ctx.cwd, file);

        if !ctx.fs.exists(&path).await.unwrap_or(false) {
            return Ok(ExecResult::err(
                format!("tar: {}: Cannot stat: No such file or directory\n", file),
                2,
            ));
        }

        let metadata = ctx.fs.stat(&path).await?;

        if metadata.file_type.is_dir() {
            // Add directory recursively
            add_directory_to_tar(
                ctx,
                &path,
                file,
                &mut output_data,
                &mut verbose_output,
                verbose,
            )
            .await?;
        } else {
            // Add single file
            add_file_to_tar(
                ctx,
                &path,
                file,
                &mut output_data,
                &mut verbose_output,
                verbose,
            )
            .await?;
        }
    }

    // Add two zero blocks at end
    output_data.extend_from_slice(&[0u8; TAR_BLOCK_SIZE * 2]);

    // Compress if -z
    let final_data = if gzip {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&output_data).map_err(|e| {
            crate::error::Error::Execution(format!("tar: gzip compression failed: {}", e))
        })?;
        encoder.finish().map_err(|e| {
            crate::error::Error::Execution(format!("tar: gzip compression failed: {}", e))
        })?
    } else {
        output_data
    };

    // Write to file or stdout
    if archive_name == "-" {
        // Convert to lossy string for stdout
        return Ok(ExecResult {
            stdout: String::from_utf8_lossy(&final_data).to_string(),
            stderr: verbose_output,
            exit_code: 0,
            control_flow: crate::interpreter::ControlFlow::None,
        });
    }

    let archive_path = resolve_path(ctx.cwd, archive_name);
    ctx.fs.write_file(&archive_path, &final_data).await?;

    Ok(ExecResult {
        stdout: String::new(),
        stderr: verbose_output,
        exit_code: 0,
        control_flow: crate::interpreter::ControlFlow::None,
    })
}

/// Add a file to tar archive
async fn add_file_to_tar(
    ctx: &Context<'_>,
    path: &Path,
    name: &str,
    output: &mut Vec<u8>,
    verbose_output: &mut String,
    verbose: bool,
) -> Result<()> {
    let metadata = ctx.fs.stat(path).await?;
    let content = ctx.fs.read_file(path).await?;

    if verbose {
        verbose_output.push_str(name);
        verbose_output.push('\n');
    }

    // Create tar header
    let mut header = [0u8; TAR_BLOCK_SIZE];

    // Name (100 bytes)
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len().min(100);
    header[..name_len].copy_from_slice(&name_bytes[..name_len]);

    // Mode (8 bytes, octal)
    write_octal(&mut header[100..108], metadata.mode as u64, 7);

    // UID (8 bytes)
    write_octal(&mut header[108..116], 1000, 7);

    // GID (8 bytes)
    write_octal(&mut header[116..124], 1000, 7);

    // Size (12 bytes, octal)
    write_octal(&mut header[124..136], content.len() as u64, 11);

    // Mtime (12 bytes, octal)
    let mtime = metadata
        .modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    write_octal(&mut header[136..148], mtime, 11);

    // Checksum placeholder (8 bytes of spaces)
    header[148..156].copy_from_slice(b"        ");

    // Type flag
    header[156] = b'0'; // Regular file

    // Magic
    header[257..263].copy_from_slice(b"ustar ");

    // Version
    header[263..265].copy_from_slice(b" \0");

    // Calculate and write checksum
    let checksum: u32 = header.iter().map(|&b| b as u32).sum();
    write_octal(&mut header[148..156], checksum as u64, 7);

    output.extend_from_slice(&header);

    // Write file content with padding
    output.extend_from_slice(&content);
    let padding = (TAR_BLOCK_SIZE - (content.len() % TAR_BLOCK_SIZE)) % TAR_BLOCK_SIZE;
    output.extend(std::iter::repeat_n(0u8, padding));

    Ok(())
}

/// Add a directory to tar archive recursively
fn add_directory_to_tar<'a>(
    ctx: &'a Context<'_>,
    path: &'a Path,
    name: &'a str,
    output: &'a mut Vec<u8>,
    verbose_output: &'a mut String,
    verbose: bool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        // Add directory entry
        if verbose {
            verbose_output.push_str(name);
            verbose_output.push_str("/\n");
        }

        let metadata = ctx.fs.stat(path).await?;

        // Create tar header for directory
        let mut header = [0u8; TAR_BLOCK_SIZE];

        // Name with trailing slash
        let dir_name = format!("{}/", name);
        let name_bytes = dir_name.as_bytes();
        let name_len = name_bytes.len().min(100);
        header[..name_len].copy_from_slice(&name_bytes[..name_len]);

        // Mode
        write_octal(&mut header[100..108], metadata.mode as u64, 7);

        // UID/GID
        write_octal(&mut header[108..116], 1000, 7);
        write_octal(&mut header[116..124], 1000, 7);

        // Size (0 for directory)
        write_octal(&mut header[124..136], 0, 11);

        // Mtime
        let mtime = metadata
            .modified
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        write_octal(&mut header[136..148], mtime, 11);

        // Checksum placeholder
        header[148..156].copy_from_slice(b"        ");

        // Type flag for directory
        header[156] = b'5';

        // Magic
        header[257..263].copy_from_slice(b"ustar ");
        header[263..265].copy_from_slice(b" \0");

        // Calculate checksum
        let checksum: u32 = header.iter().map(|&b| b as u32).sum();
        write_octal(&mut header[148..156], checksum as u64, 7);

        output.extend_from_slice(&header);

        // Add directory contents
        let entries = ctx.fs.read_dir(path).await?;
        for entry in entries {
            let child_path = path.join(&entry.name);
            let child_name = format!("{}/{}", name, entry.name);

            if entry.metadata.file_type.is_dir() {
                add_directory_to_tar(
                    ctx,
                    &child_path,
                    &child_name,
                    output,
                    verbose_output,
                    verbose,
                )
                .await?;
            } else {
                add_file_to_tar(
                    ctx,
                    &child_path,
                    &child_name,
                    output,
                    verbose_output,
                    verbose,
                )
                .await?;
            }
        }

        Ok(())
    })
}

/// Write octal value to tar header field
fn write_octal(buf: &mut [u8], value: u64, width: usize) {
    let s = format!("{:0>width$o}", value, width = width);
    let bytes = s.as_bytes();
    let len = bytes.len().min(buf.len() - 1);
    buf[..len].copy_from_slice(&bytes[bytes.len() - len..]);
    buf[len] = 0;
}

/// Extract a tar archive
async fn extract_tar(
    ctx: &Context<'_>,
    archive_name: &str,
    verbose: bool,
    gzip: bool,
) -> Result<ExecResult> {
    let data = if archive_name == "-" {
        ctx.stdin.map(|s| s.as_bytes().to_vec()).unwrap_or_default()
    } else {
        let archive_path = resolve_path(ctx.cwd, archive_name);
        if !ctx.fs.exists(&archive_path).await.unwrap_or(false) {
            return Ok(ExecResult::err(
                format!(
                    "tar: {}: Cannot open: No such file or directory\n",
                    archive_name
                ),
                2,
            ));
        }
        ctx.fs.read_file(&archive_path).await?
    };

    // Decompress if -z
    let tar_data = if gzip {
        let mut decoder = GzDecoder::new(data.as_slice());
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).map_err(|e| {
            crate::error::Error::Execution(format!("tar: gzip decompression failed: {}", e))
        })?;
        decompressed
    } else {
        data
    };

    let mut verbose_output = String::new();
    let mut offset = 0;

    while offset + TAR_BLOCK_SIZE <= tar_data.len() {
        let header = &tar_data[offset..offset + TAR_BLOCK_SIZE];

        // Check for end of archive (two zero blocks)
        if header.iter().all(|&b| b == 0) {
            break;
        }

        // Parse name
        let name_end = header[..100].iter().position(|&b| b == 0).unwrap_or(100);
        let name = String::from_utf8_lossy(&header[..name_end]).to_string();

        if name.is_empty() {
            break;
        }

        // Parse size
        let size = parse_octal(&header[124..136]);

        // Parse type
        let type_flag = header[156];

        if verbose {
            verbose_output.push_str(&name);
            verbose_output.push('\n');
        }

        offset += TAR_BLOCK_SIZE;

        match type_flag {
            b'5' | b'\0' if name.ends_with('/') => {
                // Directory
                let dir_path = resolve_path(ctx.cwd, &name);
                ctx.fs.mkdir(&dir_path, true).await?;
            }
            b'0' | b'\0' => {
                // Regular file
                let content_blocks = size.div_ceil(TAR_BLOCK_SIZE);
                let content_end = offset + content_blocks * TAR_BLOCK_SIZE;

                if content_end > tar_data.len() {
                    return Ok(ExecResult::err(
                        format!("tar: {}: Unexpected end of archive\n", name),
                        2,
                    ));
                }

                let content = &tar_data[offset..offset + size];
                let file_path = resolve_path(ctx.cwd, &name);

                // Ensure parent directory exists
                if let Some(parent) = file_path.parent() {
                    ctx.fs.mkdir(parent, true).await?;
                }

                ctx.fs.write_file(&file_path, content).await?;
                offset = content_end;
            }
            _ => {
                // Skip unknown types
                let content_blocks = size.div_ceil(TAR_BLOCK_SIZE);
                offset += content_blocks * TAR_BLOCK_SIZE;
            }
        }
    }

    Ok(ExecResult {
        stdout: String::new(),
        stderr: verbose_output,
        exit_code: 0,
        control_flow: crate::interpreter::ControlFlow::None,
    })
}

/// List tar archive contents
async fn list_tar(
    ctx: &Context<'_>,
    archive_name: &str,
    verbose: bool,
    gzip: bool,
) -> Result<ExecResult> {
    let data = if archive_name == "-" {
        ctx.stdin.map(|s| s.as_bytes().to_vec()).unwrap_or_default()
    } else {
        let archive_path = resolve_path(ctx.cwd, archive_name);
        if !ctx.fs.exists(&archive_path).await.unwrap_or(false) {
            return Ok(ExecResult::err(
                format!(
                    "tar: {}: Cannot open: No such file or directory\n",
                    archive_name
                ),
                2,
            ));
        }
        ctx.fs.read_file(&archive_path).await?
    };

    // Decompress if -z
    let tar_data = if gzip {
        let mut decoder = GzDecoder::new(data.as_slice());
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).map_err(|e| {
            crate::error::Error::Execution(format!("tar: gzip decompression failed: {}", e))
        })?;
        decompressed
    } else {
        data
    };

    let mut output = String::new();
    let mut offset = 0;

    while offset + TAR_BLOCK_SIZE <= tar_data.len() {
        let header = &tar_data[offset..offset + TAR_BLOCK_SIZE];

        // Check for end of archive
        if header.iter().all(|&b| b == 0) {
            break;
        }

        // Parse name
        let name_end = header[..100].iter().position(|&b| b == 0).unwrap_or(100);
        let name = String::from_utf8_lossy(&header[..name_end]).to_string();

        if name.is_empty() {
            break;
        }

        // Parse size
        let size = parse_octal(&header[124..136]);

        if verbose {
            // Parse mode
            let mode = parse_octal(&header[100..108]) as u32;
            let size_val = parse_octal(&header[124..136]);

            let type_flag = header[156];
            let type_char = match type_flag {
                b'5' => 'd',
                b'2' => 'l',
                _ => '-',
            };

            output.push_str(&format!(
                "{}{}{}{}{}{}{}{}{}{} {:>8} {}\n",
                type_char,
                if mode & 0o400 != 0 { 'r' } else { '-' },
                if mode & 0o200 != 0 { 'w' } else { '-' },
                if mode & 0o100 != 0 { 'x' } else { '-' },
                if mode & 0o040 != 0 { 'r' } else { '-' },
                if mode & 0o020 != 0 { 'w' } else { '-' },
                if mode & 0o010 != 0 { 'x' } else { '-' },
                if mode & 0o004 != 0 { 'r' } else { '-' },
                if mode & 0o002 != 0 { 'w' } else { '-' },
                if mode & 0o001 != 0 { 'x' } else { '-' },
                size_val,
                name
            ));
        } else {
            output.push_str(&name);
            output.push('\n');
        }

        offset += TAR_BLOCK_SIZE;

        // Skip content blocks
        let content_blocks = size.div_ceil(TAR_BLOCK_SIZE);
        offset += content_blocks * TAR_BLOCK_SIZE;
    }

    Ok(ExecResult::ok(output))
}

/// Parse octal value from tar header field
fn parse_octal(buf: &[u8]) -> usize {
    let s: String = buf
        .iter()
        .take_while(|&&b| b != 0 && b != b' ')
        .map(|&b| b as char)
        .collect();
    usize::from_str_radix(s.trim(), 8).unwrap_or(0)
}

/// The gzip builtin - compress files.
///
/// Usage: gzip [-d] [-k] [-f] [FILE...]
///
/// Options:
///   -d   Decompress (same as gunzip)
///   -k   Keep original file
///   -f   Force overwrite
pub struct Gzip;

#[async_trait]
impl Builtin for Gzip {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut decompress = false;
        let mut keep = false;
        let mut force = false;
        let mut files: Vec<String> = Vec::new();

        for arg in ctx.args {
            if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        'd' => decompress = true,
                        'k' => keep = true,
                        'f' => force = true,
                        _ => {
                            return Ok(ExecResult::err(
                                format!("gzip: invalid option -- '{}'\n", c),
                                1,
                            ));
                        }
                    }
                }
            } else {
                files.push(arg.clone());
            }
        }

        // If no files, read from stdin
        if files.is_empty() {
            if let Some(stdin) = ctx.stdin {
                if decompress {
                    let mut decoder = GzDecoder::new(stdin.as_bytes());
                    let mut output = Vec::new();
                    match decoder.read_to_end(&mut output) {
                        Ok(_) => {
                            return Ok(ExecResult::ok(String::from_utf8_lossy(&output).to_string()))
                        }
                        Err(e) => return Ok(ExecResult::err(format!("gzip: stdin: {}\n", e), 1)),
                    }
                } else {
                    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                    encoder.write_all(stdin.as_bytes()).map_err(|e| {
                        crate::error::Error::Execution(format!("gzip: compression failed: {}", e))
                    })?;
                    let compressed = encoder.finish().map_err(|e| {
                        crate::error::Error::Execution(format!("gzip: compression failed: {}", e))
                    })?;
                    return Ok(ExecResult::ok(
                        String::from_utf8_lossy(&compressed).to_string(),
                    ));
                }
            }
            return Ok(ExecResult::ok(String::new()));
        }

        for file in &files {
            let path = resolve_path(ctx.cwd, file);

            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!("gzip: {}: No such file or directory\n", file),
                    1,
                ));
            }

            let metadata = ctx.fs.stat(&path).await?;
            if metadata.file_type.is_dir() {
                return Ok(ExecResult::err(
                    format!("gzip: {}: Is a directory\n", file),
                    1,
                ));
            }

            if decompress {
                // Decompress
                if !file.ends_with(".gz") {
                    return Ok(ExecResult::err(
                        format!("gzip: {}: unknown suffix -- ignored\n", file),
                        1,
                    ));
                }

                let output_name = file.strip_suffix(".gz").unwrap();
                let output_path = resolve_path(ctx.cwd, output_name);

                if ctx.fs.exists(&output_path).await.unwrap_or(false) && !force {
                    return Ok(ExecResult::err(
                        format!("gzip: {}: already exists\n", output_name),
                        1,
                    ));
                }

                let data = ctx.fs.read_file(&path).await?;
                let mut decoder = GzDecoder::new(data.as_slice());
                let mut output = Vec::new();
                decoder.read_to_end(&mut output).map_err(|e| {
                    crate::error::Error::Execution(format!("gzip: {}: {}", file, e))
                })?;

                ctx.fs.write_file(&output_path, &output).await?;

                if !keep {
                    ctx.fs.remove(&path, false).await?;
                }
            } else {
                // Compress
                if file.ends_with(".gz") {
                    return Ok(ExecResult::err(
                        format!("gzip: {}: already has .gz suffix\n", file),
                        1,
                    ));
                }

                let output_name = format!("{}.gz", file);
                let output_path = resolve_path(ctx.cwd, &output_name);

                if ctx.fs.exists(&output_path).await.unwrap_or(false) && !force {
                    return Ok(ExecResult::err(
                        format!("gzip: {}: already exists\n", output_name),
                        1,
                    ));
                }

                let data = ctx.fs.read_file(&path).await?;
                let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(&data).map_err(|e| {
                    crate::error::Error::Execution(format!("gzip: {}: {}", file, e))
                })?;
                let compressed = encoder.finish().map_err(|e| {
                    crate::error::Error::Execution(format!("gzip: {}: {}", file, e))
                })?;

                ctx.fs.write_file(&output_path, &compressed).await?;

                if !keep {
                    ctx.fs.remove(&path, false).await?;
                }
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// The gunzip builtin - decompress files.
///
/// Usage: gunzip [-k] [-f] [FILE...]
///
/// Options:
///   -k   Keep original file
///   -f   Force overwrite
pub struct Gunzip;

#[async_trait]
impl Builtin for Gunzip {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // gunzip is equivalent to gzip -d
        let mut modified_args: Vec<String> = vec!["-d".to_string()];
        modified_args.extend(ctx.args.iter().cloned());

        let new_ctx = Context {
            args: &modified_args,
            env: ctx.env,
            variables: ctx.variables,
            cwd: ctx.cwd,
            fs: ctx.fs,
            stdin: ctx.stdin,
        };

        Gzip.execute(new_ctx).await
    }
}

/// Resolve a path relative to cwd
fn resolve_path(cwd: &std::path::Path, path_str: &str) -> std::path::PathBuf {
    let path = Path::new(path_str);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn create_test_ctx() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();

        fs.mkdir(&cwd, true).await.unwrap();

        (fs, cwd, variables)
    }

    // ==================== tar tests ====================

    #[tokio::test]
    async fn test_tar_create_and_list() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create a test file
        fs.write_file(&cwd.join("test.txt"), b"Hello, world!")
            .await
            .unwrap();

        // Create archive
        let args = vec![
            "-cf".to_string(),
            "archive.tar".to_string(),
            "test.txt".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Tar.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("archive.tar")).await.unwrap());

        // List archive
        let args = vec!["-tf".to_string(), "archive.tar".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Tar.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_tar_create_and_extract() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create a test file
        fs.write_file(&cwd.join("original.txt"), b"Test content")
            .await
            .unwrap();

        // Create archive
        let args = vec![
            "-cf".to_string(),
            "archive.tar".to_string(),
            "original.txt".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Tar.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Remove original
        fs.remove(&cwd.join("original.txt"), false).await.unwrap();
        assert!(!fs.exists(&cwd.join("original.txt")).await.unwrap());

        // Extract
        let args = vec!["-xf".to_string(), "archive.tar".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Tar.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Verify extracted file
        assert!(fs.exists(&cwd.join("original.txt")).await.unwrap());
        let content = fs.read_file(&cwd.join("original.txt")).await.unwrap();
        assert_eq!(content, b"Test content");
    }

    #[tokio::test]
    async fn test_tar_verbose() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("test.txt"), b"content")
            .await
            .unwrap();

        let args = vec![
            "-cvf".to_string(),
            "archive.tar".to_string(),
            "test.txt".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Tar.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stderr.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_tar_missing_mode() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-f".to_string(), "archive.tar".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Tar.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("must specify"));
    }

    #[tokio::test]
    async fn test_tar_nonexistent_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec![
            "-cf".to_string(),
            "archive.tar".to_string(),
            "nonexistent".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Tar.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("No such file"));
    }

    #[tokio::test]
    async fn test_tar_directory() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.mkdir(&cwd.join("testdir"), false).await.unwrap();
        fs.write_file(&cwd.join("testdir/file.txt"), b"content")
            .await
            .unwrap();

        // Create archive with directory
        let args = vec![
            "-cf".to_string(),
            "archive.tar".to_string(),
            "testdir".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Tar.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        // List to verify
        let args = vec!["-tf".to_string(), "archive.tar".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Tar.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("testdir/"));
        assert!(result.stdout.contains("testdir/file.txt"));
    }

    // ==================== gzip tests ====================

    #[tokio::test]
    async fn test_gzip_compress() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("test.txt"), b"Hello, world!")
            .await
            .unwrap();

        let args = vec!["test.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Gzip.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("test.txt.gz")).await.unwrap());
        assert!(!fs.exists(&cwd.join("test.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_gzip_decompress() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create and compress
        fs.write_file(&cwd.join("test.txt"), b"Hello, world!")
            .await
            .unwrap();

        let args = vec!["test.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        Gzip.execute(ctx).await.unwrap();

        // Decompress
        let args = vec!["-d".to_string(), "test.txt.gz".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Gzip.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("test.txt")).await.unwrap());

        let content = fs.read_file(&cwd.join("test.txt")).await.unwrap();
        assert_eq!(content, b"Hello, world!");
    }

    #[tokio::test]
    async fn test_gzip_keep() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("test.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["-k".to_string(), "test.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Gzip.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("test.txt")).await.unwrap());
        assert!(fs.exists(&cwd.join("test.txt.gz")).await.unwrap());
    }

    #[tokio::test]
    async fn test_gzip_nonexistent() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["nonexistent".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Gzip.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("No such file"));
    }

    #[tokio::test]
    async fn test_gzip_already_gz() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("test.gz"), b"content")
            .await
            .unwrap();

        let args = vec!["test.gz".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Gzip.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("already has .gz"));
    }

    #[tokio::test]
    async fn test_gzip_directory() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.mkdir(&cwd.join("testdir"), false).await.unwrap();

        let args = vec!["testdir".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Gzip.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("Is a directory"));
    }

    // ==================== gunzip tests ====================

    #[tokio::test]
    async fn test_gunzip_basic() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create and compress
        fs.write_file(&cwd.join("test.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["test.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };
        Gzip.execute(ctx).await.unwrap();

        // Decompress with gunzip
        let args = vec!["test.txt.gz".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
        };

        let result = Gunzip.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("test.txt")).await.unwrap());
    }
}
