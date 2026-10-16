// Per-execution dispatch keeps Node callback lifetime and request context out
// of persistent snapshots. Core's revocable extension cancels pending awaits;
// the JS session aborts host I/O when native execution returns.
use std::sync::Arc;
use std::sync::atomic::Ordering;

use bashkit::{
    ExcType, ExtFunctionResult, MontyException, PythonExternalFnHandler, RuntimeCallContext,
    TypeScriptExternalFnHandler, runtime_value,
};
use napi::JsValue;
use napi::bindgen_prelude::{Function, Promise, PromiseRaw};
use serde::Deserialize;
use serde_json::Value;

type RuntimeTsfn = napi::threadsafe_function::ThreadsafeFunction<
    String,
    Promise<String>,
    String,
    napi::Status,
    false,
    true,
>;

struct RuntimeSession {
    dispatcher: Arc<RuntimeTsfn>,
}

#[derive(Deserialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
enum RuntimeResponse {
    Ok(Value),
    Error(String),
}

async fn dispatch(request: Value) -> Result<Value, String> {
    let lease = RuntimeCallContext::current().execution_extension::<Arc<RuntimeSession>>()
        .ok_or_else(|| "host functions require execute() (async); executeSync() cannot service the JavaScript event loop".to_string())?;
    let session = lease.try_with(Clone::clone).map_err(|e| e.to_string())?;
    lease
        .run(async {
            let _permit = super::callback_semaphore()
                .acquire()
                .await
                .map_err(|_| "host dispatcher is shutting down")?;
            let promise = session
                .dispatcher
                .call_async(request.to_string())
                .await
                .map_err(|_| "host dispatcher failed")?;
            let response = promise.await.map_err(|_| "host dispatcher failed")?;
            match serde_json::from_str::<RuntimeResponse>(&response)
                .map_err(|_| "invalid host dispatcher response")?
            {
                RuntimeResponse::Ok(value) => {
                    runtime_value::validate(&value).map_err(|e| e.to_string())?;
                    Ok(value)
                }
                RuntimeResponse::Error(message) => Err(message),
            }
        })
        .await
        .map_err(|e| e.to_string())?
}

pub(super) fn python_handler() -> PythonExternalFnHandler {
    Arc::new(|name, args, kwargs| {
        Box::pin(async move {
            let result = async {
                let args = runtime_value::encode_python(&bashkit::MontyObject::Tuple(args))
                    .map_err(|e| e.to_string())?;
                let kwargs = runtime_value::encode_python(&bashkit::MontyObject::dict(kwargs))
                    .map_err(|e| e.to_string())?;
                let value = dispatch(serde_json::json!({
                    "language": "python", "name": name, "args": args, "kwargs": kwargs
                }))
                .await?;
                runtime_value::decode_python(&value).map_err(|e| e.to_string())
            }
            .await;
            match result {
                Ok(value) => ExtFunctionResult::Return(value),
                Err(message) => ExtFunctionResult::Error(MontyException::new(
                    ExcType::RuntimeError,
                    Some(message),
                )),
            }
        })
    })
}

pub(super) fn typescript_handler() -> TypeScriptExternalFnHandler {
    Arc::new(|name, args| {
        Box::pin(async move {
            let args = runtime_value::encode_typescript(&bashkit::ZapcodeValue::Array(args))
                .map_err(|e| e.to_string())?;
            let value =
                dispatch(serde_json::json!({"language":"typescript", "name":name, "args":args}))
                    .await?;
            runtime_value::decode_typescript(&value).map_err(|e| e.to_string())
        })
    })
}

pub(super) fn execute<'env>(
    state: Arc<super::SharedState>,
    commands: String,
    dispatcher: Function<'env, String, Promise<String>>,
    on_output: Option<Function<'env, (String, String), Option<String>>>,
) -> napi::Result<PromiseRaw<'env, super::ExecResult>> {
    super::reject_on_output_reentry(&state)?;
    let env = dispatcher.value().env;
    let dispatcher = Arc::new(
        dispatcher
            .build_threadsafe_function::<String>()
            .weak::<true>()
            .build()?,
    );
    let output = on_output.map(super::create_output_tsfn).transpose()?;
    let session = Arc::new(RuntimeSession { dispatcher });
    let promise = napi::bindgen_prelude::execute_tokio_future(
        env,
        async move {
            if let Some(result) = super::validate_async_execute_input(&state, &commands) {
                return Ok(result);
            }
            let _slot = super::acquire_async_execute_slot(&state).await?;
            let mut bash = state.inner.lock().await;
            let extensions = bashkit::ExecutionExtensions::new().with(session);
            let cancelled = bash.cancellation_token();
            let requested = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let output = output.map(|tsfn| {
                super::build_async_output_callback(
                    tsfn,
                    cancelled.clone(),
                    requested.clone(),
                    state.on_output_reentry_depth.clone(),
                )
            });
            let options = bashkit::ExecOptions::new().extensions(extensions);
            let options = match output {
                Some((output, callback_error)) => {
                    // Keep the exact error slot used by the callback.
                    let result = bash
                        .exec_with_options(&commands, options.streaming(output))
                        .await;
                    if let Some(err) = super::take_callback_error(&callback_error) {
                        if requested.load(Ordering::SeqCst) {
                            cancelled.store(false, Ordering::SeqCst);
                        }
                        return Err(err);
                    }
                    return Ok(super::js_exec_result_from_bash_result(result));
                }
                None => options,
            };
            Ok(super::js_exec_result_from_bash_result(
                bash.exec_with_options(&commands, options).await,
            ))
        },
        // SAFETY: NAPI invokes this conversion on the originating JS Env.
        |env, result| unsafe {
            <super::ExecResult as napi::bindgen_prelude::ToNapiValue>::to_napi_value(env, result)
        },
    )?;
    Ok(PromiseRaw::new(env, promise))
}
