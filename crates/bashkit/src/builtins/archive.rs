//! Archive builtins - tar, gzip, gunzip, bzip2, bunzip2, bzcat
//!
//! # Zip Bomb Protection
//!
//! All decompression operations check output size against filesystem limits
//! to prevent zip bomb attacks. Decompression is aborted early if the
//! output would exceed limits.
//!
//! Design decision: tar's first bare argv is the historical option bundle;
//! normalize it to the regular short-option path instead of maintaining a
//! second parser.
//!
//! bzip2 0.6 uses the maintained pure-Rust libbz2-rs backend by default. Its
//! fixed codec state plus bounded, pre-charged output avoids native-library
//! deployment differences while retaining standard bzip2 CRC validation.

// Uses expect() for verified safe unwraps (e.g., strip_suffix after ends_with check)
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;
use bzip2::Compression as BzCompression;
use bzip2::read::BzDecoder;
use bzip2::write::BzEncoder;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::limits::ARCHIVE_MAX_DECOMPRESSION_RATIO as MAX_DECOMPRESSION_RATIO;
use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::limits::{BudgetedBytes, BudgetedString, BudgetedVec, LimitExceeded};

fn archive_io_error(context: &str, error: std::io::Error) -> crate::error::Error {
    if let Some(limit) = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<LimitExceeded>())
    {
        return crate::error::Error::ResourceLimit(limit.clone());
    }
    crate::error::Error::Execution(format!("{context}: {error}"))
}

fn with_execution_budget<T>(
    ctx: &Context<'_>,
    use_budget: impl FnOnce(Option<&crate::limits::ExecutionBudget>) -> Result<T>,
) -> Result<T> {
    match ctx.execution_budget() {
        Some(budget) => budget
            .try_with(|budget| use_budget(Some(budget)))
            .map_err(|_| crate::error::Error::Cancelled)?,
        None => use_budget(None),
    }
}

fn budgeted_bytes(ctx: &Context<'_>) -> Result<BudgetedBytes> {
    with_execution_budget(ctx, |budget| BudgetedBytes::new(budget).map_err(Into::into))
}

fn budgeted_bytes_with_capacity(ctx: &Context<'_>, capacity: usize) -> Result<BudgetedBytes> {
    with_execution_budget(ctx, |budget| {
        BudgetedBytes::try_with_capacity(budget, capacity).map_err(Into::into)
    })
}

fn budgeted_string(ctx: &Context<'_>) -> Result<BudgetedString> {
    with_execution_budget(ctx, |budget| {
        BudgetedString::new(budget).map_err(Into::into)
    })
}

fn budgeted_vec<T>(ctx: &Context<'_>) -> Result<BudgetedVec<T>> {
    with_execution_budget(ctx, |budget| BudgetedVec::new(budget).map_err(Into::into))
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ArchiveCompression {
    #[default]
    None,
    Gzip,
    Bzip2,
}

impl ArchiveCompression {
    fn detect(data: &[u8]) -> Self {
        if data.starts_with(b"BZh") {
            Self::Bzip2
        } else if data.starts_with(&[0x1f, 0x8b]) {
            Self::Gzip
        } else {
            Self::None
        }
    }
}

/// Read from a decoder with size limits to prevent zip bombs.
///
/// Returns an error if the decompressed size exceeds the limit or
/// if the decompression ratio is suspiciously high.
fn read_with_limit<R: Read>(
    mut reader: R,
    compressed_size: usize,
    max_size: u64,
    budget: Option<&crate::limits::ExecutionBudget>,
) -> std::io::Result<BudgetedBytes> {
    let mut output = BudgetedBytes::new(budget).map_err(std::io::Error::other)?;
    let mut buffer = [0u8; 8192];

    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }

        // THREAT[TM-DOS-102]: Reject decoder growth before reserving or
        // copying attacker-controlled expansion into the output buffer.
        let next_len = output.len().checked_add(n).ok_or_else(|| {
            std::io::Error::other("decompressed size overflow (zip bomb protection)")
        })?;
        if u64::try_from(next_len).unwrap_or(u64::MAX) > max_size {
            return Err(std::io::Error::other(format!(
                "decompressed size exceeds {} byte limit (zip bomb protection)",
                max_size
            )));
        }

        // Check decompression ratio (zip bomb detection)
        let ratio_limit = compressed_size.saturating_mul(MAX_DECOMPRESSION_RATIO);
        if compressed_size > 0 && next_len > ratio_limit {
            return Err(std::io::Error::other(format!(
                "decompression ratio exceeds {}:1 (zip bomb protection)",
                MAX_DECOMPRESSION_RATIO
            )));
        }

        output
            .try_extend_from_slice(&buffer[..n])
            .map_err(std::io::Error::other)?;
    }

    Ok(output)
}

fn compress_bytes(
    compression: ArchiveCompression,
    input: &[u8],
    command: &str,
    budget: Option<&crate::limits::ExecutionBudget>,
) -> Result<BudgetedBytes> {
    match compression {
        ArchiveCompression::None => {
            let mut output = BudgetedBytes::try_with_capacity(budget, input.len())?;
            output.try_extend_from_slice(input)?;
            Ok(output)
        }
        ArchiveCompression::Gzip => {
            let sink = BudgetedBytes::new(budget)?;
            let mut encoder = GzEncoder::new(sink, Compression::default());
            encoder.write_all(input).map_err(|error| {
                archive_io_error(&format!("{command}: gzip compression failed"), error)
            })?;
            encoder.finish().map_err(|error| {
                archive_io_error(&format!("{command}: gzip compression failed"), error)
            })
        }
        ArchiveCompression::Bzip2 => {
            let sink = BudgetedBytes::new(budget)?;
            let mut encoder = BzEncoder::new(sink, BzCompression::best());
            encoder.write_all(input).map_err(|error| {
                archive_io_error(&format!("{command}: bzip2 compression failed"), error)
            })?;
            encoder.finish().map_err(|error| {
                archive_io_error(&format!("{command}: bzip2 compression failed"), error)
            })
        }
    }
}

fn decompress_bytes(
    compression: ArchiveCompression,
    input: &[u8],
    max_size: u64,
    command: &str,
    budget: Option<&crate::limits::ExecutionBudget>,
) -> Result<BudgetedBytes> {
    let decoded = match compression {
        ArchiveCompression::None => return compress_bytes(compression, input, command, budget),
        ArchiveCompression::Gzip => {
            read_with_limit(GzDecoder::new(input), input.len(), max_size, budget)
        }
        ArchiveCompression::Bzip2 => {
            read_with_limit(BzDecoder::new(input), input.len(), max_size, budget)
        }
    };
    decoded.map_err(|error| archive_io_error(command, error))
}

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
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: tar [OPTION]... [FILE]...\nCreate, extract, or list tar archives.\n\n  -c\tcreate archive\n  -x\textract archive\n  -t\tlist archive contents\n  -v\tverbose output\n  -f ARCHIVE\tarchive file name\n  -z, --gzip\tfilter through gzip\n  -j, --bzip2\tfilter through bzip2\n  -C DIR\tchange to directory DIR\n  -O\textract files to stdout\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("tar (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let mut create = false;
        let mut extract = false;
        let mut list = false;
        let mut verbose = false;
        let mut compression = ArchiveCompression::None;
        let mut to_stdout = false;
        let mut archive_file: Option<String> = None;
        let mut change_dir: Option<String> = None;
        let mut files = budgeted_vec(&ctx)?;

        // GNU/bsdtar treat a bare first word as the historical option bundle.
        // Normalize only that argv position so the regular short-option parser
        // remains the single source of truth for flags and option arguments.
        let normalized_args;
        let args = if ctx
            .args
            .first()
            .is_some_and(|arg| !arg.starts_with('-') && !arg.starts_with('+'))
        {
            normalized_args = std::iter::once(format!("-{}", ctx.args[0]))
                .chain(ctx.args[1..].iter().cloned())
                .collect::<Vec<_>>();
            normalized_args.as_slice()
        } else {
            ctx.args
        };

        // Parse arguments
        let mut p = super::arg_parser::ArgParser::new(args);
        while !p.is_done() {
            if p.current() == Some("--bzip2") {
                compression = ArchiveCompression::Bzip2;
                p.advance();
            } else if p.current() == Some("--gzip") {
                compression = ArchiveCompression::Gzip;
                p.advance();
            } else if let Some(val) = p.flag_value_opt("-f") {
                archive_file = Some(val.to_string());
            } else if let Some(val) = p.flag_value_opt("-C") {
                change_dir = Some(val.to_string());
            } else if p.is_flag() {
                // Combined short flags like -czf where f/C take the next arg
                let arg = p.current().unwrap();
                let chars: Vec<char> = arg[1..].chars().collect();
                p.advance();
                for c in &chars {
                    match c {
                        'c' => create = true,
                        'x' => extract = true,
                        't' => list = true,
                        'v' => verbose = true,
                        'z' => compression = ArchiveCompression::Gzip,
                        'j' => compression = ArchiveCompression::Bzip2,
                        'O' => to_stdout = true,
                        'f' => match p.positional() {
                            Some(val) => archive_file = Some(val.to_string()),
                            None => {
                                return Ok(ExecResult::err(
                                    "tar: option requires an argument -- 'f'\n".to_string(),
                                    2,
                                ));
                            }
                        },
                        'C' => match p.positional() {
                            Some(val) => change_dir = Some(val.to_string()),
                            None => {
                                return Ok(ExecResult::err(
                                    "tar: option requires an argument -- 'C'\n".to_string(),
                                    2,
                                ));
                            }
                        },
                        _ => {
                            return Ok(ExecResult::err(
                                format!("tar: invalid option -- '{}'\n", c),
                                2,
                            ));
                        }
                    }
                }
            } else if let Some(arg) = p.positional() {
                files.try_push(arg)?;
            }
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
            create_tar(
                &ctx,
                &archive_name,
                &files,
                verbose,
                compression,
                change_dir.as_deref(),
            )
            .await
        } else if extract {
            extract_tar(
                &ctx,
                &archive_name,
                verbose,
                compression,
                change_dir.as_deref(),
                to_stdout,
            )
            .await
        } else {
            list_tar(&ctx, &archive_name, verbose, compression).await
        }
    }
}

/// Simple tar header (512 bytes)
const TAR_BLOCK_SIZE: usize = 512;

/// Create a tar archive
async fn create_tar(
    ctx: &Context<'_>,
    archive_name: &str,
    files: &[&str],
    verbose: bool,
    compression: ArchiveCompression,
    change_dir: Option<&str>,
) -> Result<ExecResult> {
    let mut output_data = budgeted_bytes(ctx)?;
    let mut verbose_output = budgeted_string(ctx)?;

    let base_dir = if let Some(dir) = change_dir {
        resolve_path(ctx.cwd, dir)
    } else {
        ctx.cwd.clone()
    };

    for file in files {
        let path = resolve_path(&base_dir, file);

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
    output_data.try_extend_from_slice(&[0u8; TAR_BLOCK_SIZE * 2])?;

    let final_data = if compression == ArchiveCompression::None {
        output_data
    } else {
        with_execution_budget(ctx, |budget| {
            compress_bytes(compression, &output_data, "tar", budget)
        })?
    };
    let (final_data, _final_lease) = final_data.into_parts();
    let (verbose_output, _verbose_lease) = verbose_output.into_parts();

    // Write to file or stdout
    if archive_name == "-" {
        return Ok(ExecResult {
            stdout: final_data.into(),
            stderr: verbose_output.into(),
            exit_code: 0,
            control_flow: crate::interpreter::ControlFlow::None,
            ..Default::default()
        });
    }

    let archive_path = resolve_path(ctx.cwd, archive_name);
    ctx.fs.write_file(&archive_path, &final_data).await?;

    Ok(ExecResult {
        stdout: crate::StreamData::new(),
        stderr: verbose_output.into(),
        exit_code: 0,
        control_flow: crate::interpreter::ControlFlow::None,
        ..Default::default()
    })
}

/// Add a file to tar archive
async fn add_file_to_tar(
    ctx: &Context<'_>,
    path: &Path,
    name: &str,
    output: &mut BudgetedBytes,
    verbose_output: &mut BudgetedString,
    verbose: bool,
) -> Result<()> {
    let metadata = ctx.fs.stat(path).await?;
    let content = ctx.fs.read_file(path).await?;
    ctx.consume_budget_input(content.len())?;
    ctx.consume_budget_work(u64::try_from(content.len().div_ceil(64)).unwrap_or(u64::MAX))?;

    if verbose {
        verbose_output.try_push_str(name)?;
        verbose_output.try_push('\n')?;
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
        .duration_since(crate::time_compat::UNIX_EPOCH)
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

    output.try_extend_from_slice(&header)?;

    // Write file content with padding
    output.try_extend_from_slice(&content)?;
    let padding = (TAR_BLOCK_SIZE - (content.len() % TAR_BLOCK_SIZE)) % TAR_BLOCK_SIZE;
    output.try_extend_from_slice(&[0u8; TAR_BLOCK_SIZE][..padding])?;

    Ok(())
}

/// Add a directory to tar archive recursively
fn add_directory_to_tar<'a>(
    ctx: &'a Context<'_>,
    path: &'a Path,
    name: &'a str,
    output: &'a mut BudgetedBytes,
    verbose_output: &'a mut BudgetedString,
    verbose: bool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        // Add directory entry
        if verbose {
            verbose_output.try_push_str(name)?;
            verbose_output.try_push_str("/\n")?;
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
            .duration_since(crate::time_compat::UNIX_EPOCH)
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

        output.try_extend_from_slice(&header)?;

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

// THREAT[TM-FS-015]: Validate the complete archive before the first VFS
// mutation. A late malformed or escaping entry must not leave earlier files
// behind as a misleading partial extraction.
fn validate_tar_for_extraction(
    tar_data: &[u8],
    extract_base: &Path,
    to_stdout: bool,
    limits: &crate::FsLimits,
) -> std::result::Result<(u64, u64, u64), String> {
    let mut offset = 0;
    let mut output_bytes = 0u64;
    let mut file_count = 0u64;
    let mut directories = HashSet::<PathBuf>::new();
    while offset + TAR_BLOCK_SIZE <= tar_data.len() {
        let header = &tar_data[offset..offset + TAR_BLOCK_SIZE];
        if header.iter().all(|&byte| byte == 0) {
            return Ok((output_bytes, file_count, directories.len() as u64));
        }

        let name_end = header[..100]
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(100);
        let name = String::from_utf8_lossy(&header[..name_end]);
        if name.is_empty() {
            return Ok((output_bytes, file_count, directories.len() as u64));
        }
        let size = parse_tar_size(&header[124..136])
            .ok_or_else(|| format!("tar: {name}: invalid size field\n"))?;
        offset += TAR_BLOCK_SIZE;
        let content_end = tar_content_end(offset, size)
            .filter(|end| *end <= tar_data.len())
            .ok_or_else(|| format!("tar: {name}: Unexpected end of archive\n"))?;

        if !to_stdout {
            let entry = Path::new(name.as_ref());
            if entry.is_absolute()
                || entry
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir))
                || !resolve_path(extract_base, name.as_ref()).starts_with(extract_base)
            {
                return Err(format!("tar: {name}: path traversal blocked\n"));
            }
            if matches!(header[156], b'0' | b'\0') && !name.ends_with('/') {
                limits
                    .check_file_size(size as u64)
                    .map_err(|error| format!("tar: {name}: {error}\n"))?;
                output_bytes = output_bytes.saturating_add(size as u64);
                file_count = file_count.saturating_add(1);
                let mut parent = entry.parent();
                while let Some(path) = parent {
                    if path.as_os_str().is_empty() {
                        break;
                    }
                    directories.insert(path.to_path_buf());
                    parent = path.parent();
                }
            } else if matches!(header[156], b'5' | b'\0') && name.ends_with('/') {
                directories.insert(entry.to_path_buf());
            }
        } else if matches!(header[156], b'0' | b'\0') && !name.ends_with('/') {
            limits
                .check_file_size(size as u64)
                .map_err(|error| format!("tar: {name}: {error}\n"))?;
            limits
                .check_total_bytes(output_bytes, size as u64)
                .map_err(|error| format!("tar: {name}: {error}\n"))?;
            output_bytes = output_bytes.saturating_add(size as u64);
        }
        offset = content_end;
    }

    Err("tar: Unexpected end of archive\n".to_string())
}

/// Extract a tar archive
async fn extract_tar(
    ctx: &Context<'_>,
    archive_name: &str,
    verbose: bool,
    compression: ArchiveCompression,
    change_dir: Option<&str>,
    to_stdout: bool,
) -> Result<ExecResult> {
    // Resolve extraction base directory (-C flag)
    let extract_base = if let Some(dir) = change_dir {
        resolve_path(ctx.cwd, dir)
    } else {
        ctx.cwd.clone()
    };
    let (data, data_lease) = if archive_name == "-" {
        let input = ctx.stdin_bytes().unwrap_or_default();
        let mut data = budgeted_bytes_with_capacity(ctx, input.len())?;
        data.try_extend_from_slice(input)?;
        data.into_parts()
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
        let data = ctx.fs.read_file(&archive_path).await?;
        let lease = ctx.lease_budget_bytes(data.len())?;
        (data, lease)
    };
    ctx.consume_budget_input(data.len())?;

    // Get filesystem limits for zip bomb protection
    let limits = ctx.fs.limits();
    let max_size = limits.max_total_bytes;

    let detected = ArchiveCompression::detect(&data);
    let compression = if compression == ArchiveCompression::None {
        detected
    } else {
        compression
    };
    let (tar_data, _tar_lease) = if compression == ArchiveCompression::None {
        (data, data_lease)
    } else {
        match with_execution_budget(ctx, |budget| {
            decompress_bytes(compression, &data, max_size, "tar", budget)
        }) {
            Ok(data) => data.into_parts(),
            Err(err) => return Ok(ExecResult::err(format!("{err}\n"), 2)),
        }
    };
    ctx.consume_budget_input(tar_data.len())?;
    ctx.consume_budget_work(u64::try_from(tar_data.len().div_ceil(64)).unwrap_or(u64::MAX))?;

    let mut verbose_output = budgeted_string(ctx)?;
    let mut stdout_output = budgeted_string(ctx)?;
    let mut offset = 0;

    let (planned_bytes, planned_files, planned_dirs) =
        match validate_tar_for_extraction(&tar_data, &extract_base, to_stdout, &limits) {
            Ok(planned) => planned,
            Err(stderr) => return Ok(ExecResult::err(stderr, 2)),
        };
    if !to_stdout {
        let usage = ctx.fs.usage();
        if let Err(error) = limits.check_total_bytes(usage.total_bytes, planned_bytes) {
            return Ok(ExecResult::err(format!("tar: {error}\n"), 2));
        }
        if let Err(error) =
            limits.check_final_file_count(usage.file_count.saturating_add(planned_files))
        {
            return Ok(ExecResult::err(format!("tar: {error}\n"), 2));
        }
        if planned_dirs > 0
            && let Err(error) = limits.check_dir_count(
                usage
                    .dir_count
                    .saturating_add(planned_dirs)
                    .saturating_sub(1),
            )
        {
            return Ok(ExecResult::err(format!("tar: {error}\n"), 2));
        }
    }

    // Ensure extract_base directory exists when -C is used
    if change_dir.is_some() {
        ctx.fs.mkdir(&extract_base, true).await?;
    }

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
        let size = match parse_tar_size(&header[124..136]) {
            Some(size) => size,
            None => {
                return Ok(ExecResult::err(
                    format!("tar: {}: invalid size field\n", name),
                    2,
                ));
            }
        };

        // Parse type
        let type_flag = header[156];

        if verbose {
            verbose_output.try_push_str(&name)?;
            verbose_output.try_push('\n')?;
        }

        offset += TAR_BLOCK_SIZE;

        // Path traversal protection: reject entries with ".." components or absolute paths
        if !to_stdout
            && (name.contains("..")
                || name.starts_with('/')
                || !resolve_path(&extract_base, &name).starts_with(&extract_base))
        {
            return Ok(ExecResult::err(
                format!("tar: {}: path traversal blocked\n", name),
                2,
            ));
        }

        match type_flag {
            b'5' | b'\0' if name.ends_with('/') => {
                // Directory
                if !to_stdout {
                    let dir_path = resolve_path(&extract_base, &name);
                    ctx.fs.mkdir(&dir_path, true).await?;
                }
            }
            b'0' | b'\0' => {
                // Regular file
                let Some(content_end) = tar_content_end(offset, size) else {
                    return Ok(ExecResult::err(
                        format!("tar: {}: invalid size field\n", name),
                        2,
                    ));
                };

                if content_end > tar_data.len() {
                    return Ok(ExecResult::err(
                        format!("tar: {}: Unexpected end of archive\n", name),
                        2,
                    ));
                }

                let content = &tar_data[offset..offset + size];

                if to_stdout {
                    if let Err(err) = limits.check_file_size(size as u64) {
                        return Ok(ExecResult::err(format!("tar: {name}: {err}\n"), 2));
                    }

                    let output = String::from_utf8_lossy(content);
                    if let Err(err) =
                        limits.check_total_bytes(stdout_output.len() as u64, output.len() as u64)
                    {
                        return Ok(ExecResult::err(format!("tar: {name}: {err}\n"), 2));
                    }

                    // -O: write content to stdout after applying VFS-sized quotas.
                    stdout_output.try_push_str(&output)?;
                } else {
                    let file_path = resolve_path(&extract_base, &name);

                    // Ensure parent directory exists
                    if let Some(parent) = file_path.parent() {
                        ctx.fs.mkdir(parent, true).await?;
                    }

                    ctx.fs.write_file(&file_path, content).await?;
                }
                offset = content_end;
            }
            b'1' => {
                // Hard link — not supported in VFS
                verbose_output.try_push_str(&format!(
                    "tar: {name}: hard link skipped (not supported in VFS)\n"
                ))?;
                let Some(content_end) = tar_content_end(offset, size) else {
                    return Ok(ExecResult::err(
                        format!("tar: {}: invalid size field\n", name),
                        2,
                    ));
                };
                offset = content_end;
            }
            b'2' => {
                // Symbolic link — not supported in VFS
                verbose_output.try_push_str(&format!(
                    "tar: {name}: symbolic link skipped (not supported in VFS)\n"
                ))?;
                let Some(content_end) = tar_content_end(offset, size) else {
                    return Ok(ExecResult::err(
                        format!("tar: {}: invalid size field\n", name),
                        2,
                    ));
                };
                offset = content_end;
            }
            _ => {
                // Other unsupported types (char/block device, FIFO, contiguous, etc.)
                verbose_output
                    .try_push_str(&format!("tar: {name}: unsupported entry type skipped\n"))?;
                let Some(content_end) = tar_content_end(offset, size) else {
                    return Ok(ExecResult::err(
                        format!("tar: {}: invalid size field\n", name),
                        2,
                    ));
                };
                offset = content_end;
            }
        }
    }

    let (stdout_output, _stdout_lease) = stdout_output.into_parts();
    let (verbose_output, _verbose_lease) = verbose_output.into_parts();
    Ok(ExecResult {
        stdout: stdout_output.into(),
        stderr: verbose_output.into(),
        exit_code: 0,
        control_flow: crate::interpreter::ControlFlow::None,
        ..Default::default()
    })
}

/// List tar archive contents
async fn list_tar(
    ctx: &Context<'_>,
    archive_name: &str,
    verbose: bool,
    compression: ArchiveCompression,
) -> Result<ExecResult> {
    let (data, data_lease) = if archive_name == "-" {
        let input = ctx.stdin_bytes().unwrap_or_default();
        let mut data = budgeted_bytes_with_capacity(ctx, input.len())?;
        data.try_extend_from_slice(input)?;
        data.into_parts()
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
        let data = ctx.fs.read_file(&archive_path).await?;
        let lease = ctx.lease_budget_bytes(data.len())?;
        (data, lease)
    };
    ctx.consume_budget_input(data.len())?;

    // Get filesystem limits for zip bomb protection
    let limits = ctx.fs.limits();
    let max_size = limits.max_total_bytes;

    let detected = ArchiveCompression::detect(&data);
    let compression = if compression == ArchiveCompression::None {
        detected
    } else {
        compression
    };
    let (tar_data, _tar_lease) = if compression == ArchiveCompression::None {
        (data, data_lease)
    } else {
        match with_execution_budget(ctx, |budget| {
            decompress_bytes(compression, &data, max_size, "tar", budget)
        }) {
            Ok(data) => data.into_parts(),
            Err(err) => return Ok(ExecResult::err(format!("{err}\n"), 2)),
        }
    };
    ctx.consume_budget_input(tar_data.len())?;
    ctx.consume_budget_work(u64::try_from(tar_data.len().div_ceil(64)).unwrap_or(u64::MAX))?;

    let mut output = budgeted_string(ctx)?;
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
        let size = match parse_tar_size(&header[124..136]) {
            Some(size) => size,
            None => {
                return Ok(ExecResult::err(
                    format!("tar: {}: invalid size field\n", name),
                    2,
                ));
            }
        };

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

            output.try_push_str(&format!(
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
            ))?;
        } else {
            output.try_push_str(&name)?;
            output.try_push('\n')?;
        }

        offset += TAR_BLOCK_SIZE;

        // Skip content blocks
        let Some(content_end) = tar_content_end(offset, size) else {
            return Ok(ExecResult::err(
                format!("tar: {}: invalid size field\n", name),
                2,
            ));
        };
        offset = content_end;
    }

    Ok(ExecResult::ok(output.into_inner()))
}

fn tar_content_end(offset: usize, size: usize) -> Option<usize> {
    let content_blocks = size.checked_add(TAR_BLOCK_SIZE - 1)? / TAR_BLOCK_SIZE;
    let content_len = content_blocks.checked_mul(TAR_BLOCK_SIZE)?;
    offset.checked_add(content_len)
}

fn parse_tar_size(buf: &[u8]) -> Option<usize> {
    parse_octal_u64(buf).and_then(|value| usize::try_from(value).ok())
}

fn parse_octal_u64(buf: &[u8]) -> Option<u64> {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let trimmed = std::str::from_utf8(&buf[..end]).ok()?.trim();
    if trimmed.is_empty() {
        return Some(0);
    }
    if !trimmed
        .as_bytes()
        .iter()
        .all(|byte| matches!(byte, b'0'..=b'7'))
    {
        return None;
    }
    u64::from_str_radix(trimmed, 8).ok()
}

/// Parse octal value from tar header field
fn parse_octal(buf: &[u8]) -> usize {
    parse_octal_u64(buf)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
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
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: gzip [OPTION]... [FILE]...\nCompress files.\n\n  -d\tdecompress\n  -k\tkeep original file\n  -f\tforce overwrite\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("gzip (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let mut decompress = false;
        let mut keep = false;
        let mut force = false;
        let mut files = budgeted_vec(&ctx)?;

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
                files.try_push(arg.as_str())?;
            }
        }

        // Get filesystem limits for zip bomb protection
        let limits = ctx.fs.limits();
        let max_size = limits.max_file_size;

        // If no files, read from stdin
        if files.is_empty() {
            if let Some(stdin) = ctx.stdin {
                if decompress {
                    let compressed_size = stdin.len();
                    let decoder = GzDecoder::new(stdin.as_bytes());
                    match with_execution_budget(&ctx, |budget| {
                        read_with_limit(decoder, compressed_size, max_size, budget)
                            .map_err(|error| archive_io_error("gzip: stdin", error))
                    }) {
                        Ok(output) => {
                            return Ok(ExecResult::ok(
                                String::from_utf8_lossy(&output).to_string(),
                            ));
                        }
                        Err(e) => return Ok(ExecResult::err(format!("gzip: stdin: {}\n", e), 1)),
                    }
                } else {
                    let sink = budgeted_bytes(&ctx)?;
                    let mut encoder = GzEncoder::new(sink, Compression::default());
                    encoder
                        .write_all(stdin.as_bytes())
                        .map_err(|error| archive_io_error("gzip: compression failed", error))?;
                    let compressed = encoder
                        .finish()
                        .map_err(|error| archive_io_error("gzip: compression failed", error))?;
                    return Ok(ExecResult::ok(
                        String::from_utf8_lossy(&compressed).to_string(),
                    ));
                }
            }
            return Ok(ExecResult::ok(String::new()));
        }

        for file in files.iter() {
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
                ctx.consume_budget_input(data.len())?;
                let _input_lease = ctx.lease_budget_bytes(data.len())?;
                let compressed_size = data.len();
                let decoder = GzDecoder::new(data.as_slice());
                let output = with_execution_budget(&ctx, |budget| {
                    read_with_limit(decoder, compressed_size, max_size, budget)
                        .map_err(|error| archive_io_error(&format!("gzip: {file}"), error))
                })?;
                ctx.consume_budget_input(output.len())?;
                ctx.consume_budget_work(
                    u64::try_from(output.len().div_ceil(64)).unwrap_or(u64::MAX),
                )?;

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
                ctx.consume_budget_input(data.len())?;
                let _input_lease = ctx.lease_budget_bytes(data.len())?;
                let sink = budgeted_bytes(&ctx)?;
                let mut encoder = GzEncoder::new(sink, Compression::default());
                encoder
                    .write_all(&data)
                    .map_err(|error| archive_io_error(&format!("gzip: {file}"), error))?;
                let compressed = encoder
                    .finish()
                    .map_err(|error| archive_io_error(&format!("gzip: {file}"), error))?;
                ctx.consume_budget_work(
                    u64::try_from(data.len().div_ceil(64)).unwrap_or(u64::MAX),
                )?;

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
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: gunzip [OPTION]... [FILE]...\nDecompress files.\n\n  -k\tkeep original file\n  -f\tforce overwrite\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("gunzip (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
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
            #[cfg(feature = "http_client")]
            http_client: ctx.http_client,
            #[cfg(feature = "git")]
            git_client: ctx.git_client,
            #[cfg(feature = "ssh")]
            ssh_client: ctx.ssh_client,
            shell: ctx.shell,
        };

        Gzip.execute(new_ctx).await
    }
}

/// The bzip2 family shares one bounded codec implementation.
pub struct Bzip2;
pub struct Bunzip2;
pub struct Bzcat;

#[derive(Clone, Copy)]
enum Bzip2Invocation {
    Compress,
    Decompress,
    DecompressStdout,
}

#[async_trait]
impl Builtin for Bzip2 {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        execute_bzip2(ctx, Bzip2Invocation::Compress, "bzip2").await
    }
}

#[async_trait]
impl Builtin for Bunzip2 {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        execute_bzip2(ctx, Bzip2Invocation::Decompress, "bunzip2").await
    }
}

#[async_trait]
impl Builtin for Bzcat {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        execute_bzip2(ctx, Bzip2Invocation::DecompressStdout, "bzcat").await
    }
}

async fn execute_bzip2(
    ctx: Context<'_>,
    invocation: Bzip2Invocation,
    command: &str,
) -> Result<ExecResult> {
    if let Some(result) = super::check_help_version(
        ctx.args,
        &format!(
            "Usage: {command} [OPTION]... [FILE]...\nCompress or decompress files with bzip2.\n\n  -c, --stdout\twrite to standard output\n  -d, --decompress\tdecompress\n  -z, --compress\tcompress\n  -k, --keep\tkeep input files\n  -f, --force\toverwrite output files\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n"
        ),
        Some(&format!("{command} (bashkit) 0.1")),
    ) {
        return Ok(result);
    }

    let mut decompress = !matches!(invocation, Bzip2Invocation::Compress);
    let mut stdout = matches!(invocation, Bzip2Invocation::DecompressStdout);
    let mut keep = stdout;
    let mut force = false;
    let mut files = budgeted_vec(&ctx)?;
    let mut parse_options = true;

    for arg in ctx.args {
        if parse_options && arg == "--" {
            parse_options = false;
        } else if parse_options && arg.starts_with("--") {
            match arg.as_str() {
                "--stdout" => stdout = true,
                "--decompress" => decompress = true,
                "--compress" => decompress = false,
                "--keep" => keep = true,
                "--force" => force = true,
                _ => {
                    return Ok(ExecResult::err(
                        format!("{command}: unrecognized option '{arg}'\n"),
                        1,
                    ));
                }
            }
        } else if parse_options && arg.starts_with('-') && arg != "-" {
            for flag in arg[1..].chars() {
                match flag {
                    'c' => stdout = true,
                    'd' => decompress = true,
                    'z' => decompress = false,
                    'k' => keep = true,
                    'f' => force = true,
                    _ => {
                        return Ok(ExecResult::err(
                            format!("{command}: invalid option -- '{flag}'\n"),
                            1,
                        ));
                    }
                }
            }
        } else {
            files.try_push(arg.as_str())?;
        }
    }

    if files.is_empty() {
        let input = ctx.stdin_bytes().unwrap_or_default();
        ctx.consume_budget_input(input.len())?;
        let _input_lease = ctx.lease_budget_bytes(input.len())?;
        let output = if decompress {
            match with_execution_budget(&ctx, |budget| {
                decompress_bytes(
                    ArchiveCompression::Bzip2,
                    input,
                    ctx.fs.limits().max_file_size,
                    command,
                    budget,
                )
            }) {
                Ok(output) => output,
                Err(err) => return Ok(ExecResult::err(format!("{err}\n"), 1)),
            }
        } else {
            with_execution_budget(&ctx, |budget| {
                compress_bytes(ArchiveCompression::Bzip2, input, command, budget)
            })?
        };
        ctx.consume_budget_input(output.len())?;
        ctx.consume_budget_work(u64::try_from(output.len().div_ceil(64)).unwrap_or(u64::MAX))?;
        let (output, _output_lease) = output.into_parts();
        return Ok(ExecResult {
            stdout: output.into(),
            ..ExecResult::default()
        });
    }

    let mut stdout_data = budgeted_bytes(&ctx)?;
    for file in files.iter() {
        let input_path = resolve_path(ctx.cwd, file);
        if !ctx.fs.exists(&input_path).await.unwrap_or(false) {
            return Ok(ExecResult::err(
                format!("{command}: {file}: No such file or directory\n"),
                1,
            ));
        }
        if ctx.fs.stat(&input_path).await?.file_type.is_dir() {
            return Ok(ExecResult::err(
                format!("{command}: {file}: Is a directory\n"),
                1,
            ));
        }

        let input = ctx.fs.read_file(&input_path).await?;
        ctx.consume_budget_input(input.len())?;
        let _input_lease = ctx.lease_budget_bytes(input.len())?;
        let output = if decompress {
            match with_execution_budget(&ctx, |budget| {
                decompress_bytes(
                    ArchiveCompression::Bzip2,
                    &input,
                    ctx.fs.limits().max_file_size,
                    command,
                    budget,
                )
            }) {
                Ok(output) => output,
                Err(err) => return Ok(ExecResult::err(format!("{err}\n"), 1)),
            }
        } else {
            if file.ends_with(".bz2") {
                return Ok(ExecResult::err(
                    format!("{command}: {file}: already has .bz2 suffix\n"),
                    1,
                ));
            }
            with_execution_budget(&ctx, |budget| {
                compress_bytes(ArchiveCompression::Bzip2, &input, command, budget)
            })?
        };
        ctx.consume_budget_input(output.len())?;
        ctx.consume_budget_work(u64::try_from(output.len().div_ceil(64)).unwrap_or(u64::MAX))?;
        if stdout {
            let next_len = stdout_data.len().checked_add(output.len()).ok_or_else(|| {
                crate::error::Error::Execution(format!("{command}: output size overflow"))
            })?;
            if let Err(err) = ctx.fs.limits().check_total_bytes(0, next_len as u64) {
                return Ok(ExecResult::err(format!("{command}: {err}\n"), 1));
            }
            stdout_data.try_extend_from_slice(&output)?;
            continue;
        }

        let output_name = if decompress {
            let Some(output_name) = decompressed_bzip2_name(file) else {
                return Ok(ExecResult::err(
                    format!("{command}: {file}: unknown suffix -- ignored\n"),
                    1,
                ));
            };
            output_name
        } else {
            format!("{file}.bz2")
        };
        let output_path = resolve_path(ctx.cwd, &output_name);
        if ctx.fs.exists(&output_path).await.unwrap_or(false) && !force {
            return Ok(ExecResult::err(
                format!("{command}: {output_name}: already exists\n"),
                1,
            ));
        }
        ctx.fs.write_file(&output_path, &output).await?;
        if !keep {
            ctx.fs.remove(&input_path, false).await?;
        }
    }

    let (stdout_data, _stdout_lease) = stdout_data.into_parts();
    Ok(ExecResult {
        stdout: stdout_data.into(),
        ..ExecResult::default()
    })
}

fn decompressed_bzip2_name(file: &str) -> Option<String> {
    if let Some(stem) = file.strip_suffix(".bz2") {
        Some(stem.to_string())
    } else if let Some(stem) = file.strip_suffix(".tbz2") {
        Some(format!("{stem}.tar"))
    } else {
        file.strip_suffix(".tbz").map(|stem| format!("{stem}.tar"))
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Tar.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_tar_bzip2_old_style_create_list_extract_roundtrip() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();
        fs.write_file(&cwd.join("payload.txt"), b"bzip2 archive payload")
            .await
            .unwrap();

        let create_args = vec![
            "cjf".to_string(),
            "archive.tbz2".to_string(),
            "payload.txt".to_string(),
        ];
        let create_ctx = Context::new_for_test(
            &create_args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            None,
        );
        let created = Tar.execute(create_ctx).await.unwrap();
        assert_eq!(created.exit_code, 0, "{}", created.stderr);

        let archive = fs.read_file(&cwd.join("archive.tbz2")).await.unwrap();
        assert!(archive.starts_with(b"BZh"));

        let list_args = vec![
            "--bzip2".to_string(),
            "-tf".to_string(),
            "archive.tbz2".to_string(),
        ];
        let list_ctx =
            Context::new_for_test(&list_args, &env, &mut variables, &mut cwd, fs.clone(), None);
        let listed = Tar.execute(list_ctx).await.unwrap();
        assert_eq!(listed.exit_code, 0, "{}", listed.stderr);
        assert_eq!(listed.stdout, "payload.txt\n");

        fs.remove(&cwd.join("payload.txt"), false).await.unwrap();
        let extract_args = vec!["-xjf".to_string(), "archive.tbz2".to_string()];
        let extract_ctx = Context::new_for_test(
            &extract_args,
            &env,
            &mut variables,
            &mut cwd,
            fs.clone(),
            None,
        );
        let extracted = Tar.execute(extract_ctx).await.unwrap();
        assert_eq!(extracted.exit_code, 0, "{}", extracted.stderr);
        assert_eq!(
            fs.read_file(&cwd.join("payload.txt")).await.unwrap(),
            b"bzip2 archive payload"
        );
    }

    #[tokio::test]
    async fn test_tar_rejects_unknown_old_style_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();
        let args = vec!["qf".to_string(), "archive.tar".to_string()];
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Tar.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert_eq!(result.stderr, "tar: invalid option -- 'q'\n");
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Gunzip.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(fs.exists(&cwd.join("test.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_bunzip2_invalid_stream_error_does_not_leak_debug_shapes() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();
        let args = vec!["-c".to_string()];
        let ctx = Context::new_for_test(
            &args,
            &env,
            &mut variables,
            &mut cwd,
            fs,
            Some("not a bzip2 stream"),
        );

        let result = Bunzip2.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        crate::builtins::debug_leak_check::assert_no_leak(&result, "bunzip2_invalid_stream", &[]);
    }

    // ==================== zip bomb protection tests ====================

    #[test]
    fn test_read_with_limit_normal() {
        let data = b"hello world";
        let result = read_with_limit(data.as_slice(), data.len(), 1000, None);
        assert!(result.is_ok());
        assert_eq!(&*result.unwrap(), data);
    }

    #[test]
    fn test_read_with_limit_exceeds_max() {
        let data = vec![0u8; 1000];
        let result = read_with_limit(data.as_slice(), data.len(), 500, None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("exceeds") || err.contains("limit"));
    }

    #[test]
    fn test_read_with_limit_high_ratio() {
        // Simulate a high decompression ratio scenario
        // We can't easily create a real gzip bomb, but we test the ratio check
        let data = vec![0u8; 10100]; // 101x the "compressed" size
        let result = read_with_limit(data.as_slice(), 100, u64::MAX, None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("ratio") || err.contains("bomb"));
    }

    #[test]
    fn test_read_with_limit_releases_budget_after_reader_error() {
        struct FailsAfterFirstRead(bool);

        impl Read for FailsAfterFirstRead {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                if self.0 {
                    return Err(std::io::Error::other("injected read failure"));
                }
                self.0 = true;
                buffer[..4].copy_from_slice(b"data");
                Ok(4)
            }
        }

        let limits = crate::limits::ExecutionLimits::new().max_live_intermediate_bytes(8);
        let budget = crate::limits::ExecutionBudget::new(
            &limits,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        let result = read_with_limit(FailsAfterFirstRead(false), 4, 8, Some(&budget));
        assert!(result.is_err());
        assert!(
            budget.lease_bytes(8).is_ok(),
            "reader errors must release buffer accounting"
        );
    }

    #[test]
    fn test_read_with_limit_aggregates_nested_live_work() {
        let limits = crate::limits::ExecutionLimits::new().max_live_intermediate_bytes(8);
        let budget = crate::limits::ExecutionBudget::new(
            &limits,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let outer = budget.lease_bytes(4).unwrap();

        let result = read_with_limit(&b"12345"[..], 5, 8, Some(&budget));
        let error = result.unwrap_err().to_string();
        assert!(error.contains("live intermediate bytes"), "{error}");
        drop(outer);
    }

    #[test]
    fn test_gzip_sink_preserves_resource_limit_error() {
        let limits = crate::limits::ExecutionLimits::new().max_live_intermediate_bytes(0);
        let budget = crate::limits::ExecutionBudget::new(
            &limits,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let sink = BudgetedBytes::new(Some(&budget)).unwrap();
        let mut encoder = GzEncoder::new(sink, Compression::default());

        let io_error = match encoder.write_all(b"untrusted input") {
            Err(error) => error,
            Ok(()) => encoder.finish().unwrap_err(),
        };
        assert!(matches!(
            archive_io_error("gzip", io_error),
            crate::error::Error::ResourceLimit(crate::limits::LimitExceeded::ExecutionBudget(_))
        ));
    }

    #[tokio::test]
    async fn test_gzip_respects_file_size_limit() {
        use crate::fs::FsLimits;

        // Create FS with small file size limit
        let limits = FsLimits::new().max_file_size(100);
        let fs = Arc::new(crate::fs::InMemoryFs::with_limits(limits));
        let mut cwd = PathBuf::from("/tmp");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        // Create a small file
        fs.write_file(&cwd.join("small.txt"), b"small content")
            .await
            .unwrap();

        // Compress it
        let args = vec!["small.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Gzip.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_fs_limits_enforced_on_extract() {
        use crate::fs::FsLimits;

        // Create FS with small limits
        let limits = FsLimits::new().max_total_bytes(1000).max_file_size(500);
        let fs = Arc::new(crate::fs::InMemoryFs::with_limits(limits));
        let cwd = PathBuf::from("/tmp");

        // Create a file that will exceed limits when extracted
        let large_content = vec![b'x'; 600];
        fs.write_file(&cwd.join("large.txt"), &large_content)
            .await
            .expect_err("Should fail due to file size limit");
    }

    // ==================== path traversal tests ====================

    /// Build a minimal tar archive with a single file entry.
    /// Used to craft malicious archives for security testing.
    fn build_tar_with_entry(name: &str, content: &[u8]) -> Vec<u8> {
        build_tar_with_fields(
            name,
            b'0',
            &format!("{:011o}\0", content.len()).into_bytes(),
            content,
        )
    }

    fn build_tar_with_fields(
        name: &str,
        type_flag: u8,
        size_field: &[u8],
        content: &[u8],
    ) -> Vec<u8> {
        let mut output = Vec::new();
        let mut header = [0u8; 512];

        // Name
        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len().min(100);
        header[..name_len].copy_from_slice(&name_bytes[..name_len]);

        // Mode
        let mode = b"0000644\0";
        header[100..108].copy_from_slice(mode);

        // UID/GID
        header[108..116].copy_from_slice(b"0001000\0");
        header[116..124].copy_from_slice(b"0001000\0");

        // Size (octal)
        let size_len = size_field.len().min(12);
        header[124..124 + size_len].copy_from_slice(&size_field[..size_len]);

        // Mtime
        header[136..148].copy_from_slice(b"00000000000\0");

        // Checksum placeholder (spaces)
        header[148..156].copy_from_slice(b"        ");

        // Type flag: regular file
        header[156] = type_flag;

        // Magic
        header[257..263].copy_from_slice(b"ustar ");
        header[263..265].copy_from_slice(b" \0");

        // Calculate checksum
        let checksum: u32 = header.iter().map(|&b| b as u32).sum();
        let cksum_str = format!("{:06o}\0 ", checksum);
        header[148..156].copy_from_slice(cksum_str.as_bytes());

        output.extend_from_slice(&header);
        output.extend_from_slice(content);
        let padding = (512 - (content.len() % 512)) % 512;
        output.extend(std::iter::repeat_n(0u8, padding));
        // End-of-archive marker (two zero blocks)
        output.extend_from_slice(&[0u8; 1024]);
        output
    }

    fn build_tar_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut output = Vec::new();
        for (name, content) in entries {
            let entry = build_tar_with_entry(name, content);
            output.extend_from_slice(&entry[..entry.len() - 1024]);
        }
        output.extend_from_slice(&[0u8; 1024]);
        output
    }

    #[tokio::test]
    async fn test_tar_extract_stdout_enforces_file_size_limit() {
        use crate::fs::FsLimits;

        let limits = FsLimits::new().max_file_size(10).max_total_bytes(10_000);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut cwd = PathBuf::from("/tmp");
        fs.mkdir(&cwd, true).await.unwrap();
        let env = HashMap::new();
        let mut variables = HashMap::new();

        let tar = build_tar_with_entry("large.txt", b"01234567890");
        let stdin = String::from_utf8(tar).unwrap();
        let args = vec!["-xOf".to_string(), "-".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, Some(&stdin));

        let result = Tar.execute(ctx).await.unwrap();

        assert_eq!(result.exit_code, 2);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.contains("file too large"));
    }

    #[tokio::test]
    async fn test_tar_extract_stdout_enforces_total_output_limit() {
        use crate::fs::FsLimits;

        let limits = FsLimits::new().max_file_size(10).max_total_bytes(15);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut cwd = PathBuf::from("/tmp");
        fs.mkdir(&cwd, true).await.unwrap();
        let env = HashMap::new();
        let mut variables = HashMap::new();

        let tar = build_tar_with_entries(&[("one.txt", b"1234567890"), ("two.txt", b"abcdef")]);
        let stdin = String::from_utf8(tar).unwrap();
        let args = vec!["-xOf".to_string(), "-".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, Some(&stdin));

        let result = Tar.execute(ctx).await.unwrap();

        assert_eq!(result.exit_code, 2);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.contains("filesystem full"));
    }

    #[tokio::test]
    async fn test_tar_extract_path_traversal_dotdot_blocked() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Craft a malicious tar with ../../../etc/passwd entry
        let malicious_tar = build_tar_with_entry("../../../etc/passwd", b"root:x:0:0");
        fs.write_file(&cwd.join("evil.tar"), &malicious_tar)
            .await
            .unwrap();

        let args = vec!["-xf".to_string(), "evil.tar".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Tar.execute(ctx).await.unwrap();

        // Should be blocked
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("path traversal blocked"));

        // /etc/passwd must NOT exist
        assert!(!fs.exists(&PathBuf::from("/etc/passwd")).await.unwrap());
    }

    #[tokio::test]
    async fn test_tar_extract_path_traversal_absolute_blocked() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Craft a malicious tar with absolute path entry
        let malicious_tar = build_tar_with_entry("/etc/shadow", b"root:!:19000");
        fs.write_file(&cwd.join("evil.tar"), &malicious_tar)
            .await
            .unwrap();

        let args = vec!["-xf".to_string(), "evil.tar".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Tar.execute(ctx).await.unwrap();

        // Should be blocked
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("path traversal blocked"));

        // /etc/shadow must NOT exist
        assert!(!fs.exists(&PathBuf::from("/etc/shadow")).await.unwrap());
    }

    #[tokio::test]
    async fn test_tar_extract_path_traversal_dir_dotdot_blocked() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Craft malicious tar with directory entry using ..
        let mut output = Vec::new();
        let mut header = [0u8; 512];
        let name = b"../../etc/";
        header[..name.len()].copy_from_slice(name);
        header[100..108].copy_from_slice(b"0000755\0");
        header[108..116].copy_from_slice(b"0001000\0");
        header[116..124].copy_from_slice(b"0001000\0");
        header[124..136].copy_from_slice(b"00000000000\0");
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].copy_from_slice(b"        ");
        header[156] = b'5'; // directory
        header[257..263].copy_from_slice(b"ustar ");
        header[263..265].copy_from_slice(b" \0");
        let checksum: u32 = header.iter().map(|&b| b as u32).sum();
        let cksum_str = format!("{:06o}\0 ", checksum);
        header[148..156].copy_from_slice(cksum_str.as_bytes());
        output.extend_from_slice(&header);
        output.extend_from_slice(&[0u8; 1024]);

        fs.write_file(&cwd.join("evil_dir.tar"), &output)
            .await
            .unwrap();

        let args = vec!["-xf".to_string(), "evil_dir.tar".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Tar.execute(ctx).await.unwrap();

        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("path traversal blocked"));
    }

    #[tokio::test]
    async fn test_tar_extract_safe_paths_still_work() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Normal tar with safe relative path should still work
        let safe_tar = build_tar_with_entry("subdir/file.txt", b"safe content");
        fs.write_file(&cwd.join("safe.tar"), &safe_tar)
            .await
            .unwrap();

        let args = vec!["-xf".to_string(), "safe.tar".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Tar.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        let content = fs.read_file(&cwd.join("subdir/file.txt")).await.unwrap();
        assert_eq!(content, b"safe content");
    }

    #[tokio::test]
    async fn test_tar_extract_rejects_invalid_size_field() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let invalid_tar = build_tar_with_fields("bad.txt", b'0', b"88888888888\0", b"payload");
        fs.write_file(&cwd.join("invalid-size.tar"), &invalid_tar)
            .await
            .unwrap();

        let args = vec!["-xf".to_string(), "invalid-size.tar".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Tar.execute(ctx).await.unwrap();

        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("invalid size field"));
        assert!(!fs.exists(&cwd.join("bad.txt")).await.unwrap());
    }

    #[tokio::test]
    async fn test_tar_list_rejects_invalid_size_field() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let invalid_tar = build_tar_with_fields("bad.txt", b'0', b"99999999999\0", b"payload");
        fs.write_file(&cwd.join("invalid-list.tar"), &invalid_tar)
            .await
            .unwrap();

        let args = vec!["-tf".to_string(), "invalid-list.tar".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Tar.execute(ctx).await.unwrap();

        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("invalid size field"));
    }

    /// Build a tar archive with a single entry of the given type flag.
    fn build_tar_with_typed_entry(name: &str, type_flag: u8) -> Vec<u8> {
        let mut output = Vec::new();
        let mut header = [0u8; 512];

        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len().min(100);
        header[..name_len].copy_from_slice(&name_bytes[..name_len]);

        header[100..108].copy_from_slice(b"0000644\0"); // mode
        header[108..116].copy_from_slice(b"0001000\0"); // uid
        header[116..124].copy_from_slice(b"0001000\0"); // gid
        header[124..136].copy_from_slice(b"00000000000\0"); // size = 0
        header[136..148].copy_from_slice(b"00000000000\0"); // mtime
        header[148..156].copy_from_slice(b"        "); // checksum placeholder
        header[156] = type_flag;
        header[257..263].copy_from_slice(b"ustar ");
        header[263..265].copy_from_slice(b" \0");

        let checksum: u32 = header.iter().map(|&b| b as u32).sum();
        let cksum_str = format!("{:06o}\0 ", checksum);
        header[148..156].copy_from_slice(cksum_str.as_bytes());

        output.extend_from_slice(&header);
        output.extend_from_slice(&[0u8; 1024]); // end-of-archive
        output
    }

    #[tokio::test]
    async fn test_tar_extract_symlink_warning() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let tar = build_tar_with_typed_entry("evil-link", b'2');
        fs.write_file(&cwd.join("symlink.tar"), &tar).await.unwrap();

        let args = vec!["-xf".to_string(), "symlink.tar".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Tar.execute(ctx).await.unwrap();

        // Extraction succeeds but warns on stderr
        assert_eq!(result.exit_code, 0);
        assert!(
            result
                .stderr
                .contains("symbolic link skipped (not supported in VFS)"),
            "expected symlink warning, got: {}",
            result.stderr,
        );

        // Symlink must NOT be created as a file
        assert!(!fs.exists(&cwd.join("evil-link")).await.unwrap());
    }

    #[tokio::test]
    async fn test_tar_extract_hardlink_warning() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let tar = build_tar_with_typed_entry("hard-link", b'1');
        fs.write_file(&cwd.join("hardlink.tar"), &tar)
            .await
            .unwrap();

        let args = vec!["-xf".to_string(), "hardlink.tar".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Tar.execute(ctx).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(
            result
                .stderr
                .contains("hard link skipped (not supported in VFS)"),
            "expected hard link warning, got: {}",
            result.stderr,
        );
        assert!(!fs.exists(&cwd.join("hard-link")).await.unwrap());
    }

    #[tokio::test]
    async fn test_tar_extract_unsupported_type_warning() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // b'3' = character device
        let tar = build_tar_with_typed_entry("chardev", b'3');
        fs.write_file(&cwd.join("chardev.tar"), &tar).await.unwrap();

        let args = vec!["-xf".to_string(), "chardev.tar".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Tar.execute(ctx).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(
            result.stderr.contains("unsupported entry type skipped"),
            "expected unsupported type warning, got: {}",
            result.stderr,
        );
        assert!(!fs.exists(&cwd.join("chardev")).await.unwrap());
    }
}
