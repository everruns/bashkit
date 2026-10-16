//! Single-threaded live JS commands and awaited byte sinks.

use super::*;
use bashkit::{Command, CommandContext, CommandStatus};

#[wasm_bindgen(js_name = CommandInput)]
#[derive(Clone)]
pub struct Input {
    inner: bashkit::CommandInput,
}

#[wasm_bindgen(js_class = CommandInput)]
impl Input {
    #[wasm_bindgen(js_name = readChunk)]
    pub fn read_chunk(&self) -> js_sys::Promise {
        let input = self.inner.clone();
        future_to_promise(async move {
            let bytes = input.read_chunk().await.map_err(js_error)?;
            Ok(bytes.map_or(JsValue::NULL, |b| {
                js_sys::Uint8Array::from(b.as_slice()).into()
            }))
        })
    }
    #[wasm_bindgen(js_name = "[Symbol.asyncIterator]")]
    pub fn iterator(&self) -> Input {
        self.clone()
    }
    pub fn next(&self) -> js_sys::Promise {
        let input = self.inner.clone();
        future_to_promise(async move {
            let bytes = input.read_chunk().await.map_err(js_error)?;
            let result = js_sys::Object::new();
            js_sys::Reflect::set(
                &result,
                &"done".into(),
                &JsValue::from_bool(bytes.is_none()),
            )?;
            if let Some(bytes) = bytes {
                js_sys::Reflect::set(
                    &result,
                    &"value".into(),
                    &js_sys::Uint8Array::from(bytes.as_slice()),
                )?;
            }
            Ok(result.into())
        })
    }
}

#[wasm_bindgen(js_name = CommandOutput)]
pub struct Output {
    inner: bashkit::CommandOutput,
}

#[wasm_bindgen(js_class = CommandOutput)]
impl Output {
    pub fn write(&self, bytes: js_sys::Uint8Array) -> js_sys::Promise {
        let output = self.inner.clone();
        let bytes = bytes.to_vec();
        future_to_promise(async move {
            output.write_all(&bytes).await.map_err(js_error)?;
            Ok(JsValue::UNDEFINED)
        })
    }
}

fn js_error(error: impl ToString) -> JsValue {
    JsError::new(&error.to_string()).into()
}

pub(super) struct JsCommand {
    pub name: String,
    pub callback: SendWrapper<js_sys::Function>,
    pub in_sync: Arc<AtomicBool>,
}

#[async_trait]
impl Command for JsCommand {
    async fn execute(&self, ctx: CommandContext<'_>) -> bashkit::Result<CommandStatus> {
        if self.in_sync.load(Ordering::SeqCst) {
            return Err(bashkit::Error::Execution(
                "streaming commands require async execute()".into(),
            ));
        }
        // All JS objects are consumed before the Send-wrapped Promise await.
        let promise = {
            let request = BuiltinRequest {
                name: &self.name,
                argv: ctx.args,
                stdin: None,
                env: ctx.env,
                cwd: ctx.cwd.to_string_lossy().into_owned(),
            };
            let serializer = serde_wasm_bindgen::Serializer::json_compatible();
            let arg = request
                .serialize(&serializer)
                .map_err(|e| bashkit::Error::Execution(e.to_string()))?;
            for (key, value) in [
                (
                    "stdin",
                    JsValue::from(Input {
                        inner: ctx.stdin.clone(),
                    }),
                ),
                (
                    "stdout",
                    JsValue::from(Output {
                        inner: ctx.stdout.clone(),
                    }),
                ),
                (
                    "stderr",
                    JsValue::from(Output {
                        inner: ctx.stderr.clone(),
                    }),
                ),
                ("fs", JsValue::from(FileSystem::new(ctx.fs.clone()))),
            ] {
                js_sys::Reflect::set(&arg, &key.into(), &value).map_err(|_| {
                    bashkit::Error::Execution("failed to create command context".into())
                })?;
            }
            let value = self
                .callback
                .call1(&JsValue::UNDEFINED, &arg)
                .map_err(|e| bashkit::Error::Execution(sanitize_js_error(&e)))?;
            SendWrapper::new(JsFuture::from(js_sys::Promise::resolve(&value)))
        };
        let result = promise.await;
        match result {
            Ok(value) => {
                let code = value
                    .as_f64()
                    .or_else(|| {
                        js_sys::Reflect::get(&value, &"exitCode".into())
                            .ok()
                            .and_then(|v| v.as_f64())
                    })
                    .filter(|n| n.fract() == 0.0 && *n >= i32::MIN as f64 && *n <= i32::MAX as f64)
                    .ok_or_else(|| {
                        bashkit::Error::Execution(
                            "command must return an integer exit status".into(),
                        )
                    })?;
                Ok(CommandStatus::with_code(code as i32))
            }
            Err(error) => {
                let message = sanitize_js_error(&error);
                drop(error);
                ctx.stderr.write_all(message.as_bytes()).await?;
                Ok(CommandStatus::with_code(1))
            }
        }
    }
}

fn sink(options: &JsValue, key: &str) -> Result<Option<SendWrapper<js_sys::Function>>, JsError> {
    if options.is_null() || options.is_undefined() {
        return Ok(None);
    }
    let value = js_sys::Reflect::get(options, &key.into())
        .map_err(|_| JsError::new("invalid execution options"))?;
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    value
        .dyn_into::<js_sys::Function>()
        .map(SendWrapper::new)
        .map(Some)
        .map_err(|_| JsError::new("output sinks must be callable"))
}

pub(super) async fn execute(
    bash: &mut CoreBash,
    script: &str,
    options: JsValue,
) -> Result<CoreExecResult, JsValue> {
    let stdout = sink(&options, "stdout")?;
    let stderr = sink(&options, "stderr")?;
    let capture = if options.is_null() || options.is_undefined() {
        JsValue::UNDEFINED
    } else {
        js_sys::Reflect::get(&options, &"capture".into())?
    };
    let has_signal = !options.is_null()
        && !options.is_undefined()
        && js_sys::Reflect::get(&options, &"signal".into())
            .is_ok_and(|s| !s.is_null() && !s.is_undefined());
    let explicit_output = stdout.is_some() || stderr.is_some() || !capture.is_undefined();
    let live = explicit_output || has_signal;
    if !live {
        return bash.exec(script).await.map_err(js_error);
    }
    let mut execution = bash.execute(script);
    if has_signal && !explicit_output {
        // Signal-only execution keeps the buffered convenience result while
        // using the driver that interrupts pending host Promises.
        execution = execution.capture(usize::MAX);
    }
    if !capture.is_undefined() {
        let limit = capture
            .as_f64()
            .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n <= u32::MAX as f64)
            .ok_or_else(|| JsError::new("capture must be a non-negative 32-bit byte limit"))?;
        execution = execution.capture(limit as usize);
    }
    if let Some(callback) = stdout {
        execution = execution.stdout_fn(move |bytes| deliver(callback.clone(), bytes));
    }
    if let Some(callback) = stderr {
        execution = execution.stderr_fn(move |bytes| deliver(callback.clone(), bytes));
    }
    execution.await.map_err(js_error)
}

async fn deliver(callback: SendWrapper<js_sys::Function>, bytes: Vec<u8>) -> bashkit::Result<()> {
    let promise = {
        let chunk = js_sys::Uint8Array::from(bytes.as_slice());
        let value = callback
            .call1(&JsValue::UNDEFINED, &chunk)
            .map_err(|e| bashkit::Error::Execution(sanitize_js_error(&e)))?;
        SendWrapper::new(JsFuture::from(js_sys::Promise::resolve(&value)))
    };
    promise
        .await
        .map(|_| ())
        .map_err(|e| bashkit::Error::Execution(sanitize_js_error(&e)))
}
