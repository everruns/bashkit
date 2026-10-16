// JS functions/futures never leave the sole wasm thread. Only per-execution
// capabilities hold a dispatcher, so cancelled runs cannot call a later host.
use bashkit::{RuntimeCallContext, runtime_value};
use send_wrapper::SendWrapper;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

pub(super) struct Session(pub SendWrapper<js_sys::Function>);

#[derive(Deserialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
enum RuntimeResponse {
    Ok(Value),
    Error(String),
}

async fn dispatch(request: Value) -> Result<Value, String> {
    let lease = RuntimeCallContext::current()
        .execution_extension::<Session>()
        .ok_or_else(|| "host functions require execute() (async)".to_string())?;
    let promise = lease
        .try_with(|session| {
            session
                .0
                .call1(&JsValue::NULL, &JsValue::from_str(&request.to_string()))
        })
        .map_err(|e| e.to_string())?
        .map_err(|e| super::sanitize_js_error(&e))?;
    let future = SendWrapper::new(JsFuture::from(js_sys::Promise::resolve(&promise)));
    let value = lease
        .run(future)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| super::sanitize_js_error(&e))?;
    let response: RuntimeResponse =
        serde_json::from_str(&value.as_string().ok_or("invalid host response")?)
            .map_err(|_| "invalid host response")?;
    match response {
        RuntimeResponse::Ok(value) => {
            runtime_value::validate(&value).map_err(|e| e.to_string())?;
            Ok(value)
        }
        RuntimeResponse::Error(message) => Err(message),
    }
}

#[cfg(feature = "python")]
pub(super) fn python_handler() -> bashkit::PythonExternalFnHandler {
    Arc::new(|name, args, kwargs| {
        Box::pin(async move {
            let result = async {
            let args = runtime_value::encode_python(&bashkit::MontyObject::Tuple(args)).map_err(|e| e.to_string())?;
            let kwargs = runtime_value::encode_python(&bashkit::MontyObject::dict(kwargs)).map_err(|e| e.to_string())?;
            let value = dispatch(serde_json::json!({"language": "python", "name": name, "args": args, "kwargs": kwargs})).await?;
            runtime_value::decode_python(&value).map_err(|e| e.to_string())
        }.await;
            match result {
                Ok(value) => bashkit::ExtFunctionResult::Return(value),
                Err(message) => bashkit::ExtFunctionResult::Error(bashkit::MontyException::new(
                    bashkit::ExcType::RuntimeError,
                    Some(message),
                )),
            }
        })
    })
}

#[cfg(feature = "typescript")]
pub(super) fn typescript_handler() -> bashkit::TypeScriptExternalFnHandler {
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
