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
//! - One process-wide engine and pre-linked module, created on first use (or
//!   by [`CPython::warm_up`]). Every call gets a fresh store and instance,
//!   mapped copy-on-write from the pre-initialized snapshot; nothing survives
//!   between calls or tenants.
//! - Guest work is metered with fuel and yields to the async runtime at a fixed
//!   interval; each poll re-checks the call deadline and the request's
//!   `ExecutionBudget` (cancellation, timeout), so a busy loop cannot hold a
//!   worker thread past its deadline.
//! - Instances come from wasmtime's pooling allocator (512 slots per
//!   process): slots keep their memory mapped and are reset by discarding only
//!   the pages a call dirtied, instead of mmap/munmap per call. Calls beyond
//!   512 wait for a slot within their own deadline. If the pool cannot reserve
//!   its address space, instances are allocated on demand instead.
//! - Memory is capped by a store limiter; `memory.grow` past the cap fails and
//!   surfaces as `MemoryError` inside Python, not as a host error.

mod http;
mod wasi;

pub(crate) use http::check_request_invariants as check_http_request_invariants;

use std::future::Future;
use std::sync::OnceLock;
use std::task::Poll;
use std::time::Duration;

use async_trait::async_trait;
use wasmtime::{Engine, InstancePre, Linker, Store};

use super::runtime_limits::{DEFAULT_MAX_DURATION, RuntimeLimits};
use super::{Builtin, Context, ExecutionDeadline};
use crate::error::Result;
use crate::interpreter::ExecResult;

use wasi::{GuestConfig, GuestState, ProcExit};

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

/// Concurrent guest instances per process (pooled allocator slots). Calls
/// beyond this wait for a free slot, bounded by their own deadline.
const POOL_SLOTS: u32 = 512;
/// Largest guest memory a pooled slot can hold; `max_memory` above this is
/// clamped. Must not exceed the engine's `memory_reservation` (1 GiB).
const POOL_MAX_MEMORY: usize = 1 << 30;
/// Snapshot pages kept mapped in a released slot. Reset cost then scales with
/// what a call dirtied, not with the snapshot size.
const POOL_KEEP_RESIDENT: usize = 64 << 20;

/// Process-wide compiled guest, shared by every `Bash` instance.
struct Runtime {
    engine: Engine,
    pre: InstancePre<GuestState>,
    /// Pooled slots; `None` when the pool could not be created and every
    /// instance is allocated on demand.
    slots: Option<tokio::sync::Semaphore>,
}

static RUNTIME: OnceLock<std::result::Result<Runtime, String>> = OnceLock::new();

fn runtime() -> std::result::Result<&'static Runtime, &'static str> {
    RUNTIME
        .get_or_init(|| init_runtime().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(String::as_str)
}

fn init_runtime() -> wasmtime::Result<Runtime> {
    // Pooled slots first: instances reuse pre-mapped memory and stacks, and a
    // released slot is reset by discarding only the pages a call dirtied. On
    // demand allocation (mmap + munmap per call) serializes many concurrent
    // calls on the kernel's address-space lock. Fall back to it when the pool
    // cannot reserve its address space (e.g. a tight RLIMIT_AS).
    match Engine::new(&runtime_config(true)) {
        Ok(engine) => link(
            engine,
            Some(tokio::sync::Semaphore::new(POOL_SLOTS as usize)),
        ),
        Err(_) => link(Engine::new(&runtime_config(false))?, None),
    }
}

fn runtime_config(pooled: bool) -> wasmtime::Config {
    let mut config = bashkit_cpython_wasm::engine_config();
    // Runtime-only settings; they do not affect module compatibility.
    config.max_wasm_stack(MAX_WASM_STACK);
    config.async_stack_size(MAX_WASM_STACK + (1 << 20));
    if pooled {
        let mut pool = wasmtime::PoolingAllocationConfig::new();
        pool.total_core_instances(POOL_SLOTS)
            .total_memories(POOL_SLOTS)
            .total_tables(POOL_SLOTS)
            .total_stacks(POOL_SLOTS)
            .max_memory_size(POOL_MAX_MEMORY)
            .table_elements(1 << 16)
            .max_core_instance_size(1 << 20)
            .linear_memory_keep_resident(POOL_KEEP_RESIDENT)
            .pagemap_scan(wasmtime::Enabled::Auto);
        config.allocation_strategy(wasmtime::InstanceAllocationStrategy::Pooling(pool));
    }
    config
}

fn link(engine: Engine, slots: Option<tokio::sync::Semaphore>) -> wasmtime::Result<Runtime> {
    let module = bashkit_cpython_wasm::load_module(&engine)?;
    let mut linker = Linker::new(&engine);
    wasi::add_to_linker(&mut linker)?;
    http::add_to_linker(&mut linker)?;
    let pre = linker.instantiate_pre(&module)?;
    Ok(Runtime { engine, pre, slots })
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

/// Why a guest call stopped without returning.
enum Stop {
    Timeout,
    Budget(crate::limits::LimitExceeded),
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
                max_memory: if rt.slots.is_some() {
                    limits.max_memory.min(POOL_MAX_MEMORY)
                } else {
                    limits.max_memory
                },
                stdlib_path: bashkit_cpython_wasm::STDLIB_ZIP_PATH,
                stdlib: bashkit_cpython_wasm::STDLIB_ZIP,
                deadline,
                http: http::HttpState::new(
                    #[cfg(feature = "http_client")]
                    ctx.http_client.cloned(),
                    self.limits.max_http_requests,
                ),
            },
        );

        // Hold a pooled slot until the store (and its instance) is dropped;
        // waiting for one counts against the deadline.
        let _slot = match &rt.slots {
            Some(slots) => match guarded(slots.acquire(), deadline, budget.as_ref()).await {
                Ok(Ok(permit)) => Some(permit),
                // The semaphore is never closed.
                Ok(Err(_)) => None,
                Err(Stop::Timeout) => return Ok(timed_out(timeout, Vec::new())),
                Err(Stop::Budget(e)) => return Err(e.into()),
            },
            None => None,
        };
        let mut store = Store::new(&rt.engine, state);
        store.limiter(|s| &mut s.limits);
        let setup = store
            .set_fuel(u64::MAX)
            .and_then(|()| store.fuel_async_yield_interval(Some(YIELD_INTERVAL_FUEL)));
        if let Err(e) = setup {
            return Ok(ExecResult::err(format!("python3: {e}\n"), 1));
        }

        let outcome = guarded(
            async {
                let instance = rt.pre.instantiate_async(&mut store).await?;
                let run = instance.get_typed_func::<(), i32>(&mut store, "bashkit_run")?;
                run.call_async(&mut store, ()).await
            },
            deadline,
            budget.as_ref(),
        )
        .await;

        let fuel_used = u64::MAX - store.get_fuel().unwrap_or(0);
        // Files a script wrote but never closed persist, as they would after a
        // real process exits or is killed.
        store.data_mut().flush_all().await;
        let state = store.into_data();

        let mut stderr = state.output.stderr;
        let exit_code = match outcome {
            Ok(Ok(code)) => code,
            Ok(Err(e)) => {
                if let Some(exit) = e.downcast_ref::<ProcExit>() {
                    exit.0
                } else {
                    stderr.extend_from_slice(trap_message(&e).as_bytes());
                    1
                }
            }
            Err(Stop::Timeout) => {
                stderr.extend_from_slice(timeout_message(timeout).as_bytes());
                124
            }
            Err(Stop::Budget(e)) => return Err(e.into()),
        };
        ctx.consume_budget_work(fuel_used / FUEL_PER_WORK_UNIT)?;
        let exit_code = if state.flush_error.is_some() {
            stderr.extend_from_slice(b"python3: failed to write back open files\n");
            if exit_code == 0 { 1 } else { exit_code }
        } else {
            exit_code
        };

        if state.output.truncated {
            stderr.extend_from_slice(
                format!(
                    "python3: output truncated at {} bytes\n",
                    self.limits.max_output
                )
                .as_bytes(),
            );
        }
        let mut result = ExecResult::with_code(state.output.stdout, exit_code);
        result.stderr = stderr.into();
        if state.output.truncated {
            result.stdout_truncated = true;
            result.stderr_truncated = true;
        }
        Ok(result)
    }
}

fn timeout_message(timeout: Duration) -> String {
    format!(
        "python3: execution timed out after {:.1}s\n",
        timeout.as_secs_f64()
    )
}

fn timed_out(timeout: Duration, stdout: Vec<u8>) -> ExecResult {
    let mut result = ExecResult::with_code(stdout, 124);
    result.stderr = timeout_message(timeout).into();
    result
}

/// Poll `fut`, stopping at the deadline or when the request budget closes.
/// Fuel yields guarantee the guest returns to this poll regularly.
async fn guarded<F: Future>(
    fut: F,
    deadline: crate::time_compat::Instant,
    budget: Option<&crate::limits::ExecutionBudget>,
) -> std::result::Result<F::Output, Stop> {
    let mut fut = std::pin::pin!(fut);
    std::future::poll_fn(|cx| {
        if crate::time_compat::Instant::now() >= deadline {
            return Poll::Ready(Err(Stop::Timeout));
        }
        if let Some(budget) = budget
            && let Err(e) = budget.check()
        {
            return Poll::Ready(Err(Stop::Budget(e)));
        }
        fut.as_mut().poll(cx).map(Ok)
    })
    .await
}

/// Describe a guest trap without leaking internal shapes (TM-INF-022).
fn trap_message(e: &wasmtime::Error) -> String {
    match e.downcast_ref::<wasmtime::Trap>() {
        Some(wasmtime::Trap::StackOverflow) => {
            "python3: fatal error: stack overflow in the interpreter\n".to_string()
        }
        Some(wasmtime::Trap::OutOfFuel) => "python3: fatal error: out of fuel\n".to_string(),
        Some(wasmtime::Trap::UnreachableCodeReached) => {
            "python3: fatal error: interpreter aborted\n".to_string()
        }
        Some(trap) => format!("python3: fatal error: {trap}\n"),
        None => {
            let mut msg = e.to_string();
            msg.truncate(msg.floor_char_boundary(512));
            format!("python3: fatal error: {msg}\n")
        }
    }
}

#[cfg(test)]
mod tests;
