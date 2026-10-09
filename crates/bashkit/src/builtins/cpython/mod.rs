//! `python`/`python3` builtins backed by real CPython 3.14 running as a
//! WebAssembly guest (`cpython` feature).
//!
//! Decisions (see `knowledge/runtimes/cpython-wasm.md`):
//! - Isolation comes from the wasm boundary, not from CPython. The guest can
//!   only reach what [`wasi`] hands it: the VFS, captured stdio, a host RNG and
//!   clocks, plus HTTP through bashkit's `HttpClient` ([`http`]). No host
//!   filesystem, socket, process or thread API exists.
//! - No runtime opt-in env var (unlike Monty's `BASHKIT_ALLOW_INPROCESS_PYTHON`):
//!   the guest never runs native code in the host, so calling
//!   `BashBuilder::cpython()` is the opt-in.
//! - Engine, pooling, fuel-metered deadlines and budget checks are shared with
//!   other wasm guests ([`super::wasi_host`]). Every call gets a fresh
//!   instance mapped copy-on-write from the pre-initialized snapshot; nothing
//!   survives between calls or tenants.
//! - Memory is capped by a store limiter; `memory.grow` past the cap fails and
//!   surfaces as `MemoryError` inside Python, not as a host error.

mod http;

pub(crate) use http::HttpState;
pub(crate) use http::check_request_invariants as check_http_request_invariants;

use std::sync::OnceLock;
use std::time::Duration;

use async_trait::async_trait;

use super::runtime_limits::{DEFAULT_MAX_DURATION, RuntimeLimits};
use super::{Builtin, Context, ExecutionDeadline};
use crate::error::Result;
use crate::interpreter::ExecResult;

use super::wasi_host::{self, Call, Entry, GuestConfig, GuestRuntime, GuestState, PoolSpec};

/// Default memory cap for one call. The snapshot itself starts at ~40 MB of
/// linear memory (mostly untouched, mapped copy-on-write).
const DEFAULT_MAX_MEMORY: usize = 256 * 1024 * 1024;
/// CPython's own default recursion limit.
const DEFAULT_MAX_RECURSION: usize = 1000;
/// Default cap on captured stdout + stderr.
const DEFAULT_MAX_OUTPUT: usize = 16 * 1024 * 1024;
/// Default cap on HTTP requests per call.
const DEFAULT_MAX_HTTP_REQUESTS: usize = 100;
/// Guest instructions between cooperative yields (~1 ms on Pulley).
const YIELD_INTERVAL_FUEL: u64 = 100_000;
/// Request-budget work units charged per call before the guest runs.
const CALL_WORK_UNITS: u64 = 4096;
/// Guest instructions per request-budget work unit (a trivial call executes
/// ~0.5M instructions, so ~500 units on top of [`CALL_WORK_UNITS`]).
const FUEL_PER_WORK_UNIT: u64 = 1024;
/// Wasm call-stack budget. CPython recurses in C for nested containers,
/// `repr`, the compiler and the parser; 512 KiB (wasmtime's default) overflows
/// before Python's own recursion limit triggers.
const MAX_WASM_STACK: usize = 4 * 1024 * 1024;

/// Resource limits for the CPython builtin.
///
/// Defaults: 30 s per call, 256 MB guest memory, recursion limit 1000, 16 MB
/// of captured output. A tighter request deadline always wins over
/// `max_duration`.
#[derive(Debug, Clone)]
pub struct CPythonLimits {
    /// Shared VM limits: wall clock, guest memory, Python recursion limit.
    pub common: RuntimeLimits,
    /// Maximum captured stdout + stderr bytes; output past this is dropped
    /// and the result is marked truncated.
    pub max_output: usize,
    /// Maximum HTTP requests one call may make (each redirect hop counts).
    pub max_http_requests: usize,
}

impl Default for CPythonLimits {
    fn default() -> Self {
        Self {
            common: RuntimeLimits {
                max_duration: DEFAULT_MAX_DURATION,
                max_memory: DEFAULT_MAX_MEMORY,
                max_call_depth: DEFAULT_MAX_RECURSION,
            },
            max_output: DEFAULT_MAX_OUTPUT,
            max_http_requests: DEFAULT_MAX_HTTP_REQUESTS,
        }
    }
}

impl CPythonLimits {
    /// Set max wall-clock duration per call.
    #[must_use]
    pub fn max_duration(mut self, d: Duration) -> Self {
        self.common.max_duration = d;
        self
    }

    /// Set max guest memory in bytes (at most 1 GiB is honored). Values below
    /// the snapshot's initial memory (~40 MB) make every call fail to start.
    #[must_use]
    pub fn max_memory(mut self, bytes: usize) -> Self {
        self.common.max_memory = bytes;
        self
    }

    /// Set Python's recursion limit (`sys.setrecursionlimit`).
    #[must_use]
    pub fn max_recursion(mut self, depth: usize) -> Self {
        self.common.max_call_depth = depth;
        self
    }

    /// Set the captured output cap in bytes.
    #[must_use]
    pub fn max_output(mut self, bytes: usize) -> Self {
        self.max_output = bytes;
        self
    }

    /// Set the maximum number of HTTP requests per call.
    #[must_use]
    pub fn max_http_requests(mut self, n: usize) -> Self {
        self.max_http_requests = n;
        self
    }
}

/// Instance pool: 512 concurrent calls per process; slots hold up to 1 GiB
/// (the engine's `memory_reservation`), and keep 64 MiB of snapshot pages
/// mapped so reset cost scales with what a call dirtied.
const POOL: PoolSpec = PoolSpec {
    slots: 512,
    max_memory: 1 << 30,
    keep_resident: 64 << 20,
    max_wasm_stack: MAX_WASM_STACK,
};

static RUNTIME: OnceLock<std::result::Result<GuestRuntime, String>> = OnceLock::new();

fn runtime() -> std::result::Result<&'static GuestRuntime, &'static str> {
    RUNTIME
        .get_or_init(|| {
            GuestRuntime::new(
                bashkit_cpython_wasm::engine_config,
                &POOL,
                bashkit_cpython_wasm::load_module,
                http::add_to_linker,
            )
            .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(String::as_str)
}

/// CPython `python`/`python3` builtin.
pub struct CPython {
    limits: CPythonLimits,
}

impl CPython {
    /// Create the builtin with custom limits.
    pub fn with_limits(limits: CPythonLimits) -> Self {
        Self { limits }
    }

    /// Load the embedded interpreter now instead of on the first `python3`
    /// call. Idempotent and process-wide; returns the error text if the
    /// embedded module cannot be loaded.
    pub fn warm_up() -> std::result::Result<(), String> {
        runtime().map(|_| ()).map_err(str::to_string)
    }
}

impl Default for CPython {
    fn default() -> Self {
        Self::with_limits(CPythonLimits::default())
    }
}

#[async_trait]
impl Builtin for CPython {
    fn llm_hint(&self) -> Option<&'static str> {
        Some(
            "python/python3: CPython 3.14 in a WebAssembly sandbox. Full pure-Python \
             stdlib plus json, re, csv, sqlite3, zlib, hashlib, decimal, datetime. \
             open()/pathlib/os work on the virtual filesystem. -c, -m, script files and \
             stdin work. HTTP via requests, httpx (common API subsets) or \
             urllib.request, only to hosts the network allowlist permits; no raw sockets, \
             no subprocess, no threads, no pip or other third-party packages.",
        )
    }

    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let rt = match runtime() {
            Ok(rt) => rt,
            Err(e) => {
                return Ok(ExecResult::err(
                    format!("python3: embedded interpreter unavailable: {e}\n"),
                    1,
                ));
            }
        };

        let limits = &self.limits.common;
        let mut timeout = limits.max_duration;
        if let Some(remaining) = ctx
            .execution_extension::<ExecutionDeadline>()
            .and_then(|deadline| deadline.try_with(ExecutionDeadline::remaining).ok())
        {
            timeout = timeout.min(remaining);
        }
        let budget = ctx
            .execution_budget()
            .map(|b| {
                b.try_with(Clone::clone)
                    .map_err(|_| crate::Error::Cancelled)
            })
            .transpose()?;

        let stdin = ctx.stdin.map(|s| s.as_bytes().to_vec()).unwrap_or_default();
        ctx.consume_budget_input(stdin.len())?;
        // Charge a fixed cost per call up front (instance setup), then the
        // guest work actually done (see below), so repeated calls in one
        // request draw down one shared budget.
        ctx.consume_budget_work(CALL_WORK_UNITS)?;

        // THREAT[TM-INF]: only exported variables reach the guest, never
        // shell-local ones.
        let mut env: Vec<(String, String)> = ctx
            .env
            .iter()
            .filter(|(k, _)| k.as_str() != "PWD" && !k.starts_with("__BASHKIT_"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        env.sort();
        env.push(("PWD".to_string(), ctx.cwd.to_string_lossy().into_owned()));
        env.push((
            "__BASHKIT_RECURSION_LIMIT".to_string(),
            limits.max_call_depth.to_string(),
        ));

        let mut args = Vec::with_capacity(ctx.args.len() + 1);
        args.push("python3".to_string());
        args.extend(ctx.args.iter().cloned());

        let deadline = crate::time_compat::Instant::now() + timeout;
        let state = GuestState::new(
            ctx.fs.clone(),
            GuestConfig {
                args,
                env,
                stdin,
                output_cap: self.limits.max_output,
                max_memory: rt.clamp_memory(limits.max_memory),
                overlay: Some((
                    bashkit_cpython_wasm::STDLIB_ZIP_PATH,
                    bashkit_cpython_wasm::STDLIB_ZIP,
                )),
                deadline,
                http: http::HttpState::new(
                    #[cfg(feature = "http_client")]
                    ctx.http_client.cloned(),
                    self.limits.max_http_requests,
                ),
            },
        );

        let done = wasi_host::run(
            rt,
            state,
            Call {
                name: "python3",
                noun: "interpreter",
                entry: Entry::Run("bashkit_run"),
                timeout,
                deadline,
                budget: budget.as_ref(),
                yield_fuel: YIELD_INTERVAL_FUEL,
            },
        )
        .await?;
        ctx.consume_budget_work(done.fuel_used / FUEL_PER_WORK_UNIT)?;

        let mut stderr = done.stderr;
        let exit_code = if done.flush_failed {
            stderr.extend_from_slice(b"python3: failed to write back open files\n");
            if done.exit_code == 0 {
                1
            } else {
                done.exit_code
            }
        } else {
            done.exit_code
        };
        if done.truncated {
            stderr.extend_from_slice(
                format!(
                    "python3: output truncated at {} bytes\n",
                    self.limits.max_output
                )
                .as_bytes(),
            );
        }
        let mut result = ExecResult::with_code(done.stdout, exit_code);
        result.stderr = stderr.into();
        if done.truncated {
            result.stdout_truncated = true;
            result.stderr_truncated = true;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
