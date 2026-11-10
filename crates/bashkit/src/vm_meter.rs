//! Per-VM memory metering for the embedded Monty and ZapCode interpreters.
//!
//! Important decision (security): neither interpreter can safely bound its own
//! heap in a multi-tenant process. Monty 1.x enforces `max_memory` through a
//! process-wide counter that only `monty-alloc` (installed as the global
//! allocator, aborting the process at its hard limit) writes, and ZapCode 1.5.1
//! never reads `memory_limit_bytes` at all. Instead:
//!
//! - [`MeteredAllocator`] wraps `System` and charges each allocation to the
//!   meter of the VM currently running *on this thread*, if any. Monty's
//!   `start`/`resume` calls and ZapCode's run/resume steps are synchronous, so
//!   the "VM window" is exact: no other task can run on the thread while it is
//!   open.
//! - Monty's memory probe (`set_memory_probe`, pydantic/monty#740) reads that
//!   thread's meter, so every in-VM memory check sees only its own VM.
//!   Monty's checkpoint hook runs bashkit's shared `ExecutionBudget` check
//!   (cancellation, deadline, work units) at VM time checkpoints.
//! - ZapCode (vendored, `bashkit fork:` edits) checks [`over_limit`] at every
//!   instruction and calls [`reserve`] before operations whose output can be
//!   far larger than their inputs (see `zapcode_guard.rs`).
//!
//! The allocator never aborts and never refuses: the VM stops with a memory
//! error at its next checkpoint. Frees outside a window are not refunded, so
//! the meter can only over-count (the safe direction).
//!
//! A `#[global_allocator]` in a `cdylib` only governs that library's Rust
//! allocations, so the Python/Node/C bindings can install this without touching
//! CPython's or Node's allocator. Rust embedders opt in explicitly; without it,
//! `python3` and `ts`/`node` fail closed (see [`allocator_installed`]).
//!
//! See knowledge/runtimes/python-builtin.md and runtimes/zapcode-runtime.md.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

#[cfg(feature = "python")]
use monty_types::ResourceError;

/// Monty heap bytes per shared work unit (matches the 0.0.19 accounting).
const BYTES_PER_WORK_UNIT: usize = 64;

/// Accounting for one VM run. Only touched from inside a VM window on the
/// thread running that window, so relaxed atomics suffice (they exist only to
/// make the type `Sync` for the raw pointer the allocator reads).
#[derive(Debug, Default)]
pub(crate) struct VmMeter {
    live: AtomicUsize,
    peak: AtomicUsize,
    checkpoints: AtomicU64,
    charged: AtomicUsize,
    budget: Option<crate::limits::ExecutionBudget>,
    /// Byte limit checked by [`over_limit`]/[`reserve`]; 0 = none (Monty
    /// enforces its own limit through the probe).
    limit: usize,
    /// Set when a [`reserve`] call refused; the VM fails at its next check.
    exceeded: AtomicBool,
}

impl VmMeter {
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(crate) fn new(budget: Option<crate::limits::ExecutionBudget>) -> Self {
        Self {
            budget,
            ..Self::default()
        }
    }

    /// Meter that refuses growth past `limit` bytes (ZapCode).
    #[cfg_attr(not(feature = "typescript"), allow(dead_code))]
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }

    /// Bytes this VM currently holds.
    /// True when this meter holds more than its limit or refused a
    /// [`reserve`]; usable between steps, outside any window.
    #[cfg_attr(not(feature = "typescript"), allow(dead_code))]
    pub(crate) fn is_over_limit(&self) -> bool {
        self.limit > 0 && (self.exceeded.load(Ordering::Relaxed) || self.live() > self.limit)
    }

    pub(crate) fn live(&self) -> usize {
        self.live.load(Ordering::Relaxed)
    }

    /// Highest `live` value seen.
    pub(crate) fn peak(&self) -> usize {
        self.peak.load(Ordering::Relaxed)
    }

    #[inline]
    fn charge(&self, size: usize) {
        let live = self
            .live
            .fetch_add(size, Ordering::Relaxed)
            .saturating_add(size);
        if live > self.peak.load(Ordering::Relaxed) {
            self.peak.store(live, Ordering::Relaxed);
        }
    }

    #[inline]
    fn refund(&self, size: usize) {
        let live = self.live.load(Ordering::Relaxed);
        self.live
            .store(live.saturating_sub(size), Ordering::Relaxed);
    }

    /// Shared-budget work at one VM checkpoint: cancellation/deadline check,
    /// one unit per 64 checkpoints, and heap peak growth past what was already
    /// charged (one unit per [`BYTES_PER_WORK_UNIT`]).
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    fn checkpoint(&self) -> Result<(), crate::limits::LimitExceeded> {
        let Some(budget) = &self.budget else {
            return Ok(());
        };
        budget.check()?;
        let n = self.checkpoints.fetch_add(1, Ordering::Relaxed) + 1;
        if n.is_multiple_of(64) {
            budget.consume_work(1)?;
        }
        let peak = self.peak();
        let charged = self.charged.load(Ordering::Relaxed);
        if peak > charged {
            let units = peak.div_ceil(BYTES_PER_WORK_UNIT) - charged.div_ceil(BYTES_PER_WORK_UNIT);
            self.charged.store(peak, Ordering::Relaxed);
            budget.consume_work(u64::try_from(units).unwrap_or(u64::MAX))?;
        }
        Ok(())
    }
}

thread_local! {
    // Const-initialised, no destructor: safe to read from inside the allocator,
    // including during thread teardown.
    static CURRENT: Cell<*const VmMeter> = const { Cell::new(std::ptr::null()) };
}

#[inline]
fn with_current(f: impl FnOnce(&VmMeter)) {
    let ptr = CURRENT.try_with(Cell::get).unwrap_or(std::ptr::null());
    if !ptr.is_null() {
        // SAFETY: `ptr` is only non-null while a `VmWindow` borrowing the meter
        // is alive on this thread (see `VmWindow::enter`).
        f(unsafe { &*ptr });
    }
}

/// Global allocator that charges allocations made inside a Monty VM window to
/// that VM. Install it in the final binary or `cdylib`:
///
/// ```rust,ignore
/// #[global_allocator]
/// static ALLOC: bashkit::MeteredAllocator = bashkit::MeteredAllocator;
/// ```
pub struct MeteredAllocator;

// SAFETY: every method forwards its arguments unchanged to `System` and returns
// what `System` returned; the accounting never allocates or touches the block.
unsafe impl GlobalAlloc for MeteredAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded unchanged.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            with_current(|m| m.charge(layout.size()));
        }
        ptr
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded unchanged.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            with_current(|m| m.charge(layout.size()));
        }
        ptr
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        with_current(|m| m.refund(layout.size()));
        // SAFETY: `ptr`/`layout` come from this allocator.
        unsafe { System.dealloc(ptr, layout) };
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forwarded unchanged.
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            with_current(|m| {
                if new_size >= layout.size() {
                    m.charge(new_size - layout.size());
                } else {
                    m.refund(layout.size() - new_size);
                }
            });
        }
        new_ptr
    }
}

/// RAII guard: while alive, allocations on this thread charge `meter`.
pub(crate) struct VmWindow {
    previous: *const VmMeter,
}

impl VmWindow {
    /// Open a window. The guard must drop before `meter` does, and must not
    /// be held across an `.await` (it is `!Send`, which enforces that).
    pub(crate) fn enter(meter: &VmMeter) -> Self {
        let previous = CURRENT.with(|c| c.replace(meter as *const VmMeter));
        Self { previous }
    }
}

impl Drop for VmWindow {
    fn drop(&mut self) {
        CURRENT.with(|c| c.set(self.previous));
    }
}

#[cfg(feature = "python")]
fn probe() -> usize {
    let mut used = 0;
    with_current(|m| used = m.live());
    used
}

#[cfg(feature = "python")]
fn checkpoint_hook() -> Result<(), ResourceError> {
    let mut result = Ok(());
    with_current(|m| {
        if m.checkpoint().is_err() {
            // Monty has no host-defined error variant; the shared budget keeps
            // the precise poisoned reason. An uncatchable memory error only
            // stops the VM here (same as the 0.0.19 BudgetTracker).
            result = Err(ResourceError::Memory { limit: 0, used: 1 });
        }
    });
    result
}

/// Whether [`MeteredAllocator`] is the global allocator (checked once).
pub(crate) fn allocator_installed() -> bool {
    static INSTALLED: OnceLock<bool> = OnceLock::new();
    *INSTALLED.get_or_init(|| {
        let meter = VmMeter::default();
        {
            let _w = VmWindow::enter(&meter);
            let probe_alloc = std::hint::black_box(vec![0u8; 256]);
            drop(probe_alloc);
        }
        meter.peak() > 0
    })
}

/// True when the VM running on this thread holds more than its limit, or a
/// [`reserve`] call already refused. False outside a window or without a limit.
#[cfg_attr(not(feature = "typescript"), allow(dead_code))]
pub(crate) fn over_limit() -> bool {
    let mut over = false;
    with_current(|m| over = m.is_over_limit());
    over
}

/// Ask whether the VM on this thread may grow by `bytes` more. Refusal is
/// sticky ([`over_limit`] then reports it), so callers that cannot return an
/// error can return a cheap placeholder and let the next check stop the VM.
/// Outside a window there is no meter, and growth is refused past
/// [`UNMETERED_LIMIT`] so a stray conversion cannot request unbounded memory.
#[cfg_attr(not(feature = "typescript"), allow(dead_code))]
pub(crate) fn reserve(bytes: usize) -> bool {
    let mut metered = false;
    let mut ok = true;
    with_current(|m| {
        if m.limit > 0 {
            metered = true;
            ok = m.live().saturating_add(bytes) <= m.limit;
            if !ok {
                m.exceeded.store(true, Ordering::Relaxed);
            }
        }
    });
    if metered {
        ok
    } else {
        bytes <= UNMETERED_LIMIT
    }
}

/// Bytes the VM on this thread may still grow by ([`UNMETERED_LIMIT`]
/// outside a metered window).
#[cfg_attr(not(feature = "typescript"), allow(dead_code))]
pub(crate) fn remaining() -> usize {
    let mut left = UNMETERED_LIMIT;
    with_current(|m| {
        if m.limit > 0 {
            left = m.limit.saturating_sub(m.live());
        }
    });
    left
}

/// Growth cap for [`reserve`] outside any metered window: the default
/// `RuntimeLimits::max_memory`.
pub(crate) const UNMETERED_LIMIT: usize = 64 * 1024 * 1024;

/// Install Monty's memory probe and checkpoint hook (once per process) and
/// report whether [`MeteredAllocator`] is the global allocator. Returns an
/// error message when Monty must fail closed.
#[cfg(feature = "python")]
pub(crate) fn ensure_metering() -> Result<(), &'static str> {
    static STATE: OnceLock<Result<(), &'static str>> = OnceLock::new();
    *STATE.get_or_init(|| {
        if !allocator_installed() {
            return Err(
                "python3: Monty memory limits need bashkit::MeteredAllocator as the global allocator",
            );
        }
        monty_types::set_memory_probe(probe)
            .map_err(|_| "python3: another Monty memory probe is already installed")?;
        monty_types::set_checkpoint_hook(checkpoint_hook)
            .map_err(|_| "python3: another Monty checkpoint hook is already installed")?;
        Ok(())
    })
}
