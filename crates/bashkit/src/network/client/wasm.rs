//! WASM-specific HTTP transport using the browser `fetch` API.
//!
//! This module is compiled only on `target_family = "wasm"` and provides the
//! `send_request` implementation backed by `web_sys::fetch` and
//! `wasm_bindgen_futures::JsFuture`.
//!
//! # Limitations vs Native
//!
//! - No custom DNS resolver (browser handles resolution). Same-origin policy
//!   and CORS provide additional SSRF defense.
//! - No separate connect timeout (`fetch` does not expose this).
//! - No proxy controls (browser handles proxies).
//! - Response bodies are read via `array_buffer()` rather than streaming.
//!   Size limits are enforced after the full body is received.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortController, Request, RequestInit, RequestMode, Response};

use super::{Method, Response as HttpResponse};
use crate::error::{Error, Result};

/// Wrapper that asserts a future is `Send`.
///
/// # Safety
///
/// On `wasm32-unknown-unknown` there is only one thread, so all types are
/// effectively `Send`. This wrapper is only used within the WASM HTTP client
/// which is compiled exclusively for that target.
struct AssertSend<F>(F);

unsafe impl<F> Send for AssertSend<F> {}
unsafe impl<F> Sync for AssertSend<F> {}

impl<F: Future> Future for AssertSend<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: We are projecting from Pin<&mut AssertSend<F>> to Pin<&mut F>.
        // AssertSend is a newtype wrapper with the same memory layout.
        unsafe { self.map_unchecked_mut(|s| &mut s.0).poll(cx) }
    }
}

/// Format a `JsValue` error for network error reporting.
///
/// Extracts the human-readable message without dumping the full
/// `JsValue` debug representation (which includes a wasm stack trace).
fn js_err_str(e: &wasm_bindgen::JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::Reflect::get(e, &"message".into()).ok()?.as_string())
        .unwrap_or_else(|| "unknown error".to_string())
}

/// Execute an HTTP request via the browser `fetch` API.
pub(crate) fn send_request(
    max_response_bytes: usize,
    method: Method,
    url: &str,
    body: Option<&[u8]>,
    headers: &[(String, String)],
    signing_headers: Vec<(String, String)>,
    timeout: Duration,
) -> impl Future<Output = Result<HttpResponse>> + Send + Sync {
    let url = url.to_string();
    let headers = headers.to_vec();
    AssertSend(async move {
        let abort_controller = AbortController::new().map_err(|e| {
            Error::Internal(format!(
                "failed to create abort controller: {}",
                js_err_str(&e)
            ))
        })?;

        let opts = RequestInit::new();
        opts.set_method(method.as_str());
        opts.set_mode(RequestMode::Cors);
        opts.set_signal(Some(&abort_controller.signal()));

        if let Some(body_data) = body {
            let array = js_sys::Uint8Array::from(body_data);
            opts.set_body(&array);
        }

        let request = Request::new_with_str_and_init(&url, &opts)
            .map_err(|e| Error::Network(format!("failed to build request: {}", js_err_str(&e))))?;

        let req_headers = request.headers();
        for (name, value) in &headers {
            req_headers
                .set(name, value)
                .map_err(|e| Error::Network(format!("failed to set header: {}", js_err_str(&e))))?;
        }
        for (name, value) in &signing_headers {
            req_headers.set(name, value).map_err(|e| {
                Error::Network(format!("failed to set signing header: {}", js_err_str(&e)))
            })?;
        }

        // Set up timeout via abort controller + setTimeout
        let window = web_sys::window()
            .ok_or_else(|| Error::Internal("no window object available".to_string()))?;
        let timeout_ms = timeout.as_millis() as i32;
        let abort_for_timeout = abort_controller.clone();
        let timeout_closure = wasm_bindgen::closure::Closure::once_into_js(move || {
            abort_for_timeout.abort();
        });
        let timeout_id = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                timeout_closure.as_ref().unchecked_ref(),
                timeout_ms,
            )
            .map_err(|e| Error::Internal(format!("failed to set timeout: {}", js_err_str(&e))))?;

        let fetch_promise = window.fetch_with_request(&request);
        let resp_value = match JsFuture::from(fetch_promise).await {
            Ok(v) => v,
            Err(e) => {
                window.clear_timeout_with_handle(timeout_id);
                let msg = js_err_str(&e);
                if msg.contains("AbortError") || msg.contains("abort") {
                    return Err(Error::Network("operation timed out".to_string()));
                }
                return Err(Error::network_sanitized("request failed", &msg));
            }
        };

        window.clear_timeout_with_handle(timeout_id);

        let response: Response = resp_value
            .dyn_into()
            .map_err(|e| Error::Internal(format!("invalid response type: {}", js_err_str(&e))))?;

        let status = response.status();
        let resp_headers = response.headers();
        let mut header_pairs = Vec::new();
        if let Ok(Some(iter)) = js_sys::try_iter(&resp_headers) {
            for entry in iter {
                let entry = entry.map_err(|e| {
                    Error::Internal(format!("header entry error: {}", js_err_str(&e)))
                })?;
                if let Ok(array) = entry.dyn_into::<js_sys::Array>() {
                    if array.length() >= 2 {
                        let name = array.get(0).as_string().unwrap_or_default();
                        let value = array.get(1).as_string().unwrap_or_default();
                        header_pairs.push((name, value));
                    }
                }
            }
        }

        // Read body
        let body = match response.array_buffer() {
            Ok(promise) => {
                let body_value = JsFuture::from(promise).await.map_err(|e| {
                    let msg = js_err_str(&e);
                    Error::network_sanitized("failed to read response body", &msg)
                })?;
                let array_buffer: js_sys::ArrayBuffer = body_value.dyn_into().map_err(|e| {
                    Error::Internal(format!("invalid body type: {}", js_err_str(&e)))
                })?;
                js_sys::Uint8Array::new(&array_buffer).to_vec()
            }
            Err(e) => {
                return Err(Error::Network(format!(
                    "failed to read response body: {}",
                    js_err_str(&e)
                )));
            }
        };

        if body.len() > max_response_bytes {
            return Err(Error::Network(format!(
                "response too large: {} bytes (max: {} bytes)",
                body.len(),
                max_response_bytes
            )));
        }

        Ok(HttpResponse {
            status,
            headers: header_pairs,
            body,
        })
    })
}
