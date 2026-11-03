// Runtime calls are library-owned and may be answered from any thread. The
// callback itself runs on the execute caller's thread and must return promptly;
// no Rust future, allocator ownership or thread-affine runtime crosses the ABI.
// THREAT[TM-INT-012]: execution leases revoke late replies; owned calls and
// bounded, validated JSON avoid borrowed storage and stale caller context.
use super::*;
use bashkit::{ExecutionBudget, ExecutionCapability, RuntimeCallContext, runtime_value};
use serde_json::Value;
use std::cell::Cell;
use std::ffi::c_void;
use tokio::sync::oneshot;

pub const BASHKIT_MAX_RUNTIME_VALUE_BYTES: usize = 10_000_000;
pub type BashkitRuntimeHandlerV1 = unsafe extern "C" fn(*mut c_void, *mut BashkitRuntimeCall);

#[derive(Clone, Copy)]
pub(super) struct Registration {
    handler: BashkitRuntimeHandlerV1,
    user_data: *mut c_void,
}
// SAFETY: the C contract requires handler/user_data to remain valid until all
// executions finish and permits execution from any host thread, serially per handle.
unsafe impl Send for Registration {}
unsafe impl Sync for Registration {}

type Reply = Result<Value, String>;
type ReplyReceiver = oneshot::Receiver<Reply>;

pub struct BashkitRuntimeCall {
    request: Vec<u8>,
    reply: Mutex<Option<oneshot::Sender<Reply>>>,
    lease: ExecutionCapability<()>,
    budget: Option<ExecutionCapability<ExecutionBudget>>,
}

thread_local! { static IN_CALLBACK: Cell<bool> = const { Cell::new(false) }; }
pub(super) fn in_callback() -> bool {
    IN_CALLBACK.get()
}
pub(super) fn enter_callback() -> impl Drop {
    CallbackScope(IN_CALLBACK.replace(true))
}

struct CallbackScope(bool);
impl Drop for CallbackScope {
    fn drop(&mut self) {
        IN_CALLBACK.set(self.0);
    }
}

/// Construct with optional Python/TypeScript names and one host dispatcher.
///
/// # Safety
/// Same pointer rules as `bashkit_create_json`. The callback and user_data must
/// remain valid until the Bashkit handle and all executions are finished. The
/// callback must not unwind, longjmp or block indefinitely. It receives ownership
/// of each call; answer/free it immediately or transfer it to another thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_create_with_runtime_handler_v1(
    config_json: BashkitBytes,
    handler: Option<BashkitRuntimeHandlerV1>,
    user_data: *mut c_void,
    out_bash: *mut *mut Bashkit,
    out_error: *mut *mut BashkitError,
) -> BashkitStatus {
    unsafe {
        let Some(handler) = handler else {
            return ffi_boundary(out_error, || {
                let _ = output_slot(out_bash, "out_bash")?;
                Err(ApiFailure::invalid_argument("handler must not be NULL"))
            });
        };
        create_json(
            config_json,
            Some(Registration { handler, user_data }),
            out_bash,
            out_error,
        )
    }
}

/// Borrow the JSON request: language, name, args, kwargs (Python only).
/// # Safety
/// `call` must be null or live. The view lasts until call_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_runtime_call_request(
    call: *const BashkitRuntimeCall,
) -> BashkitBytes {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        call.as_ref()
            .map_or(BashkitBytes::EMPTY, |c| BashkitBytes::borrowed(&c.request))
    }))
    .unwrap_or(BashkitBytes::EMPTY)
}

/// Poll revocation, cancellation and the execution deadline from any thread.
/// # Safety
/// `call` must be null or live and cannot race call_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_runtime_call_cancelled(call: *const BashkitRuntimeCall) -> u32 {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        let Some(call) = call.as_ref() else {
            return 1;
        };
        u32::from(
            call.lease.try_with(|_| ()).is_err()
                || call
                    .budget
                    .as_ref()
                    .is_some_and(|b| !matches!(b.try_with(ExecutionBudget::check), Ok(Ok(())))),
        )
    }))
    .unwrap_or(1)
}

unsafe fn reply(call: *mut BashkitRuntimeCall, value: Reply) -> Result<(), ApiFailure> {
    let call = unsafe { call.as_ref() }
        .ok_or_else(|| ApiFailure::invalid_argument("call must not be NULL"))?;
    if unsafe { bashkit_runtime_call_cancelled(call) } != 0 {
        return Err(ApiFailure::new(
            BashkitStatus::Cancelled,
            "runtime call is no longer active",
        ));
    }
    let sender = call
        .reply
        .lock()
        .map_err(|_| ApiFailure::new(BashkitStatus::InternalError, "runtime reply unavailable"))?
        .take()
        .ok_or_else(|| ApiFailure::invalid_argument("runtime call already answered"))?;
    sender
        .send(value)
        .map_err(|_| ApiFailure::new(BashkitStatus::Cancelled, "runtime call is no longer active"))
}

/// Copy a JSON result into the library. Returns CANCELLED for a late reply.
/// Invalid JSON leaves the call unanswered, permitting a corrected response.
/// # Safety
/// `call` must be live; value bytes readable for this call. Cannot race call_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_runtime_call_respond_json(
    call: *mut BashkitRuntimeCall,
    value: BashkitBytes,
) -> BashkitStatus {
    unsafe {
        ffi_boundary(ptr::null_mut(), || {
            if value.len > BASHKIT_MAX_RUNTIME_VALUE_BYTES {
                return Err(ApiFailure::invalid_argument(
                    "runtime value exceeds 10000000 bytes",
                ));
            }
            let value: Value = serde_json::from_slice(input_bytes(value, "value")?)
                .map_err(|_| ApiFailure::invalid_argument("runtime response must be JSON"))?;
            runtime_value::validate(&value)
                .map_err(|e| ApiFailure::invalid_argument(e.to_string()))?;
            reply(call, Ok(value))
        })
    }
}

/// Report a generic script-visible failure; host diagnostics remain in the host.
/// # Safety
/// `call` must be live and cannot race call_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_runtime_call_fail(call: *mut BashkitRuntimeCall) -> BashkitStatus {
    unsafe {
        ffi_boundary(ptr::null_mut(), || {
            reply(call, Err("host function failed".into()))
        })
    }
}

/// Release the owned call. An unanswered call fails the script promptly.
/// # Safety
/// `call` must be null or library-owned, released once, with no concurrent use.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bashkit_runtime_call_free(call: *mut BashkitRuntimeCall) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !call.is_null() {
            unsafe {
                drop(Box::from_raw(call));
            }
        }
    }));
}

fn invoke(
    registration: Registration,
    request: Value,
) -> Result<(ExecutionCapability<()>, ReplyReceiver), String> {
    let context = RuntimeCallContext::current();
    let lease = context
        .execution_capability(())
        .ok_or("no active execution")?;
    let (sender, receiver) = oneshot::channel();
    let request = request.to_string().into_bytes();
    if request.len() > BASHKIT_MAX_RUNTIME_VALUE_BYTES {
        return Err("runtime request exceeds 10000000 bytes".into());
    }
    let call = Box::new(BashkitRuntimeCall {
        request,
        reply: Mutex::new(Some(sender)),
        lease: lease.clone(),
        budget: context.execution_budget(),
    });
    let _scope = enter_callback();
    // SAFETY: registration is host-provided and obeys the lifetime/no-unwind
    // contract. Ownership of this call transfers to that callback exactly once.
    unsafe {
        (registration.handler)(registration.user_data, Box::into_raw(call));
    }
    Ok((lease, receiver))
}

async fn dispatch(registration: Registration, request: Value) -> Result<Value, String> {
    let (lease, receiver) = invoke(registration, request)?;
    lease
        .run(receiver)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|_| "host call released without a response".to_string())?
}

pub(super) fn python_handler(registration: Registration) -> bashkit::PythonExternalFnHandler {
    Arc::new(move |name, args, kwargs| {
        Box::pin(async move {
            let result = async {
            let args = runtime_value::encode_python(&bashkit::MontyObject::Tuple(args)).map_err(|e| e.to_string())?;
            let kwargs = runtime_value::encode_python(&bashkit::MontyObject::dict(kwargs)).map_err(|e| e.to_string())?;
            let value = dispatch(registration, serde_json::json!({"language":"python", "name":name, "args":args, "kwargs":kwargs})).await?;
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

pub(super) fn typescript_handler(
    registration: Registration,
) -> bashkit::TypeScriptExternalFnHandler {
    Arc::new(move |name, args| {
        Box::pin(async move {
            let args = runtime_value::encode_typescript(&bashkit::ZapcodeValue::Array(args))
                .map_err(|e| e.to_string())?;
            let value = dispatch(
                registration,
                serde_json::json!({"language":"typescript", "name":name, "args":args}),
            )
            .await?;
            runtime_value::decode_typescript(&value).map_err(|e| e.to_string())
        })
    })
}
