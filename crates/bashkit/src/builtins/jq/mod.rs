//! jq - JSON processor builtin
//!
//! Implements jq functionality using the jaq library.
//!
//! Layout:
//!  - `args`: CLI parsing (incl. `--slurpfile`, `--rawfile`, `--args`,
//!    `--jsonargs`, `--indent`)
//!  - `convert`: serde_json <-> JqJson <-> jaq Val conversion, depth check
//!  - `format`: indent-aware output rendering (custom `--indent N`)
//!  - `compat`: prepended jq-compat definitions and global var names
//!  - `errors`: jq-style error formatting (no Debug-shape leaks)
//!
//! Important decisions are documented at the top of each submodule.
//!
//! Usage:
//!   echo '{"name":"foo"}' | jq '.name'
//!   jq '.[] | .id' < data.json
//!   jq -n --argjson x 5 '$x + 1'
//!   jq -n --slurpfile data /file.json '$data | length'

use async_trait::async_trait;
use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, Vars, data};
use jaq_json::Val;
use jaq_std::input::{HasInputs, Inputs, RcIter};

use super::{Builtin, Context, read_text_file, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

mod args;
mod compat;
mod convert;
mod errors;
mod format;

#[cfg(test)]
mod tests;

use args::{FileVarKind, JqArgs, ParseOutcome};
use compat::{
    ARGS_VAR_NAME, ENV_VAR_NAME, FILENAME_VAR_NAME, LINENO_VAR_NAME, PUBLIC_ENV_VAR_NAME,
    build_compat_prefix,
};
use convert::{JqJson, MAX_JQ_JSON_DEPTH, jq_to_val, parse_json_stream, val_to_jq};
use errors::{format_compile_errors, format_load_errors, format_runtime_error};
use format::{Indent, render, sort_keys as sort_jq_keys};

/// Custom DataT that holds both the LUT and a shared input iterator.
/// Required by jaq 3.0 for `input`/`inputs` filter support.
struct InputData<V>(std::marker::PhantomData<V>);

impl<V: jaq_core::ValT + 'static> data::DataT for InputData<V> {
    type V<'a> = V;
    type Data<'a> = InputDataRef<'a, V>;
}

#[derive(Clone)]
struct InputDataRef<'a, V: jaq_core::ValT + 'static> {
    lut: &'a jaq_core::Lut<InputData<V>>,
    inputs: &'a RcIter<dyn Iterator<Item = std::result::Result<V, String>> + 'a>,
}

impl<'a, V: jaq_core::ValT + 'static> data::HasLut<'a, InputData<V>> for InputDataRef<'a, V> {
    fn lut(&self) -> &'a jaq_core::Lut<InputData<V>> {
        self.lut
    }
}

impl<'a, V: jaq_core::ValT + 'static> HasInputs<'a, V> for InputDataRef<'a, V> {
    fn inputs(&self) -> Inputs<'a, V> {
        self.inputs
    }
}

/// jq command - JSON processor
pub struct Jq;

/// Bookkeeping for one filter run: the input value and the per-input
/// metadata (filename / line number) bound to compat globals.
struct FilterInput {
    value: Val,
    filename: Val,
    lineno: Val,
}

#[async_trait]
impl Builtin for Jq {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let parsed = match args::parse(ctx.args) {
            ParseOutcome::Args(a) => a,
            ParseOutcome::Done(r) => return Ok(r),
        };

        run_jq(ctx, parsed).await
    }
}

async fn run_jq(ctx: Context<'_>, parsed: JqArgs<'_>) -> Result<ExecResult> {
    // Resolve --slurpfile / --rawfile bindings BEFORE parsing the filter,
    // so we can fail fast on missing files.
    let mut all_var_bindings = parsed.var_bindings.clone();
    let mut all_named_args = parsed.named_args.clone();
    for req in &parsed.file_var_requests {
        let path = resolve_path(ctx.cwd, req.path);
        let text = match read_text_file(&*ctx.fs, &path, "jq").await {
            Ok(t) => t,
            Err(e) => return Ok(e),
        };
        let value = match req.kind {
            FileVarKind::Raw => serde_json::Value::String(text),
            FileVarKind::Slurp => match parse_json_stream(&text) {
                Ok(vals) => {
                    // Inner values are already depth-checked by parse_json_stream;
                    // the wrapping array adds one level which the recursive
                    // limit already accommodates.
                    let arr: Vec<serde_json::Value> = vals.iter().map(jq_to_serde_value).collect();
                    serde_json::Value::Array(arr)
                }
                Err(e) => return Ok(ExecResult::err(format!("{e}\n"), 5)),
            },
        };
        all_var_bindings.push((format!("${}", req.name), value.clone()));
        all_named_args.push((req.name.clone(), value));
    }

    // Build $ARGS object. positional: [...], named: {name: val, ...}.
    let args_obj = build_args_obj(&parsed.positional_args, &all_named_args);

    // Read input (stdin or files).
    let file_content: String;
    let input = if !parsed.file_args.is_empty() {
        let mut combined = String::new();
        for file_arg in &parsed.file_args {
            let path = resolve_path(ctx.cwd, file_arg);
            let text = match read_text_file(&*ctx.fs, &path, "jq").await {
                Ok(t) => t,
                Err(e) => return Ok(e),
            };
            if !combined.is_empty() && !combined.ends_with('\n') {
                combined.push('\n');
            }
            combined.push_str(&text);
        }
        file_content = combined;
        file_content.as_str()
    } else {
        ctx.stdin.unwrap_or("")
    };

    // Empty stdin without -n yields empty output (matches real jq for files
    // and stdin alike), but -Rs explicitly produces "" — keep that path.
    if input.trim().is_empty() && !parsed.null_input && !(parsed.raw_input && parsed.slurp) {
        return Ok(ExecResult::ok(String::new()));
    }

    // Build shell env object for the custom `env` filter / $ENV.
    // SECURITY: avoids std::env::set_var() (TM-INF-013).
    // ctx.env takes precedence over ctx.variables (prefix assignments
    // shadow exported variables).
    let env_obj = {
        let mut map = serde_json::Map::new();
        for (k, v) in ctx.variables.iter().chain(ctx.env.iter()) {
            map.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        serde_json::Value::Object(map)
    };

    // Compose the filter: prepend compat defs, the env def, etc.
    let prefix = build_compat_prefix();
    let compat_filter = format!("{prefix}\n{}", parsed.filter);
    let filter_src = compat_filter.as_str();

    // Set up loader.
    let defs = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs());
    let loader = Loader::new(defs);
    let arena = Arena::default();

    let program = File {
        code: filter_src,
        path: (),
    };

    let modules = match loader.load(&arena, program) {
        Ok(m) => m,
        Err(errs) => {
            return Ok(ExecResult::err(format_load_errors(errs), 3));
        }
    };

    // Names of all globals: --arg/--argjson/--slurpfile/--rawfile, then the
    // four internal ones (env, ENV, filename, lineno), then $ARGS.
    let mut var_names: Vec<&str> = all_var_bindings.iter().map(|(n, _)| n.as_str()).collect();
    var_names.push(ENV_VAR_NAME);
    var_names.push(PUBLIC_ENV_VAR_NAME);
    var_names.push(FILENAME_VAR_NAME);
    var_names.push(LINENO_VAR_NAME);
    var_names.push(ARGS_VAR_NAME);

    type D = InputData<Val>;
    let input_funs: Vec<jaq_core::native::Fun<D>> = jaq_std::input::funs::<D>()
        .into_vec()
        .into_iter()
        .map(|(name, arity, run)| (name, arity, jaq_core::Native::<D>::new(run)))
        .collect();
    let native_funs = jaq_core::funs::<D>()
        .chain(jaq_std::funs::<D>().filter(|(name, _, _)| *name != "env"))
        .chain(input_funs)
        .chain(jaq_json::funs::<D>());

    let compiler = Compiler::default()
        .with_funs(native_funs)
        .with_global_vars(var_names.iter().copied());

    let filter = match compiler.compile(modules) {
        Ok(f) => f,
        Err(errs) => {
            return Ok(ExecResult::err(format_compile_errors(errs), 3));
        }
    };

    // Pre-convert globals to Val once.
    let env_val = jq_to_val(&jq_from_serde(&env_obj));
    let args_val = jq_to_val(&jq_from_serde(&args_obj));
    let pre_var_vals: Vec<Val> = all_var_bindings
        .iter()
        .map(|(_, v)| jq_to_val(&jq_from_serde(v)))
        .collect();

    // Build inputs to process.
    let inputs_to_process: Vec<FilterInput> = if parsed.null_input {
        vec![FilterInput {
            value: Val::Null,
            filename: Val::Null,
            lineno: Val::from(0isize),
        }]
    } else if parsed.raw_input && parsed.slurp {
        vec![FilterInput {
            value: Val::from(input.to_string()),
            filename: stdin_filename(&parsed),
            lineno: Val::from(0isize),
        }]
    } else if parsed.raw_input {
        let fname = stdin_filename(&parsed);
        input
            .lines()
            .enumerate()
            .map(|(i, line)| FilterInput {
                value: Val::from(line.to_string()),
                filename: fname.clone(),
                lineno: Val::from(isize::try_from(i + 1).unwrap_or(isize::MAX)),
            })
            .collect()
    } else if parsed.slurp {
        match parse_json_stream(input) {
            Ok(vals) => {
                let arr: JqJson = JqJson::Array(vals);
                vec![FilterInput {
                    value: jq_to_val(&arr),
                    filename: stdin_filename(&parsed),
                    lineno: Val::from(0isize),
                }]
            }
            Err(e) => return Ok(ExecResult::err(format!("{e}\n"), 5)),
        }
    } else {
        match parse_json_stream(input) {
            Ok(jq_vals) => jq_vals
                .iter()
                .enumerate()
                .map(|(i, v)| FilterInput {
                    value: jq_to_val(v),
                    filename: stdin_filename(&parsed),
                    lineno: Val::from(isize::try_from(i + 1).unwrap_or(isize::MAX)),
                })
                .collect(),
            Err(e) => return Ok(ExecResult::err(format!("{e}\n"), 5)),
        }
    };

    let indent = if parsed.compact_output {
        Indent::Compact
    } else {
        parsed.indent
    };

    let mut output = String::new();
    let mut has_output = false;
    let mut all_null_or_false = true;

    // Drive the outer loop from the shared input iterator so `input`/`inputs`
    // inside the filter consume from the same source (matching real jq:
    // `[inputs]` on a 3-value stream returns the *remaining* values, not all).
    // Metadata (filename, lineno) is tracked via a parallel index.
    let metadata: Vec<(Val, Val)> = inputs_to_process
        .iter()
        .map(|fi| (fi.filename.clone(), fi.lineno.clone()))
        .collect();
    let value_iter: Box<dyn Iterator<Item = std::result::Result<Val, String>>> = Box::new(
        inputs_to_process
            .into_iter()
            .map(|fi| Ok::<Val, String>(fi.value)),
    );
    let shared_inputs = RcIter::new(value_iter);

    for (outer_idx, jaq_input) in (&shared_inputs).enumerate() {
        let jaq_input: Val = match jaq_input {
            Ok(v) => v,
            Err(e) => {
                return Ok(ExecResult::err(format!("jq: input error: {e}\n"), 5));
            }
        };
        let (filename_val, lineno_val) = metadata
            .get(outer_idx)
            .cloned()
            .unwrap_or((Val::Null, Val::from(0isize)));

        let mut var_vals: Vec<Val> = pre_var_vals.clone();
        var_vals.push(env_val.clone()); // $__bashkit_env__
        var_vals.push(env_val.clone()); // $ENV
        var_vals.push(filename_val); // $__bashkit_filename__
        var_vals.push(lineno_val); // $__bashkit_lineno__
        var_vals.push(args_val.clone()); // $ARGS

        let data = InputDataRef {
            lut: &filter.lut,
            inputs: &shared_inputs,
        };
        let cv_ctx = Ctx::<InputData<Val>>::new(data, Vars::new(var_vals));

        for result in filter.id.run((cv_ctx, jaq_input)) {
            match jaq_core::unwrap_valr(result) {
                Ok(val) => {
                    has_output = true;
                    let mut jq = val_to_jq(&val);
                    if parsed.sort_keys {
                        jq = sort_jq_keys(jq);
                    }
                    if !(jq.is_null() || jq.is_false()) {
                        all_null_or_false = false;
                    }

                    let effective_raw = parsed.raw_output || parsed.join_output;
                    let formatted = if effective_raw {
                        if let JqJson::String(s) = &jq {
                            s.clone()
                        } else {
                            render(&jq, indent)
                        }
                    } else {
                        render(&jq, indent)
                    };

                    output.push_str(&formatted);
                    if !parsed.join_output {
                        output.push('\n');
                    }
                }
                Err(e) => {
                    return Ok(ExecResult::err(format_runtime_error(&e), 5));
                }
            }
        }
    }

    // Real jq exit codes for -e:
    //   - 4 if there was no output at all
    //   - 1 if all outputs were null or false
    //   - 0 otherwise
    if parsed.exit_status {
        if !has_output {
            return Ok(ExecResult::with_code(output, 4));
        }
        if all_null_or_false {
            return Ok(ExecResult::with_code(output, 1));
        }
    }

    Ok(ExecResult::ok(output))
}

/// `--rawfile`/`--slurpfile`/$ARGS plumbing helper. The serialized object is
/// `{"positional": [...], "named": {...}}`.
fn build_args_obj(
    positional: &[serde_json::Value],
    named: &[(String, serde_json::Value)],
) -> serde_json::Value {
    let mut named_map = serde_json::Map::new();
    for (k, v) in named {
        named_map.insert(k.clone(), v.clone());
    }
    serde_json::json!({
        "positional": positional,
        "named": serde_json::Value::Object(named_map),
    })
}

/// `--slurpfile` / `--rawfile` reuse this to derive the filename Val.
fn stdin_filename(parsed: &JqArgs<'_>) -> Val {
    // Real jq reports the per-file path while reading FILE..., else null.
    // We thread a single filename per call when files are passed; if no
    // files, return null. (Per-file granularity within one call would
    // need re-architecting input dispatch.)
    match parsed.file_args.first() {
        Some(p) => Val::from((*p).to_string()),
        None => Val::Null,
    }
}

/// Convert a serde_json::Value into our internal JqJson with a depth check.
fn jq_from_serde(v: &serde_json::Value) -> JqJson {
    convert::serde_to_jq(v, 0, MAX_JQ_JSON_DEPTH).unwrap_or(JqJson::Null)
}

/// Convert a JqJson back to serde_json::Value (lossy for Number tokens
/// outside i64/f64 range, which is acceptable since this only feeds into
/// our re-serializer for `--slurpfile` arrays — values originate from
/// `parse_json_stream`, so they round-trip cleanly).
fn jq_to_serde_value(v: &JqJson) -> serde_json::Value {
    match v {
        JqJson::Null => serde_json::Value::Null,
        JqJson::Bool(b) => serde_json::Value::Bool(*b),
        JqJson::Number(s) => {
            serde_json::from_str::<serde_json::Value>(s).unwrap_or(serde_json::Value::Null)
        }
        JqJson::String(s) => serde_json::Value::String(s.clone()),
        JqJson::Array(arr) => serde_json::Value::Array(arr.iter().map(jq_to_serde_value).collect()),
        JqJson::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, item) in map {
                out.insert(k.clone(), jq_to_serde_value(item));
            }
            serde_json::Value::Object(out)
        }
    }
}
