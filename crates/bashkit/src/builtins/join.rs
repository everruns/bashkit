//! join builtin command - join lines of two sorted files on a common field
//!
//! Decisions: GNU semantics. Without `-t`, fields are separated by runs of
//! blanks (leading blanks ignored) and output uses one space; with `-t C`
//! every `C` separates and output uses `C`. Equal keys pair as a cartesian
//! product. `-e` fills missing fields only where `-o` names them, as in GNU.
//! Decisions: generated output and parsed rows own shared-budget capacity;
//! scans and emissions charge work before copying, including suppressed rows.

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::limits::{BudgetedString, BudgetedVec, ExecutionBudget};

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

        let budget = ctx
            .execution_budget()
            .map(|budget| {
                budget
                    .try_with(Clone::clone)
                    .map_err(|_| crate::Error::Cancelled)
            })
            .transpose()?;
        let budget = budget.as_ref();
        let content1 = read_input(&ctx, files[0], ctx.stdin.map(|stdin| &**stdin), budget).await?;
        let content2 = read_input(&ctx, files[1], None, budget).await?;

        let out = join_text(&opts, &content1, &content2, budget)?;
        let (stdout, _storage) = crate::StreamData::from_budgeted_string(out, budget)?;
        Ok(ExecResult::ok(stdout))
    }
}

fn split_fields<'a>(
    line: &'a str,
    sep: Option<char>,
    budget: Option<&ExecutionBudget>,
) -> Result<BudgetedVec<&'a str>> {
    let mut fields = BudgetedVec::new(budget)?;
    for field in line.split(|c| sep.map_or(c == ' ' || c == '\t', |sep| c == sep)) {
        work(budget, 1 + field.len() as u64)?;
        if sep.is_some() || !field.is_empty() {
            fields.try_push(field)?;
        }
    }
    Ok(fields)
}

struct Line<'a> {
    fields: BudgetedVec<&'a str>,
    key: BudgetedString,
}

fn work(budget: Option<&ExecutionBudget>, units: u64) -> Result<()> {
    if let Some(budget) = budget {
        budget.consume_work(units)?;
    }
    Ok(())
}

fn parse_lines<'a>(
    text: &'a str,
    field: usize,
    opts: &JoinOptions,
    budget: Option<&ExecutionBudget>,
) -> Result<BudgetedVec<Line<'a>>> {
    let mut lines = BudgetedVec::new(budget)?;
    for line in text.lines() {
        work(budget, 1)?;
        let fields = split_fields(line, opts.separator, budget)?;
        let source = fields.get(field - 1).copied().unwrap_or("");
        let mut key = BudgetedString::new(budget)?;
        if opts.ignore_case {
            // Preserve contextual Unicode lowercasing (for example final sigma).
            // Unicode mapping expands by at most 3x; reserve another 2x for
            // String's geometric growth and its minimum allocation capacity.
            work(budget, 1 + source.len() as u64)?;
            let capacity = source
                .len()
                .checked_mul(6)
                .ok_or_else(|| crate::LimitExceeded::Memory("join: key size overflow".into()))?
                .max(32);
            let _lower_lease = budget.map(|b| b.lease_bytes(capacity)).transpose()?;
            key = BudgetedString::try_from_string(source.to_lowercase(), budget)?;
        } else {
            key.try_push_str(source)?;
        }
        lines.try_push(Line { fields, key })?;
    }
    Ok(lines)
}

// THREAT[TM-DOS-138]: admit every output append and poll synchronous join work.
fn join_text(
    opts: &JoinOptions,
    text1: &str,
    text2: &str,
    budget: Option<&ExecutionBudget>,
) -> Result<BudgetedString> {
    let lines1 = parse_lines(text1, opts.field1, opts, budget)?;
    let lines2 = parse_lines(text2, opts.field2, opts, budget)?;
    let out_sep = opts.separator.unwrap_or(' ');
    let fields = [opts.field1, opts.field2];
    // `-o auto`: the join field, then every other field of each file's first line.
    let mut auto_format = BudgetedVec::new(budget)?;
    if opts.format.is_none() && opts.auto_format {
        auto_format.try_push(OutField::Key)?;
        for (n, first) in [lines1.first(), lines2.first()].into_iter().enumerate() {
            let count = first.map_or(0, |l| l.fields.len());
            for m in 1..=count {
                work(budget, 1)?;
                if m != fields[n] {
                    auto_format.try_push(OutField::Field(n + 1, m))?;
                }
            }
        }
    }
    let format = opts
        .format
        .as_deref()
        .or_else(|| opts.auto_format.then_some(&*auto_format));

    let mut out = BudgetedString::new(budget)?;
    let emit = |out: &mut BudgetedString, l1: Option<&Line>, l2: Option<&Line>| -> Result<()> {
        work(budget, 1)?;
        let key = l1.or(l2).map_or("", |l| {
            let f = if l1.is_some() { fields[0] } else { fields[1] };
            l.fields.get(f - 1).copied().unwrap_or("")
        });
        let mut first = true;
        let mut append = |part: &str| -> Result<()> {
            work(budget, 1 + part.len() as u64)?;
            if !first {
                out.try_push(out_sep)?;
            }
            first = false;
            out.try_push_str(part)?;
            Ok(())
        };
        let empty = opts.empty.as_deref().unwrap_or("");
        match format {
            Some(format) => {
                for item in format {
                    append(match *item {
                        OutField::Key => key,
                        OutField::Field(n, m) => {
                            let side = if n == 1 { l1 } else { l2 };
                            side.and_then(|l| l.fields.get(m - 1).copied())
                                .unwrap_or(empty)
                        }
                    })?;
                }
            }
            None => {
                append(key)?;
                for (n, side) in [l1, l2].into_iter().enumerate() {
                    if let Some(l) = side {
                        for (i, field) in l.fields.iter().enumerate() {
                            if i + 1 != fields[n] {
                                append(field)?;
                            }
                        }
                    }
                }
            }
        }
        out.try_push('\n')?;
        Ok(())
    };

    if opts.header {
        let h1 = lines1.first();
        let h2 = lines2.first();
        if h1.is_some() || h2.is_some() {
            emit(&mut out, h1, h2)?;
        }
    }

    let (mut i, mut j) = if opts.header {
        (
            usize::from(!lines1.is_empty()),
            usize::from(!lines2.is_empty()),
        )
    } else {
        (0, 0)
    };
    while i < lines1.len() || j < lines2.len() {
        work(budget, 1)?;
        let ord = match (lines1.get(i), lines2.get(j)) {
            (Some(a), Some(b)) => {
                work(budget, 1 + a.key.len().min(b.key.len()) as u64)?;
                (*a.key).cmp(&*b.key)
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            _ => std::cmp::Ordering::Greater,
        };
        match ord {
            std::cmp::Ordering::Less => {
                if opts.unpaired[0] {
                    emit(&mut out, Some(&lines1[i]), None)?;
                }
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                if opts.unpaired[1] {
                    emit(&mut out, None, Some(&lines2[j]))?;
                }
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                let key = &*lines1[i].key;
                let mut end1 = i;
                while end1 < lines1.len() {
                    work(budget, 1 + key.len().min(lines1[end1].key.len()) as u64)?;
                    if &*lines1[end1].key != key {
                        break;
                    }
                    end1 += 1;
                }
                let mut end2 = j;
                while end2 < lines2.len() {
                    work(budget, 1 + key.len().min(lines2[end2].key.len()) as u64)?;
                    if &*lines2[end2].key != key {
                        break;
                    }
                    end2 += 1;
                }
                if !opts.only_unpaired {
                    for a in &lines1[i..end1] {
                        for b in &lines2[j..end2] {
                            emit(&mut out, Some(a), Some(b))?;
                        }
                    }
                }
                i = end1;
                j = end2;
            }
        }
    }
    Ok(out)
}

fn parse_field_number(option: &str, value: &str) -> std::result::Result<usize, String> {
    let field = value.parse().unwrap_or(1);
    if field == 0 {
        return Err(format!("join: invalid field number for {}: 0\n", option));
    }
    Ok(field)
}

async fn read_input(
    ctx: &Context<'_>,
    file: &str,
    stdin: Option<&str>,
    budget: Option<&ExecutionBudget>,
) -> Result<BudgetedString> {
    let mut text = BudgetedString::new(budget)?;
    if file == "-" {
        let stdin = stdin.unwrap_or("");
        work(budget, 1 + stdin.len() as u64)?;
        text.try_push_str(stdin)?;
    } else {
        let path = resolve_path(ctx.cwd, file);
        let size = usize::try_from(ctx.fs.stat(&path).await?.size)
            .map_err(|_| crate::LimitExceeded::Memory("join: input size overflow".into()))?;
        ctx.consume_budget_input(size)?;
        let _read_lease = ctx.lease_budget_bytes(size)?;
        let bytes = ctx.fs.read_file(&path).await?;
        ctx.consume_budget_input(bytes.len().saturating_sub(size))?;
        let _extra_lease = ctx.lease_budget_bytes(bytes.capacity().saturating_sub(size))?;
        for chunk in bytes.utf8_chunks() {
            work(budget, 1 + chunk.valid().len() as u64)?;
            text.try_push_str(chunk.valid())?;
            if !chunk.invalid().is_empty() {
                text.try_push('\u{fffd}')?;
            }
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFs};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn options() -> JoinOptions {
        JoinOptions {
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
        }
    }

    #[test]
    fn ordinary_join_output_growth_requires_admission() {
        // Parsed storage fits; the final output capacity growth does not.
        let parsed = 4 * std::mem::size_of::<Line>() + 8 * std::mem::size_of::<&str>() + 4;
        let budget = ExecutionBudget::new(
            &crate::ExecutionLimits::new().max_live_intermediate_bytes((parsed + 23) as u64),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let error = join_text(&options(), "a 1\na 2\n", "a x\na y\n", Some(&budget)).unwrap_err();
        assert!(error.to_string().contains("live intermediate"), "{error}");
    }

    #[test]
    fn ordinary_join_preserves_contextual_unicode_lowercasing() {
        let mut opts = options();
        opts.ignore_case = true;
        let out = join_text(&opts, "ΟΣ 1\n", "ος x\n", None).unwrap();
        assert_eq!(&*out, "ΟΣ 1 x\n");
    }

    #[test]
    fn ordinary_join_emissions_consume_work() {
        let budget = ExecutionBudget::new(
            &crate::ExecutionLimits::new().max_work_units(40),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let error = join_text(&options(), "a 1\na 2\n", "a x\na y\n", Some(&budget)).unwrap_err();
        assert!(error.to_string().contains("work units"), "{error}");
    }

    #[test]
    fn ordinary_join_polls_cancelled_and_expired_budgets() {
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let budget = ExecutionBudget::new(&crate::ExecutionLimits::new(), cancelled.clone());
        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
        let error = join_text(&options(), "a 1\n", "a x\n", Some(&budget)).unwrap_err();
        assert!(error.to_string().contains("cancel"), "{error}");
        let budget = ExecutionBudget::new(
            &crate::ExecutionLimits::new().timeout(std::time::Duration::ZERO),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        std::thread::sleep(std::time::Duration::from_millis(120));
        let error = join_text(&options(), "a 1\n", "a x\n", Some(&budget)).unwrap_err();
        assert!(matches!(
            error,
            crate::Error::ResourceLimit(crate::LimitExceeded::Timeout(_))
        ));
    }

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
