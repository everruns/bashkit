//! Shared host for WebAssembly guests that run as builtins (`cpython` and
//! `wasm-coreutils` features).
//!
//! Decisions (see `knowledge/runtimes/cpython-wasm.md` and
//! `knowledge/runtimes/wasm-coreutils.md`):
//! - Isolation comes from the wasm boundary. A guest reaches only what
//!   [`wasi`] hands it: the VFS, captured stdio, a host RNG and clocks. Guests
//!   never run native code in the host.
//! - Each guest kind keeps one process-wide engine and pre-linked module
//!   ([`GuestRuntime`]), created on first use. Every call gets a fresh store
//!   and instance; nothing survives between calls or tenants.
//! - Guest work is metered with fuel and yields to the async runtime at a
//!   fixed interval; each poll re-checks the call deadline and the request's
//!   `ExecutionBudget`, so a busy loop cannot hold a worker thread past its
//!   deadline.
//! - Instances come from wasmtime's pooling allocator: slots keep their memory
//!   mapped and are reset by discarding only the pages a call dirtied. Calls
//!   beyond the pool size wait for a slot within their own deadline. If the
//!   pool cannot reserve its address space, instances are allocated on demand.
//! - Memory is capped by a store limiter; `memory.grow` past the cap fails
//!   inside the guest, not as a host error.

pub(crate) mod wasi;

pub(crate) use wasi::{GuestClock, GuestConfig, GuestState, ProcExit};

use std::future::Future;
use std::task::Poll;
use std::time::Duration;

use wasmtime::{Engine, InstancePre, Linker, Module, Store};

/// Pool sizing for one guest kind.
pub(crate) struct PoolSpec {
    /// Concurrent instances per process.
    pub(crate) slots: u32,
    /// Largest guest memory a slot holds; must not exceed the engine's
    /// `memory_reservation`.
    pub(crate) max_memory: usize,
    /// Pages kept mapped in a released slot.
    pub(crate) keep_resident: usize,
    /// Wasm call-stack budget.
    pub(crate) max_wasm_stack: usize,
}

/// Process-wide compiled guest of one kind.
pub(crate) struct GuestRuntime {
    engine: Engine,
    pre: InstancePre<GuestState>,
    /// Concurrent instances per process; the same cap applies when the pool
    /// could not be created and instances are allocated on demand.
    slots: tokio::sync::Semaphore,
    #[cfg(all(test, feature = "cpython"))]
    pooled: bool,
    max_memory: usize,
}

impl GuestRuntime {
    /// Build an engine from `base` (the module's compile-affecting config),
    /// load the module and link the WASI imports plus `extra` ones.
    pub(crate) fn new(
        base: fn() -> wasmtime::Config,
        spec: &PoolSpec,
        load: fn(&Engine) -> wasmtime::Result<Module>,
        extra: fn(&mut Linker<GuestState>) -> wasmtime::Result<()>,
    ) -> wasmtime::Result<Self> {
        // Pooled slots first; on-demand allocation (mmap + munmap per call)
        // serializes many concurrent calls on the kernel's address-space lock.
        match Engine::new(&runtime_config(base(), spec, true)) {
            Ok(engine) => Self::link(engine, true, spec, load, extra),
            Err(_) => Self::link(
                Engine::new(&runtime_config(base(), spec, false))?,
                false,
                spec,
                load,
                extra,
            ),
        }
    }

    pub(crate) fn link(
        engine: Engine,
        #[cfg_attr(not(all(test, feature = "cpython")), allow(unused_variables))] pooled: bool,
        spec: &PoolSpec,
        load: fn(&Engine) -> wasmtime::Result<Module>,
        extra: fn(&mut Linker<GuestState>) -> wasmtime::Result<()>,
    ) -> wasmtime::Result<Self> {
        let module = load(&engine)?;
        let mut linker = Linker::new(&engine);
        wasi::add_to_linker(&mut linker)?;
        extra(&mut linker)?;
        let pre = linker.instantiate_pre(&module)?;
        Ok(Self {
            engine,
            pre,
            // THREAT[TM-WCU-006]: the same concurrency and memory caps apply
            // with or without the pool.
            slots: tokio::sync::Semaphore::new(spec.slots as usize),
            #[cfg(all(test, feature = "cpython"))]
            pooled,
            max_memory: spec.max_memory,
        })
    }

    /// Whether instances come from the pool.
    #[cfg(all(test, feature = "cpython"))]
    pub(crate) fn pooled(&self) -> bool {
        self.pooled
    }

    /// `requested` clamped to what one slot can hold.
    pub(crate) fn clamp_memory(&self, requested: usize) -> usize {
        requested.min(self.max_memory)
    }
}

/// Engine configuration for running (not compiling) a guest.
pub(crate) fn runtime_config(
    mut config: wasmtime::Config,
    spec: &PoolSpec,
    pooled: bool,
) -> wasmtime::Config {
    // Runtime-only settings; they do not affect module compatibility.
    config.max_wasm_stack(spec.max_wasm_stack);
    config.async_stack_size(spec.max_wasm_stack + (1 << 20));
    if pooled {
        let mut pool = wasmtime::PoolingAllocationConfig::new();
        pool.total_core_instances(spec.slots)
            .total_memories(spec.slots)
            .total_tables(spec.slots)
            .total_stacks(spec.slots)
            .max_memory_size(spec.max_memory)
            .table_elements(1 << 16)
            .max_core_instance_size(1 << 20)
            .linear_memory_keep_resident(spec.keep_resident)
            .pagemap_scan(wasmtime::Enabled::Auto);
        config.allocation_strategy(wasmtime::InstanceAllocationStrategy::Pooling(pool));
    }
    config
}

/// How the guest is entered.
pub(crate) enum Entry {
    /// Reactor export `() -> i32` returning the exit status.
    #[cfg_attr(not(feature = "cpython"), allow(dead_code))]
    Run(&'static str),
    /// WASI command `_start`; the status comes from `proc_exit` (0 if it
    /// returns).
    #[cfg_attr(not(feature = "wasm-coreutils"), allow(dead_code))]
    Start,
}

/// One guest call.
pub(crate) struct Call<'a> {
    /// Prefix for host-generated diagnostics (`python3`, `sort`, ...).
    pub(crate) name: &'a str,
    /// What the guest is, for trap messages (`interpreter`, `program`).
    pub(crate) noun: &'a str,
    pub(crate) entry: Entry,
    pub(crate) timeout: Duration,
    pub(crate) deadline: crate::time_compat::Instant,
    pub(crate) budget: Option<&'a crate::limits::ExecutionBudget>,
    /// Guest instructions between cooperative yields.
    pub(crate) yield_fuel: u64,
    /// Guest instructions this call may run in total; running out traps.
    /// Set from the request's remaining work budget.
    pub(crate) fuel: u64,
    /// Per-tenant concurrency share, acquired before a pool slot.
    pub(crate) tenant: Option<&'a tokio::sync::Semaphore>,
}

/// What a finished call produced.
pub(crate) struct Finished {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) exit_code: i32,
    /// Output hit the cap and was cut.
    pub(crate) truncated: bool,
    /// Guest instructions executed.
    pub(crate) fuel_used: u64,
    /// Writing back an open file at exit failed.
    pub(crate) flush_failed: bool,
}

/// Why a guest call stopped without returning.
enum Stop {
    Timeout,
    Budget(crate::limits::LimitExceeded),
}

/// Run one call on a fresh instance. A closed request budget is the only
/// error; traps, timeouts and exits all become a [`Finished`].
pub(crate) async fn run(
    rt: &GuestRuntime,
    state: GuestState,
    call: Call<'_>,
) -> std::result::Result<Finished, crate::limits::LimitExceeded> {
    // THREAT[TM-WCU-006]: take the tenant's share first, then a process
    // slot, and hold both until the store (and its instance) is dropped.
    // Waiting for either counts against the deadline.
    let mut permits = Vec::with_capacity(2);
    for sem in [call.tenant, Some(&rt.slots)].into_iter().flatten() {
        match guarded(sem.acquire(), call.deadline, call.budget).await {
            Ok(Ok(permit)) => permits.push(permit),
            // The semaphores are never closed.
            Ok(Err(_)) => {}
            Err(Stop::Timeout) => {
                return Ok(Finished {
                    stdout: Vec::new(),
                    stderr: timeout_message(call.name, call.timeout).into_bytes(),
                    exit_code: 124,
                    truncated: false,
                    fuel_used: 0,
                    flush_failed: false,
                });
            }
            Err(Stop::Budget(e)) => return Err(e),
        }
    }
    let mut store = Store::new(&rt.engine, state);
    store.limiter(|s| &mut s.limits);
    let setup = store
        .set_fuel(call.fuel)
        .and_then(|()| store.fuel_async_yield_interval(Some(call.yield_fuel)));
    if let Err(e) = setup {
        return Ok(Finished {
            stdout: Vec::new(),
            stderr: format!("{}: {e}\n", call.name).into_bytes(),
            exit_code: 1,
            truncated: false,
            fuel_used: 0,
            flush_failed: false,
        });
    }

    let outcome = guarded(
        async {
            let instance = rt.pre.instantiate_async(&mut store).await?;
            match call.entry {
                Entry::Run(export) => {
                    let run = instance.get_typed_func::<(), i32>(&mut store, export)?;
                    run.call_async(&mut store, ()).await
                }
                Entry::Start => {
                    let start = instance.get_typed_func::<(), ()>(&mut store, "_start")?;
                    start.call_async(&mut store, ()).await.map(|()| 0)
                }
            }
        },
        call.deadline,
        call.budget,
    )
    .await;

    let fuel_used = call.fuel - store.get_fuel().unwrap_or(0).min(call.fuel);
    // Files a guest wrote but never closed persist, as they would after a
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
                stderr.extend_from_slice(trap_message(call.name, call.noun, &e).as_bytes());
                1
            }
        }
        Err(Stop::Timeout) => {
            stderr.extend_from_slice(timeout_message(call.name, call.timeout).as_bytes());
            124
        }
        Err(Stop::Budget(e)) => return Err(e),
    };
    Ok(Finished {
        stdout: state.output.stdout,
        stderr,
        exit_code,
        truncated: state.output.truncated,
        fuel_used,
        flush_failed: state.flush_error.is_some(),
    })
}

/// Fuel for one call: the request's remaining work budget in guest
/// instructions, plus one unit, so a guest that runs out of fuel has always
/// overspent the budget and the post-call charge fails the request.
pub(crate) fn fuel_for(
    budget: Option<&crate::limits::ExecutionBudget>,
    fuel_per_work_unit: u64,
) -> u64 {
    budget.map_or(u64::MAX, |b| {
        b.remaining_work()
            .saturating_add(1)
            .saturating_mul(fuel_per_work_unit)
    })
}

pub(crate) fn timeout_message(name: &str, timeout: Duration) -> String {
    format!(
        "{name}: execution timed out after {:.1}s\n",
        timeout.as_secs_f64()
    )
}

/// How often a wait nothing else wakes re-checks the deadline and budget.
/// Cancellation is a flag without a waker, so it needs this tick.
const CHECK_TICK: Duration = Duration::from_millis(50);

/// Poll `fut`, stopping at the deadline or when the request budget closes.
/// Fuel yields return a busy guest to this poll regularly; a timer tick
/// covers waits that nothing wakes (a pool slot that never frees, a host
/// call parked on I/O), so they cannot outlive the deadline either.
async fn guarded<F: Future>(
    fut: F,
    deadline: crate::time_compat::Instant,
    budget: Option<&crate::limits::ExecutionBudget>,
) -> std::result::Result<F::Output, Stop> {
    let mut fut = std::pin::pin!(fut);
    let mut tick = std::pin::pin!(tokio::time::sleep(CHECK_TICK));
    std::future::poll_fn(|cx| {
        if crate::time_compat::Instant::now() >= deadline {
            return Poll::Ready(Err(Stop::Timeout));
        }
        if let Some(budget) = budget
            && let Err(e) = budget.check()
        {
            return Poll::Ready(Err(Stop::Budget(e)));
        }
        if let Poll::Ready(out) = fut.as_mut().poll(cx) {
            return Poll::Ready(Ok(out));
        }
        // Re-arm until it registers a wake-up for the next check.
        while tick.as_mut().poll(cx).is_ready() {
            let next = tokio::time::Instant::now() + CHECK_TICK;
            tick.as_mut().reset(next);
        }
        Poll::Pending
    })
    .await
}

/// Describe a guest trap without leaking internal shapes (TM-INF-022).
pub(crate) fn trap_message(name: &str, noun: &str, e: &wasmtime::Error) -> String {
    match e.downcast_ref::<wasmtime::Trap>() {
        Some(wasmtime::Trap::StackOverflow) => {
            format!("{name}: fatal error: stack overflow in the {noun}\n")
        }
        Some(wasmtime::Trap::OutOfFuel) => format!("{name}: fatal error: out of fuel\n"),
        Some(wasmtime::Trap::UnreachableCodeReached) => {
            format!("{name}: fatal error: {noun} aborted\n")
        }
        Some(trap) => format!("{name}: fatal error: {trap}\n"),
        None => {
            let mut msg = e.to_string();
            msg.truncate(msg.floor_char_boundary(512));
            format!("{name}: fatal error: {msg}\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trap_messages_are_display_only() {
        let e = wasmtime::Error::from(wasmtime::Trap::StackOverflow);
        assert_eq!(
            trap_message("python3", "interpreter", &e),
            "python3: fatal error: stack overflow in the interpreter\n"
        );
        let e = wasmtime::Error::from(wasmtime::Trap::UnreachableCodeReached);
        assert_eq!(
            trap_message("sort", "program", &e),
            "sort: fatal error: program aborted\n"
        );
        let long = wasmtime::Error::msg("x".repeat(4096));
        assert!(trap_message("python3", "interpreter", &long).len() < 600);
    }

    // A wait that nothing wakes (a pool slot that never frees) must still end
    // at the call deadline, not when some other call releases a slot.
    #[tokio::test]
    async fn guarded_stops_unwoken_wait_at_deadline() {
        let start = std::time::Instant::now();
        let deadline = crate::time_compat::Instant::now() + Duration::from_millis(150);
        let r = guarded(std::future::pending::<()>(), deadline, None).await;
        assert!(matches!(r, Err(Stop::Timeout)));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    // Cancellation is a flag with no waker; an unwoken wait must notice it.
    #[tokio::test]
    async fn guarded_stops_unwoken_wait_on_cancel() {
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let budget =
            crate::limits::ExecutionBudget::new(&crate::ExecutionLimits::new(), cancel.clone());
        let flag = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let start = std::time::Instant::now();
        let deadline = crate::time_compat::Instant::now() + Duration::from_secs(30);
        let r = guarded(std::future::pending::<()>(), deadline, Some(&budget)).await;
        assert!(matches!(r, Err(Stop::Budget(_))));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn timeout_message_names_the_command() {
        assert_eq!(
            timeout_message("wc", Duration::from_millis(1500)),
            "wc: execution timed out after 1.5s\n"
        );
    }
}
