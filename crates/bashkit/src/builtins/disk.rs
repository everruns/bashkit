//! Disk usage builtins - du and df
//!
//! Decision: `du` follows GNU's option surface and output shape (operand-
//! relative paths, directories only unless `-a`, `-d`/`-s` depth limits,
//! `-c` total, GNU ceiling rounding for `-h`/`--si`/`-B`). The VFS has no
//! block allocation, so "disk usage" is the apparent size; without `-b` it
//! is reported in ceil(bytes / 1024) blocks.

use async_trait::async_trait;
use std::path::Path;

use super::arg_parser::{OptArg, gnu_getopt};
use super::{Builtin, Context};
use crate::error::Result;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;

/// The du builtin - estimate file space usage.
///
/// Usage: du [-abchkms] [-d N] [-B SIZE] [--si] [FILE...]
///
/// If no FILE is specified, shows usage for current directory.
pub struct Du;

#[derive(Clone, Copy, PartialEq)]
enum DuScale {
    Blocks(u64),
    Human(u64),
}

struct DuOptions {
    all: bool,
    total: bool,
    max_depth: Option<usize>,
    scale: DuScale,
}

fn parse_block_size(s: &str) -> Option<u64> {
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    let n: u64 = if digits == 0 {
        1
    } else {
        s[..digits].parse().ok()?
    };
    let unit = match &s[digits..] {
        "" => 1,
        u => {
            let (letter, rest) = u.split_at(1);
            let power = "KMGTPEZY".find(&letter.to_ascii_uppercase())? as u32 + 1;
            let base: u64 = match rest {
                "" | "iB" => 1024,
                "B" => 1000,
                _ => return None,
            };
            base.checked_pow(power)?
        }
    };
    n.checked_mul(unit).filter(|v| *v > 0)
}

#[allow(clippy::result_large_err)]
fn parse_du_args(args: &[String]) -> std::result::Result<(DuOptions, Vec<String>), ExecResult> {
    let (parsed, paths) = gnu_getopt(
        "du",
        args,
        "abcd:hkmsB:LPxSl0",
        &[
            ("all", OptArg::No, 'a'),
            ("apparent-size", OptArg::No, 'A'),
            ("bytes", OptArg::No, 'b'),
            ("total", OptArg::No, 'c'),
            ("max-depth", OptArg::Required, 'd'),
            ("human-readable", OptArg::No, 'h'),
            ("si", OptArg::No, 'H'),
            ("summarize", OptArg::No, 's'),
            ("block-size", OptArg::Required, 'B'),
            ("dereference", OptArg::No, 'L'),
            ("no-dereference", OptArg::No, 'P'),
            ("one-file-system", OptArg::No, 'x'),
            ("separate-dirs", OptArg::No, 'S'),
            ("count-links", OptArg::No, 'l'),
            ("null", OptArg::No, '0'),
        ],
        true,
        1,
    )?;
    let mut opts = DuOptions {
        all: false,
        total: false,
        max_depth: None,
        scale: DuScale::Blocks(1024),
    };
    let mut summarize = false;
    for o in parsed {
        match o.key {
            'a' => opts.all = true,
            'b' => opts.scale = DuScale::Blocks(1),
            'c' => opts.total = true,
            'd' => {
                let v = o.value.unwrap_or_default();
                match v.parse() {
                    Ok(n) => opts.max_depth = Some(n),
                    Err(_) => {
                        return Err(ExecResult::err(
                            format!("du: invalid maximum depth '{v}'\n"),
                            1,
                        ));
                    }
                }
            }
            'h' => opts.scale = DuScale::Human(1024),
            'H' => opts.scale = DuScale::Human(1000),
            'k' => opts.scale = DuScale::Blocks(1024),
            'm' => opts.scale = DuScale::Blocks(1024 * 1024),
            's' => summarize = true,
            'B' => {
                let v = o.value.unwrap_or_default();
                match parse_block_size(&v) {
                    Some(n) => opts.scale = DuScale::Blocks(n),
                    None => {
                        return Err(ExecResult::err(
                            format!("du: invalid -B argument '{v}'\n"),
                            1,
                        ));
                    }
                }
            }
            // Accepted for compatibility: the VFS has no devices or hard
            // links to count, and sizes are always apparent sizes.
            _ => {}
        }
    }
    if summarize {
        if opts.max_depth.is_some_and(|d| d != 0) {
            return Err(ExecResult::err(
                "du: cannot both summarize and show all entries\n".to_string(),
                1,
            ));
        }
        opts.max_depth = Some(0);
    }
    if summarize && opts.all {
        return Err(ExecResult::err(
            "du: cannot both summarize and show all entries\n".to_string(),
            1,
        ));
    }
    Ok((opts, paths))
}

/// GNU `human_readable` with ceiling rounding: one decimal below 10,
/// whole units otherwise, plain bytes below one unit.
fn human_size(n: u64, base: u64) -> String {
    const SUFFIX: &[u8] = b"KMGTPEZY";
    let base = u128::from(base);
    let n128 = u128::from(n);
    if n128 < base {
        return n.to_string();
    }
    let mut e = 0usize;
    let mut div = 1u128;
    while e < SUFFIX.len() && n128 >= div * base {
        div *= base;
        e += 1;
    }
    let suffix = |e: usize| {
        let c = SUFFIX[e - 1] as char;
        if base == 1000 && c == 'K' { 'k' } else { c }
    };
    let tenths = (n128 * 10).div_ceil(div);
    if tenths < 100 {
        return format!("{}.{}{}", tenths / 10, tenths % 10, suffix(e));
    }
    let whole = n128.div_ceil(div);
    if whole >= base && e < SUFFIX.len() {
        return format!("1.0{}", suffix(e + 1));
    }
    format!("{whole}{}", suffix(e))
}

fn format_size(bytes: u64, scale: DuScale) -> String {
    match scale {
        DuScale::Blocks(1) => bytes.to_string(),
        DuScale::Blocks(n) => bytes.div_ceil(n).to_string(),
        DuScale::Human(base) => human_size(bytes, base),
    }
}

#[async_trait]
impl Builtin for Du {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: du [OPTION]... [FILE]...\n\
             Estimate file space usage.\n\n\
             \x20 -a, --all\twrite counts for all files, not just directories\n\
             \x20 -b, --bytes\tprint sizes in bytes\n\
             \x20 -B, --block-size=SIZE\tscale sizes by SIZE\n\
             \x20 -c, --total\tproduce a grand total\n\
             \x20 -d, --max-depth=N\tprint totals only N levels deep\n\
             \x20 -h, --human-readable\tprint sizes in human readable format\n\
             \x20     --si\tlike -h, but use powers of 1000\n\
             \x20 -k\tlike --block-size=1K\n\
             \x20 -m\tlike --block-size=1M\n\
             \x20 -s, --summarize\tdisplay only a total for each argument\n\
             \x20 --help\tdisplay this help and exit\n\
             \x20 --version\toutput version information and exit\n",
            Some("du (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let (opts, mut paths) = match parse_du_args(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        if paths.is_empty() {
            paths.push(".".to_string());
        }

        let mut output = String::new();
        let mut stderr = String::new();
        let mut grand_total = 0u64;
        for operand in &paths {
            let path = if operand.starts_with('/') {
                std::path::PathBuf::from(operand)
            } else {
                vfs_join(ctx.cwd, operand)
            };
            if ctx.fs.stat(&path).await.is_err() {
                stderr.push_str(&format!(
                    "du: cannot access '{operand}': No such file or directory\n"
                ));
                continue;
            }
            grand_total += walk(&ctx, &path, operand, 0, &opts, &mut output).await?;
        }
        if opts.total {
            output.push_str(&format!("{}\ttotal\n", format_size(grand_total, opts.scale)));
        }

        Ok(ExecResult {
            stdout: output.into(),
            exit_code: i32::from(!stderr.is_empty()),
            stderr: stderr.into(),
            ..Default::default()
        })
    }
}

/// Sum `path` (displayed as `display`), printing entries GNU-style.
fn walk<'a>(
    ctx: &'a Context<'_>,
    path: &'a Path,
    display: &'a str,
    depth: usize,
    opts: &'a DuOptions,
    output: &'a mut String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + 'a>> {
    Box::pin(async move {
        ctx.consume_budget_work(1)?;
        // Operands follow symlinks (GNU stats command-line args); nested
        // entries count the link itself.
        let metadata = if depth == 0 {
            ctx.fs.stat(path).await?
        } else {
            ctx.fs.lstat(path).await?
        };
        let shown = opts.max_depth.is_none_or(|d| depth <= d);

        if !metadata.file_type.is_dir() {
            if shown && (opts.all || depth == 0) {
                output.push_str(&format!(
                    "{}\t{display}\n",
                    format_size(metadata.size, opts.scale)
                ));
            }
            return Ok(metadata.size);
        }

        let mut total = 0u64;
        let entries = ctx.fs.read_dir(path).await?;
        ctx.consume_budget_work(u64::try_from(entries.len()).unwrap_or(u64::MAX))?;
        for entry in entries {
            let child_path = vfs_join(path, &entry.name);
            let child_display = if display.ends_with('/') {
                format!("{display}{}", entry.name)
            } else {
                format!("{display}/{}", entry.name)
            };
            total += walk(ctx, &child_path, &child_display, depth + 1, opts, output).await?;
        }
        if shown {
            output.push_str(&format!("{}\t{display}\n", format_size(total, opts.scale)));
        }
        Ok(total)
    })
}

/// The df builtin - report file system disk space usage.
///
/// Usage: df [-h]
///
/// Options:
///   -h   Print sizes in human readable format
///
/// Shows total, used, and available space for the virtual filesystem.
pub struct Df;

#[async_trait]
impl Builtin for Df {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: df [-h]\n\
             Report file system disk space usage.\n\n\
             \x20 -h\tprint sizes in human readable format\n\
             \x20 --help\tdisplay this help and exit\n\
             \x20 --version\toutput version information and exit\n",
            Some("df (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let mut human_readable = false;

        for arg in ctx.args {
            if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        'h' => human_readable = true,
                        _ => {
                            return Ok(ExecResult::err(
                                format!("df: invalid option -- '{}'\n", c),
                                1,
                            ));
                        }
                    }
                }
            }
        }

        let usage = ctx.fs.usage();
        let limits = ctx.fs.limits();

        let total = limits.max_total_bytes;
        let used = usage.total_bytes;
        let available = total.saturating_sub(used);
        let use_percent = if total > 0 {
            ((used as f64 / total as f64) * 100.0) as u64
        } else {
            0
        };

        let mut output = String::new();

        // Header
        if human_readable {
            output.push_str("Filesystem      Size  Used Avail Use% Mounted on\n");
        } else {
            output.push_str("Filesystem     1K-blocks      Used Available Use% Mounted on\n");
        }

        // Data row
        let (total_str, used_str, avail_str) = if human_readable {
            (
                format_size(total, DuScale::Human(1024)),
                format_size(used, DuScale::Human(1024)),
                format_size(available, DuScale::Human(1024)),
            )
        } else {
            (
                format!("{}", total / 1024),
                format!("{}", used / 1024),
                format!("{}", available / 1024),
            )
        };

        if human_readable {
            output.push_str(&format!(
                "{:<15} {:>5} {:>5} {:>5} {:>3}% {}\n",
                "bashkit-vfs", total_str, used_str, avail_str, use_percent, "/"
            ));
        } else {
            output.push_str(&format!(
                "{:<14} {:>10} {:>9} {:>9} {:>3}% {}\n",
                "bashkit-vfs", total_str, used_str, avail_str, use_percent, "/"
            ));
        }

        // Additional info about limits
        output.push_str(&format!(
            "# Files: {}/{}, Dirs: {}\n",
            usage.file_count, limits.max_file_count, usage.dir_count
        ));

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, FsLimits, InMemoryFs};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn create_test_ctx() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();

        fs.mkdir(&cwd, true).await.unwrap();

        (fs, cwd, variables)
    }

    // ==================== du tests ====================

    // Issue #2425: `du` prints the paths it builds, so every separator in
    // them must be `/` even when the host separator is `\`.
    #[tokio::test]
    async fn windows_containment_du_prints_slash_separated_paths() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.mkdir(&cwd.join("proj/src"), true).await.unwrap();
        fs.write_file(&cwd.join("proj/src/main.rs"), b"fn main() {}")
            .await
            .unwrap();

        let args = vec!["proj".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Du.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("\tproj/src\n"),
            "recursive descent path is not slash-separated:\n{}",
            result.stdout
        );
        assert!(
            !result.stdout.contains('\\'),
            "output leaked a host separator:\n{}",
            result.stdout
        );
    }

    #[tokio::test]
    async fn test_du_file() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create test file with known size
        fs.write_file(&cwd.join("test.txt"), b"hello world")
            .await
            .unwrap();

        let args = vec!["test.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Du.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_du_summary() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.mkdir(&cwd.join("subdir"), false).await.unwrap();
        fs.write_file(&cwd.join("subdir/file1.txt"), b"content1")
            .await
            .unwrap();
        fs.write_file(&cwd.join("subdir/file2.txt"), b"content2")
            .await
            .unwrap();

        let args = vec!["-s".to_string(), "subdir".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Du.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        // Should only have one line for summary
        let lines: Vec<_> = result.stdout.lines().collect();
        assert_eq!(lines.len(), 1);
    }

    #[tokio::test]
    async fn test_du_human_readable() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create a larger file
        let large_content = vec![b'x'; 2048];
        fs.write_file(&cwd.join("large.txt"), &large_content)
            .await
            .unwrap();

        let args = vec!["-h".to_string(), "large.txt".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Du.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("K"));
    }

    #[tokio::test]
    async fn test_du_nonexistent() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["nonexistent".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Du.execute(ctx).await.unwrap();
        assert_ne!(result.exit_code, 0);
    }

    async fn run_du(args: &[&str], files: &[(&str, usize)]) -> ExecResult {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();
        fs.mkdir(&cwd.join("d/sub"), true).await.unwrap();
        for (name, size) in files {
            fs.write_file(&cwd.join(name), &vec![b'x'; *size])
                .await
                .unwrap();
        }
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);
        Du.execute(ctx).await.unwrap()
    }

    #[test]
    fn test_du_human_size_gnu_rounding() {
        let cases = [
            (1, "1"),
            (1023, "1023"),
            (1024, "1.0K"),
            (1025, "1.1K"),
            (10239, "10K"),
            (10241, "11K"),
            (1047553, "1.0M"),
            (1048577, "1.1M"),
            (5000000, "4.8M"),
        ];
        for (n, want) in cases {
            assert_eq!(human_size(n, 1024), want, "n={n}");
        }
        assert_eq!(human_size(1500, 1000), "1.5k");
    }

    #[tokio::test]
    async fn test_du_operand_paths_and_all() {
        let files = [("d/a", 10), ("d/sub/b", 2000)];
        let result = run_du(&["-b", "d"], &files).await;
        assert_eq!(result.stdout, "2000\td/sub\n2010\td\n");
        let result = run_du(&["-ab", "d"], &files).await;
        // Children come in read_dir order; the directory follows its children.
        let mut lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines.pop(), Some("2010\td"));
        lines.sort_unstable();
        assert_eq!(lines, ["10\td/a", "2000\td/sub", "2000\td/sub/b"]);
    }

    #[tokio::test]
    async fn test_du_depth_total_and_missing() {
        let files = [("d/a", 10), ("d/sub/b", 2000)];
        let result = run_du(&["-d0", "-c", "d/sub", "d/a"], &files).await;
        assert_eq!(result.stdout, "2\td/sub\n1\td/a\n2\ttotal\n");
        let result = run_du(&["-s", "d", "nope"], &files).await;
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "2\td\n");
        assert!(result.stderr.contains("cannot access 'nope'"));
    }

    // ==================== df tests ====================

    #[tokio::test]
    async fn test_df_basic() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Df.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("bashkit-vfs"));
        assert!(result.stdout.contains("Filesystem"));
    }

    #[tokio::test]
    async fn test_df_human_readable() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-h".to_string()];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Df.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        // Human readable should have M or G for 100MB limit
        assert!(result.stdout.contains("M") || result.stdout.contains("G"));
    }

    #[tokio::test]
    async fn test_df_shows_usage() {
        let limits = FsLimits::new().max_total_bytes(1_000_000); // 1MB
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut cwd = PathBuf::from("/tmp");
        let mut variables = HashMap::new();
        let env = HashMap::new();

        // Write some data
        fs.write_file(&cwd.join("data.txt"), &vec![b'x'; 100_000])
            .await
            .unwrap();

        let args: Vec<String> = vec![];
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs.clone(), None);

        let result = Df.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        // Should show some usage percentage
        assert!(result.stdout.contains("%"));
    }

    // ==================== format_size tests ====================

    const H: DuScale = DuScale::Human(1024);
    const K: DuScale = DuScale::Blocks(1024);

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(500, H), "500");
        assert_eq!(format_size(0, H), "0");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(1024, H), "1.0K");
        assert_eq!(format_size(2048, H), "2.0K");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(1024 * 1024, H), "1.0M");
        assert_eq!(format_size(5 * 1024 * 1024, H), "5.0M");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(1024 * 1024 * 1024, H), "1.0G");
    }

    #[test]
    fn test_format_size_blocks() {
        // Non-human-readable returns 1K blocks
        assert_eq!(format_size(512, K), "1");
        assert_eq!(format_size(1024, K), "1");
        assert_eq!(format_size(2048, K), "2");
    }
}
