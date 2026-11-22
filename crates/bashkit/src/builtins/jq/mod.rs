//! jq - JSON processor builtin
//!
//! Implements jq functionality using the jaq library.
//!
//! Layout:
//!  - `args`: CLI parsing (incl. `--slurpfile`, `--rawfile`, `--args`,
//!    `--jsonargs`, `--indent`)
//!  - `convert`: order-preserving JSON reader -> JqJson <-> jaq Val, depth check
//!  - `stream`: incremental, metered path events with shared ancestor keys
//!  - `format`: indent-aware output rendering (custom `--indent N`)
//!  - `compat`: jq compatibility definitions and global variable names
//!  - `filter`: compile and reuse filters within a request
//!  - `errors`: jq-style error formatting (no Debug-shape leaks)
//!
//! Important decisions are documented at the top of each submodule.
//!
//! Usage:
//!   echo '{"name":"foo"}' | jq '.name'
//!   jq '.[] | .id' < data.json
//!   jq -n --argjson x 5 '$x + 1'
//!   jq -n --slurpfile data /file.json '$data | length'

use self::jaq_json::Val;
use async_trait::async_trait;
use jaq_core::{Ctx, Vars, data};
use jaq_std::input::{HasInputs, Inputs, RcIter};
use std::sync::Arc;

use super::{Builtin, Context, ExecutionDeadline, read_text_file, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::limits::ExecutionLimits;

mod altpat;
mod args;
mod compat;
mod convert;
mod errors;
mod filter;
mod format;
mod input;
mod loc;
mod messages;
// Vendored jaq-json (MIT, Michael Färber, https://github.com/01mf02/jaq),
// see jaq_json/UPSTREAM_VERSION and knowledge/runtimes/jaq-json-vendor.md.
// Kept byte-close to upstream so `scripts/sync-jaq-json.sh` can merge new
// releases: not reformatted, not linted. Its few `unwrap`s rely on
// upstream invariants (e.g. BigInt -> f64 is total); a panic would still be
// contained by the builtin dispatcher's catch_unwind.
#[rustfmt::skip]
#[allow(
    clippy::all,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    dead_code,
    unused_imports,
    unused_macros,
    unreachable_pub,
    missing_docs
)]
mod jaq_json;
mod regex_compat;
mod stream;

#[cfg(test)]
mod tests;

use args::{FileVarKind, JqArgs, MAX_FILE_VAR_BYTES, ParseOutcome};
use convert::{JqJson, jq_to_val, parse_json_stream, val_to_jq_capped};
use errors::format_runtime_error_at;
pub(crate) use filter::JqFilterCache;
use format::{render, sort_keys as sort_jq_keys};

/// Custom DataT that holds both the LUT and a shared input iterator.
/// Required by jaq 3.0 for `input`/`inputs` filter support.
struct InputData<V>(std::marker::PhantomData<V>);

impl<V: jaq_core::ValT + 'static> data::DataT for InputData<V> {
    type V<'a> = V;
    type Data<'a> = InputDataRef<'a, V>;
}

// THREAT[TM-DOS-110]: jaq-core creates/clones this context while evaluating
// user defs, including recursion that never emits or operates on a value.
// Count live contexts to stop stack growth; tick the deadline on every clone
// so tail recursion cannot bypass the wall-clock limit.
const MAX_JQ_LIVE_CONTEXTS: usize = 64;

struct InputDataRef<'a, V: jaq_core::ValT + 'static> {
    lut: &'a jaq_core::Lut<InputData<V>>,
    inputs: &'a RcIter<dyn Iterator<Item = std::result::Result<V, String>> + 'a>,
    live_contexts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl<V: jaq_core::ValT + 'static> Clone for InputDataRef<'_, V> {
    fn clone(&self) -> Self {
        jaq_json::meter::tick();
        let prior = self
            .live_contexts
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if prior >= MAX_JQ_LIVE_CONTEXTS {
            #[cfg(panic = "unwind")]
            {
                self.live_contexts
                    .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                jaq_json::meter::abort_recursion();
            }
            #[cfg(not(panic = "unwind"))]
            jaq_json::meter::trip();
        }
        Self {
            lut: self.lut,
            inputs: self.inputs,
            live_contexts: self.live_contexts.clone(),
        }
    }
}

impl<V: jaq_core::ValT + 'static> Drop for InputDataRef<'_, V> {
    fn drop(&mut self) {
        self.live_contexts
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
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
    let mut file_binding_bytes = 0usize;
    for req in &parsed.file_var_requests {
        let path = resolve_path(ctx.cwd, req.path);
        if let Ok(meta) = ctx.fs.stat(&path).await
            && meta.file_type.is_file()
            && file_binding_exceeds_limit(file_binding_bytes, meta.size)
        {
            return Ok(file_binding_limit_error(file_binding_bytes, meta.size));
        }
        let text = match read_text_file(&*ctx.fs, &path, "jq").await {
            Ok(t) => t,
            Err(e) => return Ok(*e),
        };
        ctx.consume_budget_input(text.len())?;
        match file_binding_bytes.checked_add(text.len()) {
            Some(total) if total <= MAX_FILE_VAR_BYTES => file_binding_bytes = total,
            _ => {
                return Ok(file_binding_limit_error(
                    file_binding_bytes,
                    text.len() as u64,
                ));
            }
        }
        let value = match req.kind {
            FileVarKind::Raw => JqJson::String(text),
            FileVarKind::Slurp => match parse_jq_json_stream(&ctx, &text)? {
                // Inner values are already depth-checked by parse_json_stream;
                // the wrapping array adds one level which the recursive
                // limit already accommodates.
                Ok(vals) => JqJson::Array(vals),
                Err(e) => return Ok(ExecResult::err(format!("{e}\n"), 5)),
            },
        };
        all_var_bindings.push((format!("${}", req.name), value.clone()));
        all_named_args.push((req.name.clone(), value));
    }

    // Build $ARGS object. positional: [...], named: {name: val, ...}.
    let args_obj = build_args_obj(&parsed.positional_args, &all_named_args);

    // Read input sources: stdin, or each FILE in order. A file that cannot
    // be opened is reported like jq (`Could not open file`), skipped, and
    // turns the final exit status into 2.
    let mut stderr_out = String::new();
    let mut input_failed = false;
    let mut sources: Vec<(Option<&str>, std::borrow::Cow<'_, str>)> = Vec::new();
    if parsed.file_args.is_empty() {
        sources.push((None, ctx.stdin.map(|s| s.text_lossy()).unwrap_or_default()));
    } else {
        for file_arg in &parsed.file_args {
            let path = resolve_path(ctx.cwd, file_arg);
            match ctx.fs.read_file(&path).await {
                Ok(bytes) => {
                    let text = String::from_utf8(bytes)
                        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                    ctx.consume_budget_input(text.len())?;
                    sources.push((Some(*file_arg), text.into()));
                }
                Err(e) => {
                    input_failed = true;
                    stderr_out.push_str(&format!(
                        "jq: error: Could not open file {file_arg}: {}\n",
                        crate::error::io_error_reason(&e)
                    ));
                }
            }
        }
    }

    // Build shell env object for the custom `env` filter / $ENV.
    // SECURITY: avoids std::env::set_var() (TM-INF-013).
    // ctx.env takes precedence over ctx.variables (prefix assignments
    // shadow exported variables).
    let env_obj = {
        // Sorted by name: deterministic regardless of HashMap order.
        let mut map = std::collections::BTreeMap::new();
        for (k, v) in ctx.variables.iter().chain(ctx.env.iter()) {
            map.insert(k.clone(), JqJson::String(v.clone()));
        }
        JqJson::Object(map.into_iter().collect())
    };

    let filter_text = match parsed.filter_file {
        Some(file) => {
            let path = resolve_path(ctx.cwd, file);
            match read_text_file(&*ctx.fs, &path, "jq").await {
                Ok(t) => {
                    ctx.consume_budget_input(t.len())?;
                    std::borrow::Cow::Owned(t)
                }
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("jq: Could not open {file}: No such file or directory\n"),
                        2,
                    ));
                }
            }
        }
        None => std::borrow::Cow::Borrowed(parsed.filter),
    };
    let var_names: Vec<&str> = all_var_bindings
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let cache = if parsed.filter_file.is_none() && JqFilterCache::accepts(&filter_text, &var_names)
    {
        ctx.execution_extension::<JqFilterCache>()
    } else {
        None
    };
    let cached = cache.as_ref().and_then(|cache| {
        cache
            .try_with(|cache| cache.get(&filter_text, &var_names))
            .ok()
            .flatten()
    });
    let filter = if let Some(filter) = cached {
        filter
    } else {
        let filter = match filter::compile(&filter_text, &var_names) {
            Ok(filter) => Arc::new(filter),
            Err(stderr) => return Ok(ExecResult::err(stderr, 3)),
        };
        if let Some(cache) = cache {
            let _ = cache.try_with(|cache| cache.insert(&filter_text, &var_names, &filter));
        }
        filter
    };

    // Sources remain buffered by the builtin/VFS contract, but parsing and
    // conversion are driven by input demand, never collected up front.
    if parsed.seq && !parsed.raw_input {
        for (_, text) in &mut sources {
            if text.contains('\x1e') {
                *text = text.replace('\x1e', " ").into();
            }
        }
    }
    let parse_error = std::cell::RefCell::new(None);
    let name_val = |name: &Option<&str>| match name {
        Some(n) => Val::from((*n).to_string()),
        None => Val::Null,
    };

    let indent = parsed.indent;

    let mut output = String::new();

    // THREAT[TM-DOS-093]: jaq evaluation is a synchronous iterator that the
    // async execution timeout cannot preempt. Unbounded generators
    // (`jq -n 'repeat(1)'`, `range(0;1e18)`) would grow `output` without limit
    // (OOM) or spin forever producing no output (hang). Cap accumulated output
    // bytes against the caller's stdout limit and check the wall-clock deadline
    // periodically so a runaway filter aborts instead of wedging the host.
    //
    // THREAT[TM-DOS-110]: Non-emitting filters also poll through value
    // operations, and recursive defs poll through InputDataRef::clone. This
    // closes the gap where neither output nor values change (#2465).
    let max_output_bytes = ctx
        .execution_extension::<ExecutionLimits>()
        .and_then(|limits| limits.try_with(|limits| limits.max_stdout_bytes).ok())
        .unwrap_or_else(|| ExecutionLimits::default().max_stdout_bytes);
    let deadline = ctx.execution_extension::<ExecutionDeadline>();
    let mut values_emitted: usize = 0;

    // THREAT[TM-DOS-110]: values that grow inside one evaluation
    // (`until(false; . + .)`, `"x" * 1e18`, `[range(1e12)]`) never reach the
    // output checks above. The vendored jaq-json charges every string and
    // array/object body to this meter and fails growth past the host's
    // live-bytes limit before allocating (#2444).
    let max_live_bytes = ctx
        .execution_extension::<ExecutionLimits>()
        .and_then(|limits| {
            limits
                .try_with(|limits| limits.max_live_intermediate_bytes)
                .ok()
        })
        .unwrap_or_else(|| ExecutionLimits::default().max_live_intermediate_bytes);
    let stop = deadline.clone().map(|deadline| {
        Box::new(move || {
            deadline
                .try_with(ExecutionDeadline::is_expired)
                .unwrap_or(true)
        }) as jaq_json::meter::StopCheck
    });
    let (_meter_guard, meter) =
        jaq_json::meter::install(usize::try_from(max_live_bytes).unwrap_or(usize::MAX), stop);
    let size_error = |meter: &jaq_json::meter::Meter| {
        Ok(ExecResult::err(
            format!("jq: error: {}\n", meter.message()),
            5,
        ))
    };

    // `input`/`inputs` inside the filter consume from the same stream as
    // the outer loop (`[inputs]` on a 3-value stream returns the remaining
    // values). With `-n` the filter runs once on null and the whole stream
    // is left to `input`/`inputs`. Pulling a value records its file and
    // line for `input_filename` and error locations.
    let current: std::rc::Rc<std::cell::RefCell<(Val, usize)>> =
        std::rc::Rc::new(std::cell::RefCell::new((Val::Null, 0)));
    let null_input = parsed.null_input;
    let location = |current: &(Val, usize)| -> String {
        if null_input {
            return "<unknown>".to_string();
        }
        match &current.0 {
            Val::Null => format!("<stdin>:{}", current.1),
            name => format!("{}:{}", name.to_string().trim_matches('"'), current.1),
        }
    };

    // Exit status follows the last input processed, like jq: 5 after a
    // runtime error, else 0; with `-e`, 1 when the last output was null or
    // false and 4 when nothing was ever output.
    let mut status: Option<i32> = None;
    // `halt`/`halt_error` end the whole command with this code.
    let mut halted: Option<i32> = None;
    messages::take();

    // THREAT[TM-DOS-110]: the meter aborts a run by unwinding where jaq
    // gives it no error channel (see jaq_json::meter::Abort).
    let input_error = std::cell::RefCell::new(None);
    // Set once the filter pulls past the last parsed value. With `-n`, jq
    // only parses stdin as `input`/`inputs` read it, so a parse error past
    // the values the filter actually read is never reported.
    let stream_exhausted = std::cell::Cell::new(false);
    let run_filter = std::panic::AssertUnwindSafe(|| -> Result<Option<ExecResult>> {
        let env_val = jq_to_val(&env_obj);
        let args_val = jq_to_val(&args_obj);
        let pre_var_vals: Vec<Val> = all_var_bindings.iter().map(|(_, v)| jq_to_val(v)).collect();
        let values: Box<dyn Iterator<Item = std::result::Result<Val, input::NormalizeError>> + '_> =
            if parsed.raw_input && parsed.slurp {
                Box::new(std::iter::once_with(|| {
                    let size = sources.iter().map(|(_, t)| t.len()).sum();
                    stream::check(size).map_err(input::NormalizeError::Resource)?;
                    let text: String = sources.iter().map(|(_, t)| t.as_ref()).collect();
                    *current.borrow_mut() = (
                        sources
                            .last()
                            .map(|(n, _)| name_val(n))
                            .unwrap_or(Val::Null),
                        0,
                    );
                    Ok(Val::from(text))
                }))
            } else {
                Box::new(sources.iter().flat_map(|(name, text)| {
                    let filename = name_val(name);
                    let inputs: Box<
                        dyn Iterator<
                                Item = std::result::Result<(Val, usize), input::NormalizeError>,
                            > + '_,
                    > = if parsed.raw_input {
                        Box::new(text.lines().enumerate().map(|(i, line)| {
                            ctx.consume_budget_work(1 + line.len() as u64)?;
                            stream::check(line.len())?;
                            Ok((Val::from(line.to_owned()), i + 1))
                        }))
                    } else if parsed.stream {
                        Box::new(stream::Parser::new(text, &ctx))
                    } else {
                        Box::new(input::values(text, &ctx))
                    };
                    let current = &current;
                    inputs.map(move |value| {
                        value.map(|(value, line)| {
                            *current.borrow_mut() = (filename.clone(), line);
                            value
                        })
                    })
                }))
            };
        let mut value_iter: Box<dyn Iterator<Item = std::result::Result<Val, String>> + '_> =
            Box::new(
                values
                    .map_while(|value| match value {
                        Ok(value) => Some(Ok(value)),
                        Err(input::NormalizeError::InvalidJson(message)) => {
                            *parse_error.borrow_mut() = Some(message);
                            None
                        }
                        Err(input::NormalizeError::Resource(error)) => {
                            let message = error.to_string();
                            *input_error.borrow_mut() = Some(error);
                            Some(Err(message))
                        }
                    })
                    .fuse()
                    .chain(std::iter::from_fn(|| {
                        stream_exhausted.set(true);
                        None
                    })),
            );
        if parsed.slurp && !parsed.raw_input {
            // Slurping itself is demand-driven: `-ns 42` must not parse an
            // ignored malformed tail or allocate an unused input array.
            let mut inputs = value_iter;
            let mut done = false;
            let input_current = &current;
            let pending_error = &parse_error;
            let resource_error = &input_error;
            let input_sources = &sources;
            value_iter = Box::new(std::iter::from_fn(move || {
                if std::mem::replace(&mut done, true) {
                    return None;
                }
                let mut values = jaq_json::meter::Metered::<Vec<Val>>::default();
                for value in &mut inputs {
                    let value = match value {
                        Ok(value) => value,
                        Err(message) => return Some(Err(message)),
                    };
                    if let Err(error) = stream::push_metered(&mut values, value) {
                        let message = error.to_string();
                        *resource_error.borrow_mut() = Some(error);
                        return Some(Err(message));
                    }
                }
                // Slurp runs its filter only after the entire stream parses.
                if pending_error.borrow().is_some() {
                    return None;
                }
                *input_current.borrow_mut() = (
                    input_sources
                        .last()
                        .map(|(n, _)| name_val(n))
                        .unwrap_or(Val::Null),
                    0,
                );
                Some(Ok(Val::Arr(jaq_json::Rc::new(values))))
            }));
        }
        let shared_inputs = RcIter::new(value_iter);

        let mut first = true;
        'inputs: loop {
            let jaq_input: Val = if null_input {
                if !first {
                    break;
                }
                Val::Null
            } else {
                match (&shared_inputs).next() {
                    Some(Ok(v)) => v,
                    Some(Err(e)) => {
                        return Ok(Some(ExecResult::err(format!("jq: input error: {e}\n"), 5)));
                    }
                    None => break,
                }
            };
            first = false;
            let (filename_val, lineno) = current.borrow().clone();

            let mut var_vals: Vec<Val> = pre_var_vals.clone();
            var_vals.push(env_val.clone()); // $__bashkit_env__
            var_vals.push(env_val.clone()); // $ENV
            var_vals.push(filename_val); // $__bashkit_filename__
            var_vals.push(Val::from(isize::try_from(lineno).unwrap_or(isize::MAX))); // $__bashkit_lineno__
            var_vals.push(args_val.clone()); // $ARGS

            let data = InputDataRef {
                lut: &filter.lut,
                inputs: &shared_inputs,
                live_contexts: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(1)),
            };
            let cv_ctx = Ctx::<InputData<Val>>::new(data, Vars::new(var_vals));

            let mut input_status: Option<i32> = if parsed.exit_status { None } else { Some(0) };
            for result in filter.id.run((cv_ctx, jaq_input)) {
                ctx.consume_budget_work(1)?;
                if meter.tripped() {
                    // The value may have been cut short at the limit.
                    return size_error(&meter).map(Some);
                }
                stderr_out.push_str(&messages::take());
                // Never `jaq_core::unwrap_valr`: it exits the process on halt.
                let result = match result {
                    Ok(val) => Ok(val),
                    Err(exn) => match exn.get_err() {
                        Ok(e) => Err(e),
                        Err(exn) => {
                            halted = Some(exn.get_halt().unwrap_or(5));
                            break 'inputs;
                        }
                    },
                };
                match result {
                    Ok(val) => {
                        let Some(mut jq) = val_to_jq_capped(&val, max_output_bytes) else {
                            return Ok(Some(ExecResult::err(
                                format!("jq: output limit exceeded ({max_output_bytes} bytes)\n"),
                                5,
                            )));
                        };
                        if parsed.sort_keys {
                            jq = sort_jq_keys(jq);
                        }
                        if parsed.exit_status {
                            input_status = Some(i32::from(jq.is_null() || jq.is_false()));
                        }

                        // With -a, jq JSON-encodes strings even under -r.
                        let effective_raw =
                            (parsed.raw_output || parsed.join_output || parsed.raw_output0)
                                && !parsed.ascii_output;
                        let formatted = match &jq {
                            JqJson::String(s) if effective_raw => {
                                if parsed.raw_output0 && s.contains('\0') {
                                    let loc = location(&current.borrow());
                                    stderr_out.push_str(&format!(
                                        "jq: error (at {loc}): Cannot dump a string \
                                         containing NUL with --raw-output0 option\n"
                                    ));
                                    input_status = Some(5);
                                    break;
                                }
                                s.clone()
                            }
                            _ => render(&jq, indent),
                        };
                        let formatted = if parsed.ascii_output {
                            ascii_escape(&formatted)
                        } else {
                            formatted
                        };

                        if parsed.seq {
                            output.push('\x1e');
                        }
                        output.push_str(&formatted);
                        if parsed.raw_output0 {
                            output.push('\0');
                        } else if !parsed.join_output {
                            output.push('\n');
                        }

                        if output.len() > max_output_bytes {
                            return Ok(Some(ExecResult::err(
                                format!("jq: output limit exceeded ({max_output_bytes} bytes)\n"),
                                5,
                            )));
                        }
                        values_emitted += 1;
                        if values_emitted.is_multiple_of(4096)
                            && deadline.as_ref().is_some_and(|deadline| {
                                deadline
                                    .try_with(ExecutionDeadline::is_expired)
                                    .unwrap_or(true)
                            })
                        {
                            return Ok(Some(ExecResult::err(
                                "jq: execution timed out\n".to_string(),
                                5,
                            )));
                        }
                    }
                    Err(e) => {
                        // jq reports the error and moves on to the next input.
                        let loc = location(&current.borrow());
                        stderr_out.push_str(&format_runtime_error_at(&e, &loc));
                        input_status = Some(5);
                        break;
                    }
                }
            }
            if input_status.is_some() {
                status = input_status;
            }
        }
        Ok(None)
    });
    let evaluation = std::panic::catch_unwind(run_filter);
    match evaluation {
        Ok(result) => {
            if meter.tripped() {
                return size_error(&meter);
            }
            if let Some(error) = input_error.into_inner() {
                return Err(error);
            }
            if let Some(early) = result? {
                return Ok(early);
            }
        }
        Err(payload) => match payload.downcast_ref::<jaq_json::meter::Abort>() {
            Some(jaq_json::meter::Abort::Memory) => return size_error(&meter),
            Some(jaq_json::meter::Abort::Interrupted) => {
                return Ok(ExecResult::err("jq: execution timed out\n".to_string(), 5));
            }
            Some(jaq_json::meter::Abort::Recursion) => {
                return Ok(ExecResult::err(
                    format!("jq: error: recursion limit ({MAX_JQ_LIVE_CONTEXTS}) exceeded\n"),
                    5,
                ));
            }
            None => std::panic::resume_unwind(payload),
        },
    }

    stderr_out.push_str(&messages::take());
    let mut code = status.unwrap_or(if parsed.exit_status { 4 } else { 0 });
    if let Some(e) = parse_error
        .into_inner()
        .filter(|_| halted.is_none() && (!null_input || stream_exhausted.get()))
    {
        stderr_out.push_str(&e);
        stderr_out.push('\n');
        code = 5;
    }
    if input_failed {
        code = 2;
    }
    if let Some(h) = halted {
        code = h;
    }
    Ok(ExecResult {
        stdout: output.into(),
        stderr: stderr_out.into(),
        exit_code: code,
        ..Default::default()
    })
}

/// Normalize and parse just the value requested by the shared input cursor.
fn parse_jq_json_value(
    ctx: &Context<'_>,
    input: &str,
) -> Result<std::result::Result<JqJson, String>> {
    let execution_budget = ctx
        .execution_budget()
        .map(|budget| {
            budget
                .try_with(Clone::clone)
                .map_err(|_| crate::Error::Cancelled)
        })
        .transpose()?;
    let normalized = match input::normalize(execution_budget.as_ref(), input) {
        Ok(normalized) => normalized,
        Err(input::NormalizeError::InvalidJson(message)) => return Ok(Err(message)),
        Err(input::NormalizeError::Resource(error)) => return Err(error),
    };
    Ok(
        convert::parse_json_value(normalized.as_str()).map_err(|error| match error {
            convert::JsonParseError::TooDeep(message) => message,
            convert::JsonParseError::Invalid(message) => format!("jq: invalid JSON: {message}"),
        }),
    )
}

fn parse_jq_json_stream(
    ctx: &Context<'_>,
    input: &str,
) -> Result<std::result::Result<Vec<JqJson>, String>> {
    let execution_budget = ctx
        .execution_budget()
        .map(|budget| {
            budget
                .try_with(Clone::clone)
                .map_err(|_| crate::Error::Cancelled)
        })
        .transpose()?;
    let normalized = match input::normalize(execution_budget.as_ref(), input) {
        Ok(normalized) => normalized,
        Err(input::NormalizeError::InvalidJson(message)) => return Ok(Err(message)),
        Err(input::NormalizeError::Resource(error)) => return Err(error),
    };
    Ok(parse_json_stream(normalized.as_str()))
}

fn file_binding_exceeds_limit(used: usize, next: u64) -> bool {
    match usize::try_from(next) {
        Ok(next) => used
            .checked_add(next)
            .is_none_or(|total| total > MAX_FILE_VAR_BYTES),
        Err(_) => true,
    }
}

fn file_binding_limit_error(used: usize, next: u64) -> ExecResult {
    ExecResult::err(
        format!(
            "jq: file bindings exceed {} bytes (used {used}, next {next})\n",
            MAX_FILE_VAR_BYTES
        ),
        2,
    )
}

/// `--rawfile`/`--slurpfile`/$ARGS plumbing helper. The object is
/// `{"positional": [...], "named": {...}}`, named in argument order.
fn build_args_obj(positional: &[JqJson], named: &[(String, JqJson)]) -> JqJson {
    let mut named_map: Vec<(String, JqJson)> = Vec::new();
    for (k, v) in named {
        match named_map.iter_mut().find(|(n, _)| n == k) {
            Some(slot) => slot.1 = v.clone(),
            None => named_map.push((k.clone(), v.clone())),
        }
    }
    JqJson::Object(vec![
        ("positional".to_string(), JqJson::Array(positional.to_vec())),
        ("named".to_string(), JqJson::Object(named_map)),
    ])
}

/// `-a`: escape every non-ASCII character as `\uXXXX` (UTF-16 surrogate
/// pairs above the BMP). Rendered JSON has non-ASCII text only inside
/// strings, so escaping the whole rendering is safe.
fn ascii_escape(s: &str) -> String {
    if s.is_ascii() {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}
