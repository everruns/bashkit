//! join builtin command - join lines of two sorted files on a common field
//!
//! Decisions: GNU semantics. Without `-t`, fields are separated by runs of
//! blanks (leading blanks ignored) and output uses one space; with `-t C`
//! every `C` separates and output uses `C`. Equal keys pair as a cartesian
//! product. `-e` fills missing fields only where `-o` names them, as in GNU.

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The join builtin command.
///
/// Usage: join [OPTION]... FILE1 FILE2
///
/// Join lines of two sorted files on a common field (default: first field).
pub struct Join;

/// One `-o` item: the join field, or field `M` (1-based) of file `N`.
#[derive(Clone, Copy)]
enum OutField {
    Key,
    Field(usize, usize),
}

struct JoinOptions {
    field1: usize,                 // 1-based field number for file1
    field2: usize,                 // 1-based field number for file2
    separator: Option<char>,       // None: blank runs
    unpaired: [bool; 2],           // -a / -v: print unpairable lines of file 1/2
    only_unpaired: bool,           // -v: suppress joined lines
    empty: Option<String>,         // -e: replacement for missing fields
    format: Option<Vec<OutField>>, // -o
    auto_format: bool,             // -o auto
    ignore_case: bool,             // -i
    header: bool,                  // --header
}

const USAGE: &str = "Usage: join [OPTION]... FILE1 FILE2\nJoin lines of two sorted files on a common field.\n\n  -a FILENUM\talso print unpairable lines from file FILENUM\n  -e EMPTY\treplace missing input fields with EMPTY\n  -i, --ignore-case\tignore differences in case when comparing fields\n  -j FIELD\tequivalent to '-1 FIELD -2 FIELD'\n  -o FORMAT\tobey FORMAT while constructing output line\n  -t CHAR\tuse CHAR as input and output field separator\n  -v FILENUM\tlike -a FILENUM, but suppress joined output lines\n  -1 FIELD\tjoin on this FIELD of file 1\n  -2 FIELD\tjoin on this FIELD of file 2\n  --header\ttreat the first line in each file as field headers\n  --help\t\tdisplay this help and exit\n  --version\toutput version information and exit\n";

fn parse_format(spec: &str) -> std::result::Result<Vec<OutField>, String> {
    spec.split([',', ' ', '\t'])
        .filter(|s| !s.is_empty())
        .map(|item| {
            if item == "0" {
                return Ok(OutField::Key);
            }
            let bad = || format!("join: invalid field specifier: '{item}'\n");
            let (file, field) = item.split_once('.').ok_or_else(bad)?;
            let file: usize = file.parse().map_err(|_| bad())?;
            let field: usize = field.parse().map_err(|_| bad())?;
            if !(1..=2).contains(&file) || field == 0 {
                return Err(bad());
            }
            Ok(OutField::Field(file, field))
        })
        .collect()
}

fn parse_file_number(value: &str) -> std::result::Result<usize, String> {
    match value {
        "1" => Ok(1),
        "2" => Ok(2),
        _ => Err(format!("join: invalid file number: '{value}'\n")),
    }
}

#[async_trait]
impl Builtin for Join {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(ctx.args, USAGE, Some("join (bashkit) 0.1")) {
            return Ok(r);
        }
        let mut opts = JoinOptions {
            field1: 1,
            field2: 1,
            separator: None,
            unpaired: [false; 2],
            only_unpaired: false,
            empty: None,
            format: None,
            auto_format: false,
            ignore_case: false,
            header: false,
        };

        let mut files: Vec<&str> = Vec::new();
        let mut p = super::arg_parser::ArgParser::new(ctx.args);
        macro_rules! try_opt {
            ($e:expr) => {
                match $e {
                    Ok(v) => v,
                    Err(message) => return Ok(ExecResult::err(message, 1)),
                }
            };
        }

        while !p.is_done() {
            if let Some(val) = p.flag_value_opt("-1") {
                opts.field1 = try_opt!(parse_field_number("-1", val));
            } else if let Some(val) = p.flag_value_opt("-2") {
                opts.field2 = try_opt!(parse_field_number("-2", val));
            } else if let Some(val) = p.flag_value_opt("-j") {
                let f = try_opt!(parse_field_number("-j", val));
                opts.field1 = f;
                opts.field2 = f;
            } else if let Some(val) = p.flag_value_opt("-t") {
                opts.separator = Some(val.chars().next().unwrap_or('\n'));
            } else if let Some(val) = p.flag_value_opt("-a") {
                let n = try_opt!(parse_file_number(val));
                opts.unpaired[n - 1] = true;
            } else if let Some(val) = p.flag_value_opt("-v") {
                let n = try_opt!(parse_file_number(val));
                opts.unpaired[n - 1] = true;
                opts.only_unpaired = true;
            } else if let Some(val) = p.flag_value_opt("-e") {
                opts.empty = Some(val.to_string());
            } else if let Some(val) = p.flag_value_opt("-o") {
                if val == "auto" {
                    opts.auto_format = true;
                } else {
                    let mut items = try_opt!(parse_format(val));
                    opts.format.get_or_insert_with(Vec::new).append(&mut items);
                }
            } else if matches!(p.current(), Some("-i" | "--ignore-case")) {
                opts.ignore_case = true;
                p.advance();
            } else if p.current() == Some("--header") {
                opts.header = true;
                p.advance();
            } else if matches!(p.current(), Some("--nocheck-order" | "--check-order")) {
                p.advance();
            } else if p.is_flag() && p.current() != Some("--") {
                // Reject unknown options instead of treating them as files.
                return Ok(super::invalid_option(
                    "join",
                    p.current().unwrap_or_default(),
                    1,
                ));
            } else if let Some(arg) = p.positional() {
                files.push(arg);
            }
        }

        if files.len() < 2 {
            return Ok(ExecResult::err("join: missing operand\n".to_string(), 1));
        }

        let content1 = read_input(
            ctx.fs.as_ref(),
            ctx.cwd,
            files[0],
            ctx.stdin.map(|stdin| &**stdin),
        )
        .await?;
        let content2 = read_input(ctx.fs.as_ref(), ctx.cwd, files[1], None).await?;

        Ok(ExecResult::ok(join_text(&opts, &content1, &content2)))
    }
}

fn split_fields(line: &str, sep: Option<char>) -> Vec<&str> {
    match sep {
        Some(c) => line.split(c).collect(),
        None => line.split([' ', '\t']).filter(|f| !f.is_empty()).collect(),
    }
}

struct Line<'a> {
    fields: Vec<&'a str>,
    key: String,
}

fn parse_lines<'a>(text: &'a str, field: usize, opts: &JoinOptions) -> Vec<Line<'a>> {
    text.lines()
        .map(|l| {
            let fields = split_fields(l, opts.separator);
            let key = fields.get(field - 1).copied().unwrap_or("");
            let key = if opts.ignore_case {
                key.to_lowercase()
            } else {
                key.to_string()
            };
            Line { fields, key }
        })
        .collect()
}

fn join_text(opts: &JoinOptions, text1: &str, text2: &str) -> String {
    let mut lines1 = parse_lines(text1, opts.field1, opts);
    let mut lines2 = parse_lines(text2, opts.field2, opts);
    let out_sep = opts.separator.unwrap_or(' ');
    let fields = [opts.field1, opts.field2];
    // `-o auto`: the join field, then every other field of each file's first line.
    let format = opts.format.clone().or_else(|| {
        opts.auto_format.then(|| {
            let mut f = vec![OutField::Key];
            for (n, first) in [lines1.first(), lines2.first()].into_iter().enumerate() {
                let count = first.map_or(0, |l| l.fields.len());
                f.extend(
                    (1..=count)
                        .filter(|&m| m != fields[n])
                        .map(|m| OutField::Field(n + 1, m)),
                );
            }
            f
        })
    });

    let mut out = String::new();
    let emit = |out: &mut String, l1: Option<&Line>, l2: Option<&Line>| {
        let key = l1.or(l2).map_or("", |l| {
            let f = if l1.is_some() { fields[0] } else { fields[1] };
            l.fields.get(f - 1).copied().unwrap_or("")
        });
        let mut parts: Vec<&str> = Vec::new();
        let empty = opts.empty.as_deref().unwrap_or("");
        match &format {
            Some(format) => {
                for item in format {
                    parts.push(match *item {
                        OutField::Key => key,
                        OutField::Field(n, m) => {
                            let side = if n == 1 { l1 } else { l2 };
                            side.and_then(|l| l.fields.get(m - 1).copied())
                                .unwrap_or(empty)
                        }
                    });
                }
            }
            None => {
                parts.push(key);
                for (n, side) in [l1, l2].into_iter().enumerate() {
                    if let Some(l) = side {
                        parts.extend(
                            l.fields
                                .iter()
                                .enumerate()
                                .filter(|(i, _)| *i + 1 != fields[n])
                                .map(|(_, f)| *f),
                        );
                    }
                }
            }
        }
        for (i, p) in parts.iter().enumerate() {
            if i > 0 {
                out.push(out_sep);
            }
            out.push_str(p);
        }
        out.push('\n');
    };

    if opts.header {
        let h1 = (!lines1.is_empty()).then(|| lines1.remove(0));
        let h2 = (!lines2.is_empty()).then(|| lines2.remove(0));
        if h1.is_some() || h2.is_some() {
            emit(&mut out, h1.as_ref(), h2.as_ref());
        }
    }

    let (mut i, mut j) = (0, 0);
    while i < lines1.len() || j < lines2.len() {
        let ord = match (lines1.get(i), lines2.get(j)) {
            (Some(a), Some(b)) => a.key.cmp(&b.key),
            (Some(_), None) => std::cmp::Ordering::Less,
            _ => std::cmp::Ordering::Greater,
        };
        match ord {
            std::cmp::Ordering::Less => {
                if opts.unpaired[0] {
                    emit(&mut out, Some(&lines1[i]), None);
                }
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                if opts.unpaired[1] {
                    emit(&mut out, None, Some(&lines2[j]));
                }
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                let key = lines1[i].key.clone();
                let end1 = i + lines1[i..].iter().take_while(|l| l.key == key).count();
                let end2 = j + lines2[j..].iter().take_while(|l| l.key == key).count();
                if !opts.only_unpaired {
                    for a in &lines1[i..end1] {
                        for b in &lines2[j..end2] {
                            emit(&mut out, Some(a), Some(b));
                        }
                    }
                }
                i = end1;
                j = end2;
            }
        }
    }
    out
}

fn parse_field_number(option: &str, value: &str) -> std::result::Result<usize, String> {
    let field = value.parse().unwrap_or(1);
    if field == 0 {
        return Err(format!("join: invalid field number for {}: 0\n", option));
    }
    Ok(field)
}

async fn read_input(
    fs: &dyn crate::fs::FileSystem,
    cwd: &std::path::Path,
    file: &str,
    stdin: Option<&str>,
) -> Result<String> {
    if file == "-" {
        Ok(stdin.unwrap_or("").to_string())
    } else {
        let path = resolve_path(cwd, file);
        let bytes = fs.read_file(&path).await?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFs};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    async fn run_join(args: &[&str], fs: Arc<dyn FileSystem>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };
        Join.execute(ctx).await.expect("join failed")
    }

    #[tokio::test]
    async fn test_join_basic() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.write_file(Path::new("/f1"), b"a 1\nb 2\nc 3")
            .await
            .unwrap();
        fs.write_file(Path::new("/f2"), b"a x\nb y\nc z")
            .await
            .unwrap();
        let result = run_join(&["/f1", "/f2"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("a 1 x"));
        assert!(result.stdout.contains("b 2 y"));
        assert!(result.stdout.contains("c 3 z"));
    }

    #[tokio::test]
    async fn test_join_custom_field() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.write_file(Path::new("/f1"), b"x a\ny b").await.unwrap();
        fs.write_file(Path::new("/f2"), b"a 1\nb 2").await.unwrap();
        let result = run_join(&["-1", "2", "/f1", "/f2"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("a x 1"));
    }

    #[tokio::test]
    async fn test_join_custom_separator() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.write_file(Path::new("/f1"), b"a:1\nb:2").await.unwrap();
        fs.write_file(Path::new("/f2"), b"a:x\nb:y").await.unwrap();
        let result = run_join(&["-t", ":", "/f1", "/f2"], fs).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("a:1:x"));
    }

    #[tokio::test]
    async fn test_join_missing_operand() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_join(&["/f1"], fs).await;
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_join_unpairable() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.write_file(Path::new("/f1"), b"a 1\nb 2\nc 3")
            .await
            .unwrap();
        fs.write_file(Path::new("/f2"), b"a x\nc z").await.unwrap();
        let result = run_join(&["-a", "1", "/f1", "/f2"], fs).await;
        assert_eq!(result.exit_code, 0);
        // "b 2" should appear as unpairable from file1
        assert!(result.stdout.contains("b 2"));
    }

    #[tokio::test]
    async fn test_join_rejects_zero_file1_field() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.write_file(Path::new("/f1"), b"a 1").await.unwrap();
        fs.write_file(Path::new("/f2"), b"a x").await.unwrap();

        let result = run_join(&["-1", "0", "/f1", "/f2"], fs).await;

        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid field number"));
    }

    #[tokio::test]
    async fn test_join_rejects_zero_file2_field() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        fs.write_file(Path::new("/f1"), b"a 1").await.unwrap();
        fs.write_file(Path::new("/f2"), b"a x").await.unwrap();

        let result = run_join(&["-2", "0", "/f1", "/f2"], fs).await;

        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid field number"));
    }

    #[tokio::test]
    async fn test_join_rejects_unknown_option() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let result = run_join(&["-Q", "/f1", "/f2"], fs).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }
}
