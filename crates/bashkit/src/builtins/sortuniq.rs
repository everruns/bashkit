//! Sort and uniq builtins - sort lines and filter duplicates

// Uses unwrap() after is_empty() check (e.g., files.first() in else branch)
#![allow(clippy::unwrap_used)]

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The sort builtin - sort lines of text.
///
/// Usage: sort [-cfhnMruVs] [-t DELIM] [-k KEYDEF] [-o FILE] [FILE...]
///
/// Options:
///   -f   Fold lower case to upper case characters (case insensitive)
///   -n   Compare according to string numerical value
///   -r   Reverse the result of comparisons
///   -u   Output only unique lines (like sort | uniq)
///   -V   Natural sort of version numbers
///   -t   Field delimiter character
///   -k   Sort key definition (e.g., -k2 or -k2,2)
///   -s   Stable sort (preserve input order for equal keys)
///   -c   Check if input is sorted; exit 1 if not
///   -h   Human numeric sort (1K, 2M, 3G)
///   -M   Month sort (JAN < FEB < ... < DEC)
///   -o   Write output to FILE
pub struct Sort;

/// Extract the sort key from a line based on field delimiter and key spec
fn extract_key(line: &str, delimiter: Option<char>, key_field: usize) -> String {
    if let Some(delim) = delimiter {
        line.split(delim)
            .nth(key_field.saturating_sub(1))
            .unwrap_or("")
            .to_string()
    } else {
        // Default: whitespace-separated fields
        line.split_whitespace()
            .nth(key_field.saturating_sub(1))
            .unwrap_or("")
            .to_string()
    }
}

/// Parse human-numeric value (e.g., "10K" → 10_000, "5M" → 5_000_000)
fn parse_human_numeric(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return 0.0;
    }
    let last = s.as_bytes().last().copied().unwrap_or(b'0');
    let multiplier = match last {
        b'K' | b'k' => 1_000.0,
        b'M' | b'm' => 1_000_000.0,
        b'G' | b'g' => 1_000_000_000.0,
        b'T' | b't' => 1_000_000_000_000.0,
        _ => return s.parse::<f64>().unwrap_or(0.0),
    };
    let num_part = &s[..s.len() - 1];
    num_part.parse::<f64>().unwrap_or(0.0) * multiplier
}

/// Parse month abbreviation to ordinal (1-12, 0 for unknown)
fn month_ordinal(s: &str) -> u32 {
    match s.trim().to_uppercase().as_str() {
        "JAN" => 1,
        "FEB" => 2,
        "MAR" => 3,
        "APR" => 4,
        "MAY" => 5,
        "JUN" => 6,
        "JUL" => 7,
        "AUG" => 8,
        "SEP" => 9,
        "OCT" => 10,
        "NOV" => 11,
        "DEC" => 12,
        _ => 0,
    }
}

#[async_trait]
impl Builtin for Sort {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut reverse = false;
        let mut numeric = false;
        let mut unique = false;
        let mut fold_case = false;
        let mut stable = false;
        let mut check_sorted = false;
        let mut human_numeric = false;
        let mut month_sort = false;
        let mut merge = false;
        let mut delimiter: Option<char> = None;
        let mut key_field: Option<usize> = None;
        let mut output_file: Option<String> = None;
        let mut zero_terminated = false;
        let mut files = Vec::new();

        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            if arg == "-t" {
                i += 1;
                if i < ctx.args.len() {
                    delimiter = ctx.args[i].chars().next();
                }
            } else if let Some(d) = arg.strip_prefix("-t") {
                delimiter = d.chars().next();
            } else if arg == "-k" {
                i += 1;
                if i < ctx.args.len() {
                    // Parse key: "2" or "2,2" or "2n"
                    let key_spec = &ctx.args[i];
                    let field_str: String = key_spec
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    key_field = field_str.parse().ok();
                    // Check for type suffix in key spec
                    if key_spec.contains('n') {
                        numeric = true;
                    }
                    if key_spec.contains('r') {
                        reverse = true;
                    }
                }
            } else if let Some(k) = arg.strip_prefix("-k") {
                let field_str: String = k.chars().take_while(|c| c.is_ascii_digit()).collect();
                key_field = field_str.parse().ok();
                if k.contains('n') {
                    numeric = true;
                }
            } else if arg == "-o" {
                i += 1;
                if i < ctx.args.len() {
                    output_file = Some(ctx.args[i].clone());
                }
            } else if arg.starts_with('-') && !arg.starts_with("--") {
                for c in arg.chars().skip(1) {
                    match c {
                        'r' => reverse = true,
                        'n' => numeric = true,
                        'u' => unique = true,
                        'f' => fold_case = true,
                        's' => stable = true,
                        'c' | 'C' => check_sorted = true,
                        'h' => human_numeric = true,
                        'M' => month_sort = true,
                        'm' => merge = true,
                        'z' => zero_terminated = true,
                        _ => {}
                    }
                }
            } else {
                files.push(arg.clone());
            }
            i += 1;
        }

        // Collect all input
        let mut all_lines = Vec::new();

        let line_sep = if zero_terminated { '\0' } else { '\n' };

        if files.is_empty() {
            if let Some(stdin) = ctx.stdin {
                for line in stdin.split(line_sep) {
                    if !line.is_empty() {
                        all_lines.push(line.to_string());
                    }
                }
            }
        } else {
            for file in &files {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    ctx.cwd.join(file)
                };

                match ctx.fs.read_file(&path).await {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content);
                        for line in text.split(line_sep) {
                            if !line.is_empty() {
                                all_lines.push(line.to_string());
                            }
                        }
                    }
                    Err(e) => {
                        return Ok(ExecResult::err(format!("sort: {}: {}\n", file, e), 1));
                    }
                }
            }
        }

        // Merge mode: k-way merge of pre-sorted inputs
        if merge && !files.is_empty() {
            let mut streams: Vec<Vec<String>> = Vec::new();
            for file in &files {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    ctx.cwd.join(file)
                };
                match ctx.fs.read_file(&path).await {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content);
                        let lines: Vec<String> = text
                            .split(line_sep)
                            .filter(|l| !l.is_empty())
                            .map(|l| l.to_string())
                            .collect();
                        streams.push(lines);
                    }
                    Err(e) => {
                        return Ok(ExecResult::err(format!("sort: {}: {}\n", file, e), 1));
                    }
                }
            }
            // k-way merge using indices
            let mut indices: Vec<usize> = vec![0; streams.len()];
            let mut merged = Vec::new();
            loop {
                let mut best: Option<(usize, &str)> = None;
                for (i, stream) in streams.iter().enumerate() {
                    if indices[i] < stream.len() {
                        let line = &stream[indices[i]];
                        if let Some((_, best_line)) = best {
                            if line.as_str() < best_line {
                                best = Some((i, line));
                            }
                        } else {
                            best = Some((i, line));
                        }
                    }
                }
                if let Some((i, line)) = best {
                    merged.push(line.to_string());
                    indices[i] += 1;
                } else {
                    break;
                }
            }
            let sep = if zero_terminated { "\0" } else { "\n" };
            let mut output = merged.join(sep);
            if !output.is_empty() {
                output.push_str(sep);
            }
            return Ok(ExecResult::ok(output));
        }

        // Check sorted mode
        if check_sorted {
            for i in 1..all_lines.len() {
                let cmp = if numeric {
                    let a: f64 = all_lines[i - 1].trim().parse().unwrap_or(0.0);
                    let b: f64 = all_lines[i].trim().parse().unwrap_or(0.0);
                    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
                } else {
                    all_lines[i - 1].cmp(&all_lines[i])
                };
                let out_of_order = if reverse {
                    cmp == std::cmp::Ordering::Less
                } else {
                    cmp == std::cmp::Ordering::Greater
                };
                if out_of_order {
                    return Ok(ExecResult::err(
                        format!("sort: -:{}:disorder: {}\n", i + 1, all_lines[i]),
                        1,
                    ));
                }
            }
            return Ok(ExecResult::ok(String::new()));
        }

        // Get the key extractor
        let get_key = |line: &str| -> String {
            if let Some(kf) = key_field {
                extract_key(line, delimiter, kf)
            } else {
                line.to_string()
            }
        };

        // Sort the lines
        let sort_fn = |a: &String, b: &String| -> std::cmp::Ordering {
            let ka = get_key(a);
            let kb = get_key(b);
            if human_numeric {
                let na = parse_human_numeric(&ka);
                let nb = parse_human_numeric(&kb);
                na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal)
            } else if month_sort {
                let ma = month_ordinal(&ka);
                let mb = month_ordinal(&kb);
                ma.cmp(&mb)
            } else if numeric {
                let na: f64 = ka
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                let nb: f64 = kb
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);
                na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal)
            } else if fold_case {
                ka.to_lowercase().cmp(&kb.to_lowercase())
            } else {
                ka.cmp(&kb)
            }
        };

        if stable {
            all_lines.sort_by(sort_fn);
        } else {
            all_lines.sort_unstable_by(sort_fn);
        }

        if reverse {
            all_lines.reverse();
        }

        if unique {
            all_lines.dedup();
        }

        let sep = if zero_terminated { "\0" } else { "\n" };
        let mut output = all_lines.join(sep);
        if !output.is_empty() {
            output.push_str(sep);
        }

        // Write to output file if -o specified
        if let Some(ref outfile) = output_file {
            let path = if outfile.starts_with('/') {
                std::path::PathBuf::from(outfile)
            } else {
                ctx.cwd.join(outfile)
            };
            if let Err(e) = ctx.fs.write_file(&path, output.as_bytes()).await {
                return Ok(ExecResult::err(format!("sort: {}: {}\n", outfile, e), 1));
            }
            return Ok(ExecResult::ok(String::new()));
        }

        Ok(ExecResult::ok(output))
    }
}

/// The uniq builtin - report or omit repeated lines.
///
/// Usage: uniq [-cdiu] [-f N] [INPUT [OUTPUT]]
///
/// Options:
///   -c   Prefix lines by the number of occurrences
///   -d   Only print duplicate lines
///   -u   Only print unique lines
///   -i   Case insensitive comparison
///   -f N Skip N fields before comparing
pub struct Uniq;

/// Get the comparison key for a line, skipping fields and optionally case-folding
fn uniq_key(line: &str, skip_fields: usize, case_insensitive: bool) -> String {
    let key = if skip_fields > 0 {
        line.split_whitespace()
            .skip(skip_fields)
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        line.to_string()
    };
    if case_insensitive {
        key.to_lowercase()
    } else {
        key
    }
}

#[async_trait]
impl Builtin for Uniq {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut count = false;
        let mut only_duplicates = false;
        let mut only_unique = false;
        let mut case_insensitive = false;
        let mut skip_fields: usize = 0;
        let mut files = Vec::new();

        let mut idx = 0;
        while idx < ctx.args.len() {
            let arg = &ctx.args[idx];
            if arg == "-f" {
                idx += 1;
                if idx < ctx.args.len() {
                    skip_fields = ctx.args[idx].parse().unwrap_or(0);
                }
            } else if let Some(n) = arg
                .strip_prefix("-f")
                .filter(|s| s.chars().all(|c| c.is_ascii_digit()))
            {
                skip_fields = n.parse().unwrap_or(0);
            } else if arg.starts_with('-') && !arg.starts_with("--") {
                for c in arg.chars().skip(1) {
                    match c {
                        'c' => count = true,
                        'd' => only_duplicates = true,
                        'u' => only_unique = true,
                        'i' => case_insensitive = true,
                        _ => {}
                    }
                }
            } else {
                files.push(arg.clone());
            }
            idx += 1;
        }

        // Get input lines
        let lines: Vec<String> = if files.is_empty() {
            ctx.stdin
                .map(|s| s.lines().map(|l| l.to_string()).collect())
                .unwrap_or_default()
        } else {
            let file = &files[0];
            let path = if file.starts_with('/') {
                std::path::PathBuf::from(file)
            } else {
                ctx.cwd.join(file)
            };

            match ctx.fs.read_file(&path).await {
                Ok(content) => {
                    let text = String::from_utf8_lossy(&content);
                    text.lines().map(|l| l.to_string()).collect()
                }
                Err(e) => {
                    return Ok(ExecResult::err(format!("uniq: {}: {}\n", file, e), 1));
                }
            }
        };

        let mut result = Vec::new();
        let mut prev_line: Option<String> = None;
        let mut prev_key: Option<String> = None;
        let mut current_count = 0usize;

        for line in lines {
            let key = uniq_key(&line, skip_fields, case_insensitive);
            if let Some(ref pk) = prev_key {
                if *pk == key {
                    current_count += 1;
                    continue;
                } else {
                    let should_output = if only_duplicates {
                        current_count > 1
                    } else if only_unique {
                        current_count == 1
                    } else {
                        true
                    };

                    if should_output {
                        if count {
                            result.push(format!(
                                "{:>7} {}",
                                current_count,
                                prev_line.as_deref().unwrap_or("")
                            ));
                        } else {
                            result.push(prev_line.clone().unwrap_or_default());
                        }
                    }
                }
            }
            prev_line = Some(line);
            prev_key = Some(key);
            current_count = 1;
        }

        // Last line
        if let Some(prev) = prev_line {
            let should_output = if only_duplicates {
                current_count > 1
            } else if only_unique {
                current_count == 1
            } else {
                true
            };

            if should_output {
                if count {
                    result.push(format!("{:>7} {}", current_count, prev));
                } else {
                    result.push(prev);
                }
            }
        }

        let mut output = result.join("\n");
        if !output.is_empty() {
            output.push('\n');
        }

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_sort(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        Sort.execute(ctx).await.unwrap()
    }

    async fn run_uniq(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        Uniq.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_sort_basic() {
        let result = run_sort(&[], Some("banana\napple\ncherry\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "apple\nbanana\ncherry\n");
    }

    #[tokio::test]
    async fn test_sort_reverse() {
        let result = run_sort(&["-r"], Some("apple\nbanana\ncherry\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "cherry\nbanana\napple\n");
    }

    #[tokio::test]
    async fn test_sort_numeric() {
        let result = run_sort(&["-n"], Some("10\n2\n1\n20\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\n2\n10\n20\n");
    }

    #[tokio::test]
    async fn test_sort_unique() {
        let result = run_sort(&["-u"], Some("apple\nbanana\napple\ncherry\nbanana\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "apple\nbanana\ncherry\n");
    }

    #[tokio::test]
    async fn test_sort_fold_case() {
        let result = run_sort(&["-f"], Some("Banana\napple\nCherry\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "apple\nBanana\nCherry\n");
    }

    #[tokio::test]
    async fn test_uniq_basic() {
        let result = run_uniq(&[], Some("a\na\nb\nb\nb\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\nc\n");
    }

    #[tokio::test]
    async fn test_uniq_count() {
        let result = run_uniq(&["-c"], Some("a\na\nb\nc\nc\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("2 a"));
        assert!(result.stdout.contains("1 b"));
        assert!(result.stdout.contains("3 c"));
    }

    #[tokio::test]
    async fn test_uniq_duplicates_only() {
        let result = run_uniq(&["-d"], Some("a\na\nb\nc\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("a"));
        assert!(result.stdout.contains("c"));
        assert!(!result.stdout.contains("b\n"));
    }

    #[tokio::test]
    async fn test_uniq_unique_only() {
        let result = run_uniq(&["-u"], Some("a\na\nb\nc\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("b"));
        assert!(!result.stdout.contains("a\n"));
        assert!(!result.stdout.contains("c\n"));
    }

    #[tokio::test]
    async fn test_sort_key_field() {
        let result = run_sort(&["-k2n"], Some("Bob 25\nAlice 30\nDavid 20\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "David 20\nBob 25\nAlice 30\n");
    }

    #[tokio::test]
    async fn test_sort_delimiter_key() {
        let result = run_sort(&["-t:", "-k2n"], Some("b:2\na:1\nc:3\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a:1\nb:2\nc:3\n");
    }

    #[tokio::test]
    async fn test_sort_check_sorted() {
        let result = run_sort(&["-c"], Some("a\nb\nc\n")).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_sort_check_unsorted() {
        let result = run_sort(&["-c"], Some("b\na\nc\n")).await;
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_sort_human_numeric() {
        let result = run_sort(&["-h"], Some("10K\n1K\n100M\n1G\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1K\n10K\n100M\n1G\n");
    }

    #[tokio::test]
    async fn test_sort_month() {
        let result = run_sort(&["-M"], Some("Mar\nJan\nFeb\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Jan\nFeb\nMar\n");
    }

    #[tokio::test]
    async fn test_uniq_case_insensitive() {
        let result = run_uniq(&["-i"], Some("a\nA\nb\nB\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\n");
    }

    #[tokio::test]
    async fn test_uniq_skip_fields() {
        let result = run_uniq(&["-f1"], Some("x a\ny a\nx b\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "x a\nx b\n");
    }

    #[tokio::test]
    async fn test_extract_key() {
        assert_eq!(extract_key("a:b:c", Some(':'), 2), "b");
        assert_eq!(extract_key("hello world", None, 1), "hello");
        assert_eq!(extract_key("hello world", None, 2), "world");
        assert_eq!(extract_key("x", None, 5), "");
    }

    #[tokio::test]
    async fn test_parse_human_numeric() {
        assert_eq!(parse_human_numeric("1K"), 1000.0);
        assert_eq!(parse_human_numeric("5M"), 5_000_000.0);
        assert_eq!(parse_human_numeric("2G"), 2_000_000_000.0);
        assert_eq!(parse_human_numeric("42"), 42.0);
        assert_eq!(parse_human_numeric(""), 0.0);
    }

    #[tokio::test]
    async fn test_month_ordinal() {
        assert_eq!(month_ordinal("JAN"), 1);
        assert_eq!(month_ordinal("feb"), 2);
        assert_eq!(month_ordinal("Dec"), 12);
        assert_eq!(month_ordinal("xyz"), 0);
    }
}
