//! Byte streaming across the NAPI boundary. Only one bounded chunk per sink
//! is dispatched at a time; awaiting a JS Promise backpressures the engine.

use super::*;
use bashkit::{Command, CommandContext, CommandInput, CommandOutput, CommandStatus};
use napi::bindgen_prelude::{Function, Promise, PromiseRaw, Uint8Array};

type CommandArgs = (
    String,
    External<NativeFileSystemState>,
    External<CommandInput>,
    External<CommandOutput>,
    External<CommandOutput>,
);
pub type Callback<'env> = Function<'env, CommandArgs, Promise<i32>>;
type CommandTsfn = napi::threadsafe_function::ThreadsafeFunction<
    CommandArgs,
    Promise<i32>,
    CommandArgs,
    napi::Status,
    false,
    true,
>;
type SinkTsfn = napi::threadsafe_function::ThreadsafeFunction<
    (Uint8Array,),
    Promise<()>,
    (Uint8Array,),
    napi::Status,
    false,
    true,
>;

struct JsCommand {
    name: String,
    callback: Arc<CommandTsfn>,
    in_sync: Arc<AtomicUsize>,
}

#[async_trait]
impl Command for JsCommand {
    async fn execute(&self, ctx: CommandContext<'_>) -> bashkit::Result<CommandStatus> {
        if self.in_sync.load(Ordering::SeqCst) > 0 {
            return Err(bashkit::Error::Execution(
                SYNC_BUILTIN_DEADLOCK_ERROR.into(),
            ));
        }
        let payload = serde_json::to_string(&serde_json::json!({
            "name": self.name, "argv": ctx.args, "env": ctx.env, "cwd": ctx.cwd,
        }))
        .map_err(|e| bashkit::Error::Execution(e.to_string()))?;
        let args = (
            payload,
            External::new(NativeFileSystemState::from_static(ctx.fs.clone())),
            External::new(ctx.stdin.clone()),
            External::new(ctx.stdout.clone()),
            External::new(ctx.stderr.clone()),
        );
        let result = async {
            let promise = self.callback.call_async(args).await?;
            promise.await
        }
        .await;
        match result {
            Ok(code) => Ok(CommandStatus::with_code(code)),
            Err(error) => {
                ctx.stderr.write_all(error.to_string().as_bytes()).await?;
                Ok(CommandStatus::with_code(1))
            }
        }
    }
}

pub(super) fn register(
    state: &SharedState,
    name: String,
    callback: Callback<'_>,
) -> napi::Result<()> {
    let callback = callback
        .build_threadsafe_function::<CommandArgs>()
        .weak::<true>()
        .build()?;
    state.host_registry.insert_command(
        name.clone(),
        JsCommand {
            name,
            callback: Arc::new(callback),
            in_sync: state.in_sync_execute_depth.clone(),
        },
    );
    Ok(())
}

fn sink(
    callback: Option<Function<'_, (Uint8Array,), Promise<()>>>,
) -> napi::Result<Option<Arc<SinkTsfn>>> {
    callback
        .map(|f| {
            f.build_threadsafe_function::<(Uint8Array,)>()
                .weak::<true>()
                .build()
                .map(Arc::new)
        })
        .transpose()
}

pub(super) fn execute<'env>(
    env: &'env Env,
    state: Arc<SharedState>,
    commands: String,
    capture: Option<u32>,
    stdout: Option<Function<'env, (Uint8Array,), Promise<()>>>,
    stderr: Option<Function<'env, (Uint8Array,), Promise<()>>>,
    dispatcher: Option<Function<'env, String, Promise<String>>>,
) -> napi::Result<PromiseRaw<'env, ExecResult>> {
    reject_on_output_reentry(&state)?;
    let extensions = dispatcher
        .map(runtime_functions::extensions)
        .transpose()?
        .unwrap_or_default();
    let stdout = sink(stdout)?;
    let stderr = sink(stderr)?;
    env.spawn_future(async move {
        if let Some(result) = validate_async_execute_input(&state, &commands) {
            return Ok(result);
        }
        let _slot = acquire_async_execute_slot(&state).await?;
        let mut bash = state.inner.lock().await;
        let mut execution = bash
            .execute(&commands)
            .options(bashkit::ExecOptions::new().extensions(extensions));
        if let Some(limit) = capture {
            execution = execution.capture(limit as usize);
        }
        if let Some(callback) = stdout {
            let depth = state.on_output_reentry_depth.clone();
            execution =
                execution.stdout_fn(move |bytes| deliver(callback.clone(), depth.clone(), bytes));
        }
        if let Some(callback) = stderr {
            let depth = state.on_output_reentry_depth.clone();
            execution =
                execution.stderr_fn(move |bytes| deliver(callback.clone(), depth.clone(), bytes));
        }
        Ok(js_exec_result_from_bash_result(execution.await))
    })
}

async fn deliver(
    callback: Arc<SinkTsfn>,
    depth: Arc<AtomicUsize>,
    bytes: Vec<u8>,
) -> bashkit::Result<()> {
    let _reentry = OnOutputReentryScope::enter(depth);
    let promise = callback
        .call_async((Uint8Array::new(bytes),))
        .await
        .map_err(|e| bashkit::Error::Execution(e.to_string()))?;
    promise
        .await
        .map_err(|e| bashkit::Error::Execution(e.to_string()))
}
