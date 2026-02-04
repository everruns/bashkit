//! File inspection builtins - less, file, stat

use async_trait::async_trait;

use super::{resolve_path, Builtin, Context};
use crate::error::Result;
use crate::fs::FileType;
use crate::interpreter::ExecResult;

/// The less builtin - view file contents with paging.
///
/// Usage: less [FILE...]
///
/// In a sandboxed environment, this behaves like cat (no interactive paging).
/// The command still succeeds to allow scripts that use less to work.
pub struct Less;

#[async_trait]
impl Builtin for Less {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // less without args reads from stdin
        if ctx.args.is_empty() {
            if let Some(stdin) = ctx.stdin {
                return Ok(ExecResult::ok(stdin.to_string()));
            }
            return Ok(ExecResult::ok(String::new()));
        }

        let mut output = String::new();
        let files: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        if files.is_empty() {
            if let Some(stdin) = ctx.stdin {
                return Ok(ExecResult::ok(stdin.to_string()));
            }
            return Ok(ExecResult::ok(String::new()));
        }

        for (i, file) in files.iter().enumerate() {
            let path = resolve_path(ctx.cwd, file);

            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!("{}: No such file or directory\n", file),
                    1,
                ));
            }

            // Check if it's a directory
            let metadata = ctx.fs.stat(&path).await?;
            if metadata.file_type.is_dir() {
                return Ok(ExecResult::err(format!("{}: Is a directory\n", file), 1));
            }

            // Show file header if multiple files
            if files.len() > 1 {
                if i > 0 {
                    output.push('\n');
                }
                output.push_str(&format!("==> {} <==\n", file));
            }

            let content = ctx.fs.read_file(&path).await?;
            output.push_str(&String::from_utf8_lossy(&content));
        }

        Ok(ExecResult::ok(output))
    }
}

/// The file builtin - determine file type.
///
/// Usage: file FILE...
///
/// Reports the type of each file (regular file, directory, symlink, empty, text, binary).
pub struct File;

#[async_trait]
impl Builtin for File {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "file: missing file operand\n".to_string(),
                1,
            ));
        }

        let mut output = String::new();
        let files: Vec<_> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

        if files.is_empty() {
            return Ok(ExecResult::err(
                "file: missing file operand\n".to_string(),
                1,
            ));
        }

        for file in files {
            let path = resolve_path(ctx.cwd, file);

            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                output.push_str(&format!(
                    "{}: cannot open '{}' (No such file or directory)\n",
                    file, file
                ));
                continue;
            }

            let metadata = ctx.fs.stat(&path).await?;
            let file_type_str = match metadata.file_type {
                FileType::Directory => "directory".to_string(),
                FileType::Symlink => {
                    // Try to read the symlink target
                    match ctx.fs.read_link(&path).await {
                        Ok(target) => format!("symbolic link to {}", target.display()),
                        Err(_) => "symbolic link".to_string(),
                    }
                }
                FileType::File => {
                    if metadata.size == 0 {
                        "empty".to_string()
                    } else {
                        // Read some bytes to determine content type
                        let content = ctx.fs.read_file(&path).await?;
                        determine_file_content_type(&content)
                    }
                }
            };

            output.push_str(&format!("{}: {}\n", file, file_type_str));
        }

        Ok(ExecResult::ok(output))
    }
}

/// Determine content type from file bytes
fn determine_file_content_type(content: &[u8]) -> String {
    if content.is_empty() {
        return "empty".to_string();
    }

    // Check for common file signatures (magic bytes)
    if content.starts_with(b"\x7FELF") {
        return "ELF executable".to_string();
    }
    if content.starts_with(b"PK\x03\x04") {
        return "Zip archive".to_string();
    }
    if content.starts_with(b"\x1f\x8b") {
        return "gzip compressed data".to_string();
    }
    if content.starts_with(b"BZh") {
        return "bzip2 compressed data".to_string();
    }
    if content.starts_with(b"\xfd7zXZ\x00") {
        return "XZ compressed data".to_string();
    }
    if content.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "PNG image data".to_string();
    }
    if content.starts_with(b"\xff\xd8\xff") {
        return "JPEG image data".to_string();
    }
    if content.starts_with(b"GIF87a") || content.starts_with(b"GIF89a") {
        return "GIF image data".to_string();
    }
    if content.starts_with(b"%PDF") {
        return "PDF document".to_string();
    }
    if content.len() >= 4 && &content[0..4] == b"RIFF" {
        return "RIFF (little-endian) data".to_string();
    }

    // Check for shebang
    if content.starts_with(b"#!") {
        if let Ok(s) = std::str::from_utf8(content.get(0..64.min(content.len())).unwrap_or(b"")) {
            if let Some(line) = s.lines().next() {
                if line.contains("bash") || line.contains("/sh") {
                    return "Bourne-Again shell script".to_string();
                }
                if line.contains("python") {
                    return "Python script".to_string();
                }
                if line.contains("perl") {
                    return "Perl script".to_string();
                }
                if line.contains("ruby") {
                    return "Ruby script".to_string();
                }
                if line.contains("node") {
                    return "Node.js script".to_string();
                }
                return "script text executable".to_string();
            }
        }
    }

    // Check if content is valid UTF-8 text
    if std::str::from_utf8(content).is_ok() {
        // Check for common text formats
        let sample = std::str::from_utf8(&content[0..512.min(content.len())]).unwrap_or("");

        if sample.trim_start().starts_with('{') || sample.trim_start().starts_with('[') {
            return "JSON text".to_string();
        }
        if sample.contains("<?xml") || sample.starts_with('<') {
            return "XML text".to_string();
        }
        if sample.starts_with("<!DOCTYPE html") || sample.starts_with("<html") {
            return "HTML document".to_string();
        }

        return "ASCII text".to_string();
    }

    // Binary data
    "data".to_string()
}

/// The stat builtin - display file status.
///
/// Usage: stat [-c FORMAT] FILE...
///
/// Options:
///   -c FORMAT   Use specified format instead of default
///
/// Format sequences:
///   %n   File name
///   %s   Size in bytes
///   %a   Octal permissions
///   %A   Human-readable permissions
///   %F   File type
pub struct Stat;

#[async_trait]
impl Builtin for Stat {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut format: Option<String> = None;
        let mut files: Vec<&String> = Vec::new();

        // Parse arguments
        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            if arg == "-c" || arg == "--format" {
                i += 1;
                if i >= ctx.args.len() {
                    return Ok(ExecResult::err(
                        "stat: missing argument to '-c'\n".to_string(),
                        1,
                    ));
                }
                format = Some(ctx.args[i].clone());
            } else if let Some(fmt) = arg.strip_prefix("-c") {
                // -cFORMAT (no space)
                format = Some(fmt.to_string());
            } else if !arg.starts_with('-') {
                files.push(arg);
            }
            i += 1;
        }

        if files.is_empty() {
            return Ok(ExecResult::err("stat: missing operand\n".to_string(), 1));
        }

        let mut output = String::new();

        for file in files {
            let path = resolve_path(ctx.cwd, file);

            if !ctx.fs.exists(&path).await.unwrap_or(false) {
                return Ok(ExecResult::err(
                    format!("stat: cannot stat '{}': No such file or directory\n", file),
                    1,
                ));
            }

            let metadata = ctx.fs.stat(&path).await?;

            if let Some(fmt) = &format {
                output.push_str(&format_stat(file, &metadata, fmt));
                output.push('\n');
            } else {
                // Default format
                output.push_str(&default_stat_format(file, &metadata));
            }
        }

        Ok(ExecResult::ok(output))
    }
}

/// Format stat output using format string
fn format_stat(name: &str, metadata: &crate::fs::Metadata, format: &str) -> String {
    let mut result = String::new();
    let mut chars = format.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            if let Some(&next) = chars.peek() {
                chars.next();
                match next {
                    'n' => result.push_str(name),
                    's' => result.push_str(&metadata.size.to_string()),
                    'a' => result.push_str(&format!("{:o}", metadata.mode & 0o777)),
                    'A' => result.push_str(&format_permissions(metadata)),
                    'F' => result.push_str(&format_file_type(metadata.file_type)),
                    '%' => result.push('%'),
                    _ => {
                        result.push('%');
                        result.push(next);
                    }
                }
            } else {
                result.push('%');
            }
        } else if ch == '\\' {
            if let Some(&next) = chars.peek() {
                chars.next();
                match next {
                    'n' => result.push('\n'),
                    't' => result.push('\t'),
                    '\\' => result.push('\\'),
                    _ => {
                        result.push('\\');
                        result.push(next);
                    }
                }
            } else {
                result.push('\\');
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Format file type for stat
fn format_file_type(file_type: FileType) -> String {
    match file_type {
        FileType::File => "regular file".to_string(),
        FileType::Directory => "directory".to_string(),
        FileType::Symlink => "symbolic link".to_string(),
    }
}

/// Format permissions like ls -l
fn format_permissions(metadata: &crate::fs::Metadata) -> String {
    let file_type = match metadata.file_type {
        FileType::Directory => 'd',
        FileType::Symlink => 'l',
        FileType::File => '-',
    };

    let mode = metadata.mode;
    format!(
        "{}{}{}{}{}{}{}{}{}{}",
        file_type,
        if mode & 0o400 != 0 { 'r' } else { '-' },
        if mode & 0o200 != 0 { 'w' } else { '-' },
        if mode & 0o100 != 0 { 'x' } else { '-' },
        if mode & 0o040 != 0 { 'r' } else { '-' },
        if mode & 0o020 != 0 { 'w' } else { '-' },
        if mode & 0o010 != 0 { 'x' } else { '-' },
        if mode & 0o004 != 0 { 'r' } else { '-' },
        if mode & 0o002 != 0 { 'w' } else { '-' },
        if mode & 0o001 != 0 { 'x' } else { '-' },
    )
}

/// Default stat format
fn default_stat_format(name: &str, metadata: &crate::fs::Metadata) -> String {
    let modified = metadata
        .modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let access_str = format!(
        "{:04o}/{}",
        metadata.mode & 0o777,
        format_permissions(metadata)
    );

    format!(
        "  File: {}\n  Size: {}\t\tBlocks: {}\t{}\nAccess: ({})\nModify: {}\n",
        name,
        metadata.size,
        metadata.size.div_ceil(512),
        format_file_type(metadata.file_type),
        access_str,
        modified,
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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

    // ==================== less tests ====================

    #[tokio::test]
    async fn test_less_file() {
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
        };

        let result = Less.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Hello, world!");
    }

    #[tokio::test]
    async fn test_less_stdin() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("stdin content"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = Less.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "stdin content");
    }

    #[tokio::test]
    async fn test_less_multiple_files() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("file1.txt"), b"content1")
            .await
            .unwrap();
        fs.write_file(&cwd.join("file2.txt"), b"content2")
            .await
            .unwrap();

        let args = vec!["file1.txt".to_string(), "file2.txt".to_string()];
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
        };

        let result = Less.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("==> file1.txt <=="));
        assert!(result.stdout.contains("content1"));
        assert!(result.stdout.contains("==> file2.txt <=="));
        assert!(result.stdout.contains("content2"));
    }

    #[tokio::test]
    async fn test_less_nonexistent() {
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
        };

        let result = Less.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("No such file or directory"));
    }

    #[tokio::test]
    async fn test_less_directory() {
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
        };

        let result = Less.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("Is a directory"));
    }

    // ==================== file tests ====================

    #[tokio::test]
    async fn test_file_regular() {
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
        };

        let result = File.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("test.txt:"));
        assert!(result.stdout.contains("ASCII text"));
    }

    #[tokio::test]
    async fn test_file_directory() {
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
        };

        let result = File.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("directory"));
    }

    #[tokio::test]
    async fn test_file_empty() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("empty.txt"), b"").await.unwrap();

        let args = vec!["empty.txt".to_string()];
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
        };

        let result = File.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("empty"));
    }

    #[tokio::test]
    async fn test_file_json() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("data.json"), b"{\"key\": \"value\"}")
            .await
            .unwrap();

        let args = vec!["data.json".to_string()];
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
        };

        let result = File.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("JSON"));
    }

    #[tokio::test]
    async fn test_file_script() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("script.sh"), b"#!/bin/bash\necho hello")
            .await
            .unwrap();

        let args = vec!["script.sh".to_string()];
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
        };

        let result = File.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("shell script") || result.stdout.contains("Bourne"));
    }

    #[tokio::test]
    async fn test_file_nonexistent() {
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
        };

        let result = File.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0); // file command continues with other files
        assert!(result.stdout.contains("No such file or directory"));
    }

    #[tokio::test]
    async fn test_file_missing_operand() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
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
        };

        let result = File.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing file operand"));
    }

    // ==================== stat tests ====================

    #[tokio::test]
    async fn test_stat_basic() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

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
        };

        let result = Stat.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("test.txt"));
        assert!(result.stdout.contains("Size:"));
    }

    #[tokio::test]
    async fn test_stat_format_name() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("test.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["-c".to_string(), "%n".to_string(), "test.txt".to_string()];
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
        };

        let result = Stat.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "test.txt");
    }

    #[tokio::test]
    async fn test_stat_format_size() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("test.txt"), b"content")
            .await
            .unwrap();

        let args = vec!["-c".to_string(), "%s".to_string(), "test.txt".to_string()];
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
        };

        let result = Stat.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "7"); // "content" is 7 bytes
    }

    #[tokio::test]
    async fn test_stat_format_permissions() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("test.txt"), b"content")
            .await
            .unwrap();
        fs.chmod(&cwd.join("test.txt"), 0o755).await.unwrap();

        let args = vec!["-c".to_string(), "%a".to_string(), "test.txt".to_string()];
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
        };

        let result = Stat.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "755");
    }

    #[tokio::test]
    async fn test_stat_format_type() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.mkdir(&cwd.join("testdir"), false).await.unwrap();

        let args = vec!["-c".to_string(), "%F".to_string(), "testdir".to_string()];
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
        };

        let result = Stat.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("directory"));
    }

    #[tokio::test]
    async fn test_stat_nonexistent() {
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
        };

        let result = Stat.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("No such file or directory"));
    }

    #[tokio::test]
    async fn test_stat_missing_operand() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
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
        };

        let result = Stat.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing operand"));
    }

    #[tokio::test]
    async fn test_stat_missing_format_arg() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-c".to_string()];
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
        };

        let result = Stat.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing argument"));
    }

    // ==================== content type tests ====================

    #[test]
    fn test_determine_file_content_type_empty() {
        assert_eq!(determine_file_content_type(b""), "empty");
    }

    #[test]
    fn test_determine_file_content_type_json() {
        assert!(determine_file_content_type(b"{\"key\": \"value\"}").contains("JSON"));
        assert!(determine_file_content_type(b"[1, 2, 3]").contains("JSON"));
    }

    #[test]
    fn test_determine_file_content_type_text() {
        assert!(determine_file_content_type(b"Hello, world!").contains("ASCII"));
    }

    #[test]
    fn test_determine_file_content_type_gzip() {
        assert!(determine_file_content_type(b"\x1f\x8b\x08\x00").contains("gzip"));
    }

    #[test]
    fn test_determine_file_content_type_png() {
        assert!(determine_file_content_type(b"\x89PNG\r\n\x1a\n").contains("PNG"));
    }
}
