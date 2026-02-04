//! jq - JSON processor builtin
//!
//! Implements jq functionality using the jaq library.
//!
//! Usage:
//!   echo '{"name":"foo"}' | jq '.name'
//!   jq '.[] | .id' < data.json

use async_trait::async_trait;
use jaq_core::{load, Compiler, Ctx, RcIter};
use jaq_json::Val;

use super::{Builtin, Context};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;

/// jq command - JSON processor
pub struct Jq;

#[async_trait]
impl Builtin for Jq {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Parse arguments for flags
        let mut raw_output = false;
        let mut compact_output = false;
        let mut null_input = false;
        let mut filter = ".";

        for arg in ctx.args {
            if arg == "-r" || arg == "--raw-output" {
                raw_output = true;
            } else if arg == "-c" || arg == "--compact-output" {
                compact_output = true;
            } else if arg == "-n" || arg == "--null-input" {
                null_input = true;
            } else if !arg.starts_with('-') {
                filter = arg;
                break;
            }
        }

        // Get input from stdin
        let input = ctx.stdin.unwrap_or("");

        // If no input and not null_input mode, return empty
        if input.trim().is_empty() && !null_input {
            return Ok(ExecResult::ok(String::new()));
        }

        // Set up the loader with standard library definitions
        let loader = load::Loader::new(jaq_std::defs().chain(jaq_json::defs()));
        let arena = load::Arena::default();

        // Parse the filter
        let program = load::File {
            code: filter,
            path: (),
        };

        let modules = loader.load(&arena, program).map_err(|errs| {
            Error::Execution(format!(
                "jq: parse error: {}",
                errs.into_iter()
                    .map(|e| format!("{:?}", e))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;

        // Compile the filter
        let filter = Compiler::default()
            .with_funs(jaq_std::funs().chain(jaq_json::funs()))
            .compile(modules)
            .map_err(|errs| {
                Error::Execution(format!(
                    "jq: compile error: {}",
                    errs.into_iter()
                        .map(|e| format!("{:?}", e))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;

        // Process input as JSON
        let mut output = String::new();

        // Build list of inputs to process
        let inputs_to_process: Vec<Val> = if null_input {
            // -n flag: use null as input
            vec![Val::from(serde_json::Value::Null)]
        } else {
            // Process each line of input as JSON
            let mut vals = Vec::new();
            for line in input.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let json_input: serde_json::Value = serde_json::from_str(line)
                    .map_err(|e| Error::Execution(format!("jq: invalid JSON: {}", e)))?;
                vals.push(Val::from(json_input));
            }
            vals
        };

        for jaq_input in inputs_to_process {
            // Create empty inputs iterator
            let inputs = RcIter::new(core::iter::empty());

            // Run the filter
            let ctx = Ctx::new([], &inputs);
            for result in filter.run((ctx, jaq_input)) {
                match result {
                    Ok(val) => {
                        // Convert back to serde_json::Value and format
                        let json: serde_json::Value = val.into();
                        // In raw mode, strings are output without quotes
                        if raw_output {
                            if let serde_json::Value::String(s) = json {
                                output.push_str(&s);
                                output.push('\n');
                            } else {
                                // For non-strings in raw mode, use compact or pretty based on -c flag
                                let formatted = if compact_output {
                                    serde_json::to_string(&json)
                                } else {
                                    match &json {
                                        serde_json::Value::Array(_)
                                        | serde_json::Value::Object(_) => {
                                            serde_json::to_string_pretty(&json)
                                        }
                                        _ => serde_json::to_string(&json),
                                    }
                                };
                                match formatted {
                                    Ok(s) => {
                                        output.push_str(&s);
                                        output.push('\n');
                                    }
                                    Err(e) => {
                                        return Err(Error::Execution(format!(
                                            "jq: output error: {}",
                                            e
                                        )));
                                    }
                                }
                            }
                        } else if compact_output {
                            // Compact mode: no pretty-printing
                            match serde_json::to_string(&json) {
                                Ok(s) => {
                                    output.push_str(&s);
                                    output.push('\n');
                                }
                                Err(e) => {
                                    return Err(Error::Execution(format!(
                                        "jq: output error: {}",
                                        e
                                    )));
                                }
                            }
                        } else {
                            // Use pretty-print for arrays/objects to match real jq behavior
                            let formatted = match &json {
                                serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                                    serde_json::to_string_pretty(&json)
                                }
                                _ => serde_json::to_string(&json),
                            };
                            match formatted {
                                Ok(s) => {
                                    output.push_str(&s);
                                    output.push('\n');
                                }
                                Err(e) => {
                                    return Err(Error::Execution(format!(
                                        "jq: output error: {}",
                                        e
                                    )));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        return Err(Error::Execution(format!("jq: runtime error: {:?}", e)));
                    }
                }
            }
        }

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn run_jq(filter: &str, input: &str) -> Result<String> {
        let jq = Jq;
        let fs = Arc::new(InMemoryFs::new());
        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args = vec![filter.to_string()];

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: Some(input),
            #[cfg(feature = "http_client")]
            http_client: None,
        };

        let result = jq.execute(ctx).await?;
        Ok(result.stdout)
    }

    #[tokio::test]
    async fn test_jq_identity() {
        let result = run_jq(".", r#"{"name":"test"}"#).await.unwrap();
        // Pretty-printed output to match real jq behavior
        assert_eq!(result.trim(), "{\n  \"name\": \"test\"\n}");
    }

    #[tokio::test]
    async fn test_jq_field_access() {
        let result = run_jq(".name", r#"{"name":"foo","id":42}"#).await.unwrap();
        assert_eq!(result.trim(), r#""foo""#);
    }

    #[tokio::test]
    async fn test_jq_array_index() {
        let result = run_jq(".[1]", r#"["a","b","c"]"#).await.unwrap();
        assert_eq!(result.trim(), r#""b""#);
    }

    #[tokio::test]
    async fn test_jq_nested() {
        let result = run_jq(".user.name", r#"{"user":{"name":"alice"}}"#)
            .await
            .unwrap();
        assert_eq!(result.trim(), r#""alice""#);
    }

    #[tokio::test]
    async fn test_jq_keys() {
        let result = run_jq("keys", r#"{"b":1,"a":2}"#).await.unwrap();
        // Pretty-printed array output to match real jq behavior
        assert_eq!(result.trim(), "[\n  \"a\",\n  \"b\"\n]");
    }

    #[tokio::test]
    async fn test_jq_length() {
        let result = run_jq("length", r#"[1,2,3,4,5]"#).await.unwrap();
        assert_eq!(result.trim(), "5");
    }

    async fn run_jq_with_args(args: &[&str], input: &str) -> Result<String> {
        let jq = Jq;
        let fs = Arc::new(InMemoryFs::new());
        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: Some(input),
            #[cfg(feature = "http_client")]
            http_client: None,
        };

        let result = jq.execute(ctx).await?;
        Ok(result.stdout)
    }

    #[tokio::test]
    async fn test_jq_raw_output() {
        let result = run_jq_with_args(&["-r", ".name"], r#"{"name":"test"}"#)
            .await
            .unwrap();
        assert_eq!(result.trim(), "test");
    }

    #[tokio::test]
    async fn test_jq_raw_output_long_flag() {
        let result = run_jq_with_args(&["--raw-output", ".name"], r#"{"name":"test"}"#)
            .await
            .unwrap();
        assert_eq!(result.trim(), "test");
    }
}
