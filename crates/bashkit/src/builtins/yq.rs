//! `yq` structured-data processor.
//!
//! Important decision: yq performs format conversion only. Expressions are
//! evaluated by the existing jq/jaq builtin, so Bashkit has one query language,
//! one evaluator, and one set of work/output/deadline mitigations.

use std::path::Path;

use async_trait::async_trait;
use serde::Deserialize;

use super::{Builtin, Context, Jq, read_text_file, resolve_path};
use crate::StreamData;
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::limits::ExecutionLimits;

const MAX_DEPTH: usize = 100;
const MAX_DOCUMENTS: usize = 4096;
type DataResult<T> = std::result::Result<T, anyhow::Error>;
const VERSION: &str = concat!("yq (bashkit) ", env!("CARGO_PKG_VERSION"), "\n");
const HELP: &str = "Usage: yq [OPTIONS] [EXPRESSION] [FILE...]\n\n\
Process YAML or JSON with Bashkit's jq-compatible expression engine.\n\n\
  -p, --input-format FORMAT    auto, yaml, or json\n\
  -o, --output-format FORMAT   yaml or json (default yaml)\n\
  -r, --raw-output             unwrap scalar strings\n\
  -c, --compact-output         compact JSON output\n\
  -e, --exit-status            fail for no output, null, or false\n\
  -s, --slurp                  read all documents into an array\n\
  -n, --null-input             evaluate once with null input\n\
  -i, --inplace                atomically update one input file\n\
  -I, --indent N               output indentation (0..9)\n\
  -N, --no-doc                 omit YAML document separators\n\
      --expression FILTER      force an ambiguous expression argument\n\
  -h, --help                   display this help\n\
  -V, --version                display version\n";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Auto,
    Yaml,
    Json,
}

struct Args {
    filter: String,
    files: Vec<String>,
    input: Format,
    output: Format,
    raw: bool,
    compact: bool,
    exit_status: bool,
    slurp: bool,
    null_input: bool,
    inplace: bool,
    no_doc: bool,
    indent: u8,
}

pub struct Yq;

#[async_trait]
impl Builtin for Yq {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let args = match parse_args(ctx.args) {
            Ok(args) => args,
            Err(done) => return Ok(*done),
        };

        if args.inplace
            && (args.files.len() != 1
                || args.files.first().is_some_and(|file| file == "-")
                || args.null_input)
        {
            return Ok(usage_error("-i/--inplace requires exactly one input file"));
        }

        let mut input_documents = Vec::new();
        if !args.null_input {
            if args.files.is_empty() {
                let input = ctx.stdin.map(ToString::to_string).unwrap_or_default();
                ctx.consume_budget_input(input.len())?;
                match parse_documents(&input, args.input, None) {
                    Ok(values) => {
                        if let Err(error) = append_documents(&mut input_documents, values) {
                            return Ok(data_error(error));
                        }
                    }
                    Err(error) => return Ok(data_error(error)),
                }
            } else {
                for file in &args.files {
                    if file == "-" {
                        let input = ctx.stdin.map(ToString::to_string).unwrap_or_default();
                        ctx.consume_budget_input(input.len())?;
                        match parse_documents(&input, args.input, None) {
                            Ok(values) => {
                                if let Err(error) = append_documents(&mut input_documents, values) {
                                    return Ok(data_error(error));
                                }
                            }
                            Err(error) => return Ok(data_error(error)),
                        }
                        continue;
                    }
                    let path = resolve_path(ctx.cwd, file);
                    let input = match read_text_file(ctx.fs.as_ref(), &path, "yq").await {
                        Ok(input) => input,
                        Err(error) => return Ok(error),
                    };
                    ctx.consume_budget_input(input.len())?;
                    match parse_documents(&input, args.input, Some(file)) {
                        Ok(values) => {
                            if let Err(error) = append_documents(&mut input_documents, values) {
                                return Ok(data_error(error));
                            }
                        }
                        Err(error) => return Ok(data_error(error)),
                    }
                }
            }
        }

        let json_input = match serialize_json_stream(&input_documents) {
            Ok(input) => input,
            Err(error) => return Ok(data_error(error)),
        };
        let jq_stdin = StreamData::from(json_input);
        let mut jq_args = vec!["-c".to_string()];
        if args.slurp {
            jq_args.push("-s".to_string());
        }
        if args.null_input {
            jq_args.push("-n".to_string());
        }
        if args.exit_status {
            jq_args.push("-e".to_string());
        }
        jq_args.push(args.filter.clone());

        let max_output = match ctx.execution_extension::<ExecutionLimits>() {
            Some(limits) => limits
                .try_with(|limits| limits.max_stdout_bytes)
                .map_err(|_| crate::Error::Cancelled)?,
            None => ExecutionLimits::default().max_stdout_bytes,
        };
        let fs = ctx.fs.clone();
        let cwd = ctx.cwd.clone();
        let inplace_path = if args.inplace {
            args.files.first().map(|file| resolve_path(&cwd, file))
        } else {
            None
        };

        let jq_result = Jq
            .execute(Context {
                args: &jq_args,
                env: ctx.env,
                variables: ctx.variables,
                cwd: ctx.cwd,
                fs: ctx.fs,
                stdin: Some(&jq_stdin),
                #[cfg(feature = "http_client")]
                http_client: ctx.http_client,
                #[cfg(feature = "git")]
                git_client: ctx.git_client,
                #[cfg(feature = "ssh")]
                ssh_client: ctx.ssh_client,
                shell: ctx.shell,
            })
            .await?;

        if !jq_result.stderr.is_empty() {
            return Ok(relabel_jq_error(jq_result));
        }
        let jq_exit_code = jq_result.exit_code;

        let values = match parse_json_results(&jq_result.stdout) {
            Ok(values) => values,
            Err(error) => return Ok(data_error(error)),
        };
        let rendered = match render_results(&values, &args) {
            Ok(rendered) => rendered,
            Err(error) => return Ok(data_error(error)),
        };
        if rendered.len() > max_output {
            return Ok(ExecResult::err(
                format!("yq: output limit exceeded ({max_output} bytes)\n"),
                1,
            ));
        }

        if jq_exit_code != 0 {
            return Ok(ExecResult::with_code(
                if args.inplace {
                    String::new()
                } else {
                    rendered
                },
                jq_exit_code,
            ));
        }

        if let Some(path) = inplace_path {
            if let Err(message) = atomic_replace(fs.as_ref(), &path, rendered.as_bytes()).await {
                return Ok(ExecResult::err(format!("yq: {message}\n"), 1));
            }
            return Ok(ExecResult::ok(String::new()));
        }

        Ok(ExecResult::ok(rendered))
    }
}

type ArgResult<T> = std::result::Result<T, Box<ExecResult>>;

fn parse_args(raw: &[String]) -> ArgResult<Args> {
    let mut args = Args {
        filter: ".".into(),
        files: Vec::new(),
        input: Format::Auto,
        output: Format::Yaml,
        raw: false,
        compact: false,
        exit_status: false,
        slurp: false,
        null_input: false,
        inplace: false,
        no_doc: false,
        indent: 2,
    };
    let mut positional = Vec::new();
    let mut explicit_filter = None;
    let mut i = 0;
    while i < raw.len() {
        let arg = raw[i].as_str();
        match arg {
            "-h" | "--help" => return Err(Box::new(ExecResult::ok(HELP))),
            "-V" | "--version" => {
                return Err(Box::new(ExecResult::ok(VERSION)));
            }
            "--raw-output" => args.raw = true,
            "--compact-output" => args.compact = true,
            "--exit-status" => args.exit_status = true,
            "--slurp" => args.slurp = true,
            "--null-input" => args.null_input = true,
            "--inplace" => args.inplace = true,
            "--no-doc" => args.no_doc = true,
            "--no-colors" | "--colors" | "--prettyPrint" => {}
            "--" => {
                positional.extend(raw[i + 1..].iter().cloned());
                break;
            }
            "-p" | "--input-format" | "-o" | "--output-format" | "-I" | "--indent" => {
                let value = raw
                    .get(i + 1)
                    .ok_or_else(|| boxed_usage_error(format!("{arg} requires an argument")))?;
                apply_value_option(&mut args, arg, value)?;
                i += 2;
                continue;
            }
            "--expression" => {
                let value = raw
                    .get(i + 1)
                    .ok_or_else(|| boxed_usage_error("--expression requires an argument"))?;
                explicit_filter = Some(value.clone());
                i += 2;
                continue;
            }
            _ if arg.starts_with("--input-format=") => {
                apply_value_option(&mut args, "--input-format", &arg[15..])?;
            }
            _ if arg.starts_with("--output-format=") => {
                apply_value_option(&mut args, "--output-format", &arg[16..])?;
            }
            _ if arg.starts_with("--indent=") => {
                apply_value_option(&mut args, "--indent", &arg[9..])?;
            }
            _ if arg.starts_with("--expression=") => {
                explicit_filter = Some(arg[13..].to_string());
            }
            _ if arg.starts_with('-') && arg.len() > 1 => parse_short_flags(&mut args, arg)?,
            _ => positional.push(arg.to_string()),
        }
        i += 1;
    }

    if positional
        .first()
        .is_some_and(|value| matches!(value.as_str(), "e" | "eval"))
    {
        positional.remove(0);
    } else if positional
        .first()
        .is_some_and(|value| matches!(value.as_str(), "ea" | "eval-all"))
    {
        return Err(boxed_usage_error(
            "eval-all is not supported; use -s with a jq array expression",
        ));
    }

    if let Some(filter) = explicit_filter {
        args.filter = filter;
    } else if let Some(first) = positional.first()
        && (looks_like_expression(first)
            || (positional.len() > 1 && !looks_like_input_filename(first)))
    {
        args.filter = positional.remove(0);
    }
    args.files = positional;
    Ok(args)
}

fn parse_short_flags(args: &mut Args, arg: &str) -> ArgResult<()> {
    let mut chars = arg[1..].chars().peekable();
    while let Some(flag) = chars.next() {
        match flag {
            'r' => args.raw = true,
            'c' => args.compact = true,
            'e' => args.exit_status = true,
            's' => args.slurp = true,
            'n' => args.null_input = true,
            'i' => args.inplace = true,
            'N' => args.no_doc = true,
            'C' | 'M' | 'P' => {}
            'j' => args.output = Format::Json,
            'y' => args.output = Format::Yaml,
            'p' | 'o' | 'I' => {
                let value: String = chars.collect();
                if value.is_empty() {
                    return Err(boxed_usage_error(format!("-{flag} requires an argument")));
                }
                apply_value_option(args, &format!("-{flag}"), value.trim_start_matches('='))?;
                return Ok(());
            }
            _ => return Err(boxed_usage_error(format!("unknown option '-{flag}'"))),
        }
    }
    Ok(())
}

fn apply_value_option(args: &mut Args, option: &str, value: &str) -> ArgResult<()> {
    match option {
        "-p" | "--input-format" => args.input = parse_format(value, true)?,
        "-o" | "--output-format" => args.output = parse_format(value, false)?,
        "-I" | "--indent" => {
            args.indent = value
                .parse::<u8>()
                .map_err(|_| boxed_usage_error(format!("invalid indentation '{value}'")))?;
            if args.indent > 9 {
                return Err(boxed_usage_error("indentation must be between 0 and 9"));
            }
        }
        _ => unreachable!("known value option"),
    }
    Ok(())
}

fn parse_format(value: &str, allow_auto: bool) -> ArgResult<Format> {
    match value {
        "auto" | "a" if allow_auto => Ok(Format::Auto),
        "yaml" | "y" | "yml" => Ok(Format::Yaml),
        "json" | "j" => Ok(Format::Json),
        _ => Err(boxed_usage_error(format!(
            "unsupported format '{value}' (supported: yaml, json{})",
            if allow_auto { ", auto" } else { "" }
        ))),
    }
}

fn looks_like_expression(value: &str) -> bool {
    value.starts_with(['.', '[', '{', '$', '"', '\'', '-', '+'])
        || value.contains(['(', '|'])
        || value.parse::<f64>().is_ok()
        || matches!(
            value.split(['(', ' ', '|']).next(),
            Some(
                "map"
                    | "select"
                    | "keys"
                    | "length"
                    | "type"
                    | "add"
                    | "reduce"
                    | "if"
                    | "true"
                    | "false"
                    | "null"
                    | "empty"
                    | "paths"
            )
        )
}

fn looks_like_input_filename(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    value == "-"
        || value.contains('/')
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with(".json")
}

fn parse_documents(
    input: &str,
    requested: Format,
    filename: Option<&str>,
) -> DataResult<Vec<serde_json::Value>> {
    let format = match requested {
        Format::Auto
            if filename.is_some_and(|name| name.to_ascii_lowercase().ends_with(".json")) =>
        {
            Format::Json
        }
        Format::Auto => Format::Yaml,
        format => format,
    };
    match format {
        Format::Json => parse_json_documents(input),
        Format::Yaml => parse_yaml_documents(input),
        Format::Auto => unreachable!(),
    }
}

fn append_documents(
    destination: &mut Vec<serde_json::Value>,
    documents: Vec<serde_json::Value>,
) -> DataResult<()> {
    if destination
        .len()
        .checked_add(documents.len())
        .is_none_or(|total| total > MAX_DOCUMENTS)
    {
        anyhow::bail!("yq: document limit exceeded ({MAX_DOCUMENTS})");
    }
    destination.extend(documents);
    Ok(())
}

fn parse_json_documents(input: &str) -> DataResult<Vec<serde_json::Value>> {
    let mut values = Vec::new();
    for value in serde_json::Deserializer::from_str(input).into_iter::<serde_json::Value>() {
        if values.len() >= MAX_DOCUMENTS {
            anyhow::bail!("yq: document limit exceeded ({MAX_DOCUMENTS})");
        }
        let value = value.map_err(|error| anyhow::anyhow!("yq: invalid JSON: {error}"))?;
        check_depth(&value, 0)?;
        values.push(value);
    }
    Ok(values)
}

// THREAT[TM-DOS-101]: serde_yaml_ng rejects nesting beyond 128 while this
// conversion enforces Bashkit's lower shared structured-data depth of 100.
fn parse_yaml_documents(input: &str) -> DataResult<Vec<serde_json::Value>> {
    let mut values = Vec::new();
    for document in serde_yaml_ng::Deserializer::from_str(input) {
        if values.len() >= MAX_DOCUMENTS {
            anyhow::bail!("yq: document limit exceeded ({MAX_DOCUMENTS})");
        }
        let yaml = serde_yaml_ng::Value::deserialize(document)
            .map_err(|error| anyhow::anyhow!("yq: invalid YAML: {error}"))?;
        let json = yaml_to_json(yaml, 0)?;
        values.push(json);
    }
    Ok(values)
}

fn yaml_to_json(value: serde_yaml_ng::Value, depth: usize) -> DataResult<serde_json::Value> {
    if depth > MAX_DEPTH {
        anyhow::bail!("yq: nesting too deep ({depth} levels, max {MAX_DEPTH})");
    }
    Ok(match value {
        serde_yaml_ng::Value::Null => serde_json::Value::Null,
        serde_yaml_ng::Value::Bool(value) => serde_json::Value::Bool(value),
        serde_yaml_ng::Value::Number(value) => {
            if value.as_f64().is_some_and(|number| !number.is_finite()) {
                anyhow::bail!("yq: non-finite YAML number cannot be represented as JSON");
            }
            serde_json::to_value(value)
                .map_err(|_| anyhow::anyhow!("yq: YAML number cannot be represented as JSON"))?
        }
        serde_yaml_ng::Value::String(value) => serde_json::Value::String(value),
        serde_yaml_ng::Value::Sequence(values) => serde_json::Value::Array(
            values
                .into_iter()
                .map(|value| yaml_to_json(value, depth + 1))
                .collect::<DataResult<Vec<_>>>()?,
        ),
        serde_yaml_ng::Value::Mapping(values) => {
            let mut object = serde_json::Map::new();
            for (key, value) in values {
                let serde_yaml_ng::Value::String(key) = key else {
                    anyhow::bail!("yq: YAML mapping keys must be strings");
                };
                object.insert(key, yaml_to_json(value, depth + 1)?);
            }
            serde_json::Value::Object(object)
        }
        serde_yaml_ng::Value::Tagged(_) => {
            anyhow::bail!("yq: custom YAML tags are not supported");
        }
    })
}

fn check_depth(value: &serde_json::Value, depth: usize) -> DataResult<()> {
    if depth > MAX_DEPTH {
        anyhow::bail!("yq: nesting too deep ({depth} levels, max {MAX_DEPTH})");
    }
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                check_depth(value, depth + 1)?;
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                check_depth(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn serialize_json_stream(values: &[serde_json::Value]) -> DataResult<String> {
    let mut out = String::new();
    for value in values {
        out.push_str(&serde_json::to_string(value)?);
        out.push('\n');
    }
    Ok(out)
}

fn parse_json_results(output: &StreamData) -> DataResult<Vec<serde_json::Value>> {
    serde_json::Deserializer::from_str(output)
        .into_iter::<serde_json::Value>()
        .map(|value| value.map_err(Into::into))
        .collect()
}

fn render_results(values: &[serde_json::Value], args: &Args) -> DataResult<String> {
    let mut output = String::new();
    for (index, value) in values.iter().enumerate() {
        if args.output == Format::Json {
            if args.raw
                && let serde_json::Value::String(value) = value
            {
                output.push_str(value);
            } else if args.compact || args.indent == 0 {
                output.push_str(&serde_json::to_string(value)?);
            } else {
                let indent = vec![b' '; usize::from(args.indent)];
                let formatter = serde_json::ser::PrettyFormatter::with_indent(&indent);
                let mut bytes = Vec::new();
                let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
                serde::Serialize::serialize(value, &mut serializer)?;
                output.push_str(std::str::from_utf8(&bytes)?);
            }
            output.push('\n');
            continue;
        }

        if index > 0 && !args.no_doc {
            output.push_str("---\n");
        }
        if let serde_json::Value::String(value) = value {
            output.push_str(value);
            output.push('\n');
        } else {
            output.push_str(&serde_yaml_ng::to_string(value)?);
        }
    }
    Ok(output)
}

// THREAT[TM-FS-016]: write a sibling temporary file and rename only after
// successful evaluation and serialization; failures leave the source intact.
async fn atomic_replace(
    fs: &dyn crate::fs::FileSystem,
    target: &Path,
    content: &[u8],
) -> std::result::Result<(), String> {
    let parent = target.parent().unwrap_or_else(|| Path::new("/"));
    #[cfg(feature = "failpoints")]
    if injected_failure("yq::temp_allocate", "exhausted") {
        return Err("cannot allocate temporary file".to_string());
    }
    let mut temp = None;
    for _ in 0..16 {
        let mut suffix = [0u8; 8];
        getrandom::fill(&mut suffix)
            .map_err(|_| "cannot generate temporary filename".to_string())?;
        let suffix = suffix
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let candidate = parent.join(format!(".bashkit-yq-{suffix}.tmp"));
        if !fs
            .exists(&candidate)
            .await
            .map_err(|error| error.to_string())?
        {
            temp = Some(candidate);
            break;
        }
    }
    let temp = temp.ok_or_else(|| "cannot allocate temporary file".to_string())?;
    if let Err(error) = fs.write_file(&temp, content).await {
        let _ = fs.remove(&temp, false).await;
        return Err(format!("cannot write temporary file: {error}"));
    }
    #[cfg(feature = "failpoints")]
    if injected_failure("yq::temp_chmod", "error") {
        let _ = fs.remove(&temp, false).await;
        return Err("cannot preserve file mode: injected failure".to_string());
    }
    if let Ok(metadata) = fs.stat(target).await
        && let Err(error) = fs.chmod(&temp, metadata.mode).await
    {
        let _ = fs.remove(&temp, false).await;
        return Err(format!("cannot preserve file mode: {error}"));
    }
    #[cfg(feature = "failpoints")]
    if injected_failure("yq::temp_rename", "error") {
        let _ = fs.remove(&temp, false).await;
        return Err(format!(
            "cannot replace '{}': injected failure",
            target.display()
        ));
    }
    if let Err(error) = fs.rename(&temp, target).await {
        let _ = fs.remove(&temp, false).await;
        return Err(format!("cannot replace '{}': {error}", target.display()));
    }
    Ok(())
}

#[cfg(feature = "failpoints")]
fn injected_failure(name: &str, expected: &str) -> bool {
    fail::eval(name, |action| action.as_deref() == Some(expected)).unwrap_or(false)
}

fn relabel_jq_error(mut result: ExecResult) -> ExecResult {
    let stderr = result.stderr.to_string().replace("jq:", "yq:");
    result.stderr = StreamData::from(stderr);
    result
}

fn usage_error(message: impl std::fmt::Display) -> ExecResult {
    ExecResult::err(format!("yq: {message}\nUse yq --help for help.\n"), 2)
}

fn boxed_usage_error(message: impl std::fmt::Display) -> Box<ExecResult> {
    Box::new(usage_error(message))
}

fn data_error(error: anyhow::Error) -> ExecResult {
    let message = error.to_string();
    let message = message.strip_prefix("yq: ").unwrap_or(&message);
    let capped: String = message.chars().take(960).collect();
    ExecResult::err(format!("yq: {capped}\n"), 1)
}
