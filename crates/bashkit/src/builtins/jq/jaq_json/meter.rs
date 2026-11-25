//! BASHKIT: live-memory meter for jq values (TM-DOS-110, #2444).
//!
//! Not part of upstream jaq-json. Every string buffer and every array/object
//! allocation made by a value operation is charged to the meter installed
//! for the current jq run and released when the allocation is dropped, so
//! the meter holds the bytes live values own right now. Operations that grow
//! a value check the meter *before* allocating and fail with a jq error;
//! infallible constructors (`FromIterator`, `From<String>`) cannot fail, so
//! they stop growing, trip the meter and the caller reports it.
//!
//! Decision: the meter lives in a thread-local, set by `install` for the
//! duration of one filter run. jaq evaluation is synchronous on the calling
//! thread, and values keep their own `Arc<Meter>`, so a value dropped after
//! the run (or on another thread) still releases against the right meter.
//!
//! Decision: where no error can be returned (an infallible constructor, or
//! a loop that allocates evaluator state without emitting, like
//! `until(false; .)`), the run is aborted by unwinding with an [`Abort`]
//! payload via `resume_unwind` (no panic hook, no message), and the jq
//! builtin catches it. Value operations also `tick`, and every
//! [`TICKS_PER_POLL`] ticks the installed stop check (the execution
//! deadline) is polled, so a non-emitting loop cannot outlive the timeout.
//! The jq context wrapper also ticks this meter on every clone and aborts at
//! 64 simultaneous contexts, covering recursion with no value operations.
//! Generated structure has a separate depth ceiling: bytes and contexts do
//! not bound the native stack consumed by recursive traversal or destruction.
//! Container admission checks depth, cached on each body and invalidated by
//! mutable access so shared trees and wide scalar updates remain efficient.
//! Builds with `panic = "abort"` cannot unwind: there the meter only trips
//! and values are cut short, and a non-emitting loop is not interrupted.

use std::cell::{Cell, RefCell};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use bytes::Bytes;

use super::Error;

/// Live bytes held by one jq run, with its limit.
#[derive(Debug)]
pub struct Meter {
    live: AtomicUsize,
    limit: usize,
    tripped: AtomicBool,
    depth_exceeded: AtomicBool,
}

/// A stop check polled while a filter runs (the execution deadline).
pub type StopCheck = Box<dyn Fn() -> bool>;

/// Value operations between two polls of the stop check.
pub const TICKS_PER_POLL: usize = 4096;

/// Why a run was aborted by unwinding.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Abort {
    /// A value outgrew the meter's limit where no error could be returned.
    Memory,
    /// A generated value exceeded the structural nesting limit.
    Depth,
    /// The stop check fired (execution deadline).
    Interrupted,
    /// The evaluator retained too many live contexts (recursive filters).
    Recursion,
}

struct Poll {
    stop: StopCheck,
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Meter>>> = const { RefCell::new(None) };
    static POLL: RefCell<Option<Poll>> = const { RefCell::new(None) };
    // BASHKIT PATCH: the hot path touches one Cell, borrowing the callback only
    // at a poll boundary. Guard restoration preserves nested-run progress.
    static TICKS: Cell<usize> = const { Cell::new(0) };
}

/// Restores the previously installed meter when dropped.
pub struct MeterGuard {
    prev: Option<Arc<Meter>>,
    prev_poll: Option<Poll>,
    prev_ticks: usize,
}

impl Drop for MeterGuard {
    fn drop(&mut self) {
        let prev = self.prev.take();
        CURRENT.with(|c| *c.borrow_mut() = prev);
        let prev_poll = self.prev_poll.take();
        TICKS.with(|ticks| ticks.set(self.prev_ticks));
        POLL.with(|p| *p.borrow_mut() = prev_poll);
    }
}

/// Meter all values created on this thread until the guard drops, polling
/// `stop` every [`TICKS_PER_POLL`] value operations.
pub fn install(limit: usize, stop: Option<StopCheck>) -> (MeterGuard, Arc<Meter>) {
    let meter = Arc::new(Meter {
        live: AtomicUsize::new(0),
        limit,
        tripped: AtomicBool::new(false),
        depth_exceeded: AtomicBool::new(false),
    });
    let prev = CURRENT.with(|c| c.borrow_mut().replace(meter.clone()));
    let poll = stop.map(|stop| Poll { stop });
    let prev_poll = POLL.with(|p| core::mem::replace(&mut *p.borrow_mut(), poll));
    let prev_ticks = TICKS.with(|ticks| ticks.replace(0));
    (MeterGuard { prev, prev_poll, prev_ticks }, meter)
}

/// Stop the run now. Only returns in `panic = "abort"` builds, where the
/// caller carries on with a tripped meter.
fn abort(kind: Abort) {
    if matches!(kind, Abort::Memory | Abort::Depth) {
        trip();
    }
    #[cfg(panic = "unwind")]
    std::panic::resume_unwind(Box::new(kind));
    #[cfg(not(panic = "unwind"))]
    let _ = kind;
}

/// Stop evaluation when the host-side context budget is exhausted.
pub fn abort_recursion() {
    abort(Abort::Recursion);
}

/// Count one value operation; poll the stop check now and then.
pub fn tick() {
    let due = TICKS.with(|ticks| {
        let next = ticks.get().wrapping_add(1);
        ticks.set(next);
        next.is_multiple_of(TICKS_PER_POLL)
    });
    if !due { return; }
    let stop = POLL.with(|p| p.borrow().as_ref().is_some_and(|poll| (poll.stop)()));
    if stop {
        abort(Abort::Interrupted);
    }
}

fn current() -> Option<Arc<Meter>> {
    CURRENT.with(|c| c.borrow().clone())
}

impl Meter {
    /// True once any value hit the limit during this run.
    pub fn tripped(&self) -> bool {
        self.tripped.load(Ordering::Relaxed)
    }

    /// The configured limit in bytes.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Diagnostic for a tripped meter.
    pub fn message(&self) -> String {
        if self.depth_exceeded.load(Ordering::Relaxed) {
            return depth_message();
        }
        format!("value size limit ({} bytes) exceeded", self.limit)
    }

    /// Would `extra` more bytes fit? Trips the meter when not.
    fn fits(&self, extra: usize) -> bool {
        let live = self.live.load(Ordering::Relaxed);
        if self.tripped() || live.saturating_add(extra) > self.limit {
            self.tripped.store(true, Ordering::Relaxed);
            return false;
        }
        true
    }

    fn charge(&self, bytes: usize) {
        self.live.fetch_add(bytes, Ordering::Relaxed);
    }

    fn release(&self, bytes: usize) {
        self.live.fetch_sub(bytes, Ordering::Relaxed);
    }
}

/// Fail before allocating `extra` bytes that would exceed the limit.
pub fn check(extra: usize) -> Result<(), Error> {
    match current() {
        Some(m) if !m.fits(extra) => Err(Error::str(m.message())),
        _ => Ok(()),
    }
}

/// BASHKIT PATCH: pre-allocation admission for infallible owned constructors
/// and Rc copy-on-write clones. Panic-abort builds leave the meter tripped.
#[cfg(feature = "experimental_jq")]
pub(crate) fn require(extra: usize) -> bool {
    if current().is_some_and(|meter| !meter.fits(extra)) {
        abort(Abort::Memory);
        return false;
    }
    true
}

/// Room left for new allocations, `usize::MAX` without a meter.
pub fn headroom() -> usize {
    match current() {
        Some(m) if !m.tripped() => m.limit.saturating_sub(m.live.load(Ordering::Relaxed)),
        Some(_) => 0,
        None => usize::MAX,
    }
}

/// Mark the current run as over its limit.
pub fn trip() {
    if let Some(m) = current() {
        m.tripped.store(true, Ordering::Relaxed);
    }
}

/// THREAT[TM-DOS-027]: generated values need their own structural bound;
/// input depth, live bytes and evaluator context counts cannot bound Drop.
pub const MAX_VALUE_DEPTH: usize = 100;

fn depth_message() -> String {
    format!("value nesting depth limit ({MAX_VALUE_DEPTH}) exceeded")
}

/// Admit a container before attaching it to a parent. Sticky across jq try.
/// In panic-abort builds callers must discard the rejected attachment.
pub fn check_depth(depth: usize) -> Result<(), Error> {
    if depth <= MAX_VALUE_DEPTH {
        return Ok(());
    }
    if let Some(m) = current() {
        m.depth_exceeded.store(true, Ordering::Relaxed);
    }
    abort(Abort::Depth);
    Err(Error::str(depth_message()))
}

/// The charge for one string buffer, released when the last value sharing
/// it drops.
struct Charge {
    meter: Arc<Meter>,
    bytes: usize,
}

impl Drop for Charge {
    fn drop(&mut self) {
        self.meter.release(self.bytes);
    }
}

/// A string body whose length is charged while alive.
///
/// Decision: the charge sits beside a plain `Bytes` (not inside it as the
/// buffer owner), so a uniquely held string can be taken out with
/// [`Str::into_bytes`] and appended to in place. `join` and other `. + $x`
/// reductions stay linear instead of copying the whole string per step.
/// Clones share the charge; a slice is a new string and is charged again.
#[derive(Clone, Default)]
pub struct Str {
    data: Bytes,
    charge: Option<Arc<Charge>>,
}

impl Str {
    /// Charge a new string. Over the limit it trips the meter and aborts.
    pub fn new(data: Bytes) -> Self {
        tick();
        let charge = current().map(|meter| {
            let bytes = data.len();
            if !meter.fits(bytes) {
                abort(Abort::Memory);
            }
            meter.charge(bytes);
            Arc::new(Charge { meter, bytes })
        });
        Self { data, charge }
    }

    /// Take the bytes out; the charge is released unless a clone holds it.
    pub fn into_bytes(self) -> Bytes {
        self.data
    }
}

impl Deref for Str {
    type Target = Bytes;
    fn deref(&self) -> &Bytes {
        &self.data
    }
}

impl AsRef<[u8]> for Str {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl PartialEq for Str {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

impl Eq for Str {}

impl PartialEq<Str> for &[u8] {
    fn eq(&self, other: &Str) -> bool {
        *self == &other.data[..]
    }
}

impl PartialOrd for Str {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Str {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.data.cmp(&other.data)
    }
}

impl core::hash::Hash for Str {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.data.hash(state)
    }
}

impl core::fmt::Debug for Str {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.data.fmt(f)
    }
}

/// Size of a container, charged while it is alive.
pub trait Footprint {
    /// Bytes owned directly (not counting what elements own themselves).
    fn footprint(&self) -> usize;
}

impl<T> Footprint for Vec<T> {
    fn footprint(&self) -> usize {
        self.capacity() * core::mem::size_of::<T>()
    }
}

impl<K, V, S> Footprint for indexmap::IndexMap<K, V, S> {
    fn footprint(&self) -> usize {
        // Entries plus the hash index (one usize per slot).
        self.capacity() * (core::mem::size_of::<(K, V)>() + 2 * core::mem::size_of::<usize>())
    }
}

/// An array or object body whose footprint is charged while alive.
///
/// Growth through `DerefMut` is re-charged by `resync`, which every mutation
/// site of the vendored code calls after mutating (marked `BASHKIT PATCH`).
pub struct Metered<T: Footprint> {
    inner: T,
    charged: usize,
    meter: Option<Arc<Meter>>,
    // BASHKIT PATCH: lazily cached structural depth, invalidated on mutation.
    // Shared subtrees must not be recursively re-walked at every wrapper.
    depth: core::cell::Cell<Option<usize>>,
}

impl<T: Footprint> Metered<T> {
    /// Charge a new container. Over the limit it trips the meter but keeps
    /// the value (it is already allocated); the run reports the trip.
    pub fn new(inner: T) -> Self {
        tick();
        let meter = current();
        let charged = inner.footprint();
        if let Some(m) = &meter {
            if !m.fits(charged) {
                abort(Abort::Memory);
            }
            m.charge(charged);
        }
        Self {
            inner,
            charged,
            meter,
            depth: core::cell::Cell::new(None),
        }
    }

    /// Read structural depth, computing it only after a mutation.
    pub fn depth(&self, compute: impl FnOnce(&T) -> usize) -> usize {
        if let Some(depth) = self.depth.get() {
            return depth;
        }
        let depth = compute(&self.inner);
        self.depth.set(Some(depth));
        depth
    }

    /// Re-charge after the container grew or shrank in place.
    pub fn resync(&mut self) {
        let now = self.inner.footprint();
        if let Some(m) = &self.meter {
            if now > self.charged {
                if !m.fits(now - self.charged) {
                    abort(Abort::Memory);
                }
                m.charge(now - self.charged);
            } else {
                m.release(self.charged - now);
            }
        }
        self.charged = now;
    }

    /// Take the container out, releasing its charge.
    pub fn into_inner(mut self) -> T
    where
        T: Default,
    {
        core::mem::take(&mut self.inner)
        // `self` drops here and releases `charged`.
    }

    /// BASHKIT PATCH: an owning iterator retains the allocated body. Transfer
    /// its charge instead of refunding while the buffer is still alive.
    #[cfg(feature = "experimental_jq")]
    pub(crate) fn into_charged_iter(mut self) -> OwnedIter<T::IntoIter>
    where
        T: Default + IntoIterator,
    {
        let inner = core::mem::take(&mut self.inner);
        let charge = self.meter.take().map(|meter| Charge { meter, bytes: self.charged });
        self.charged = 0;
        OwnedIter { iter: inner.into_iter(), _charge: charge }
    }
}

/// BASHKIT PATCH: release the allocation before refunding its meter charge.
#[cfg(feature = "experimental_jq")]
pub(crate) struct OwnedIter<I> {
    iter: I,
    _charge: Option<Charge>,
}

#[cfg(feature = "experimental_jq")]
impl<I: Iterator> Iterator for OwnedIter<I> {
    type Item = I::Item;
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

#[cfg(feature = "experimental_jq")]
impl<I: ExactSizeIterator> ExactSizeIterator for OwnedIter<I> {}

impl<T: Footprint> Drop for Metered<T> {
    fn drop(&mut self) {
        if let Some(m) = &self.meter {
            m.release(self.charged);
        }
    }
}

impl<T: Footprint + Clone> Clone for Metered<T> {
    fn clone(&self) -> Self {
        Self::new(self.inner.clone())
    }
}

impl<T: Footprint + Default> Default for Metered<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Footprint> Deref for Metered<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T: Footprint> DerefMut for Metered<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.depth.set(None);
        &mut self.inner
    }
}

impl<T: Footprint + PartialEq> PartialEq for Metered<T> {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl<T: Footprint + Eq> Eq for Metered<T> {}

impl<T: Footprint + core::hash::Hash> core::hash::Hash for Metered<T> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.inner.hash(state)
    }
}

impl<T: Footprint + core::fmt::Debug> core::fmt::Debug for Metered<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T: Footprint + Default + IntoIterator> IntoIterator for Metered<T> {
    type Item = T::Item;
    type IntoIter = T::IntoIter;
    fn into_iter(self) -> T::IntoIter {
        self.into_inner().into_iter()
    }
}

impl<'a, T: Footprint> IntoIterator for &'a Metered<T>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

/// Collect an iterator into a metered `Vec`, stopping (and tripping the
/// meter) once it would exceed the limit, so `[range(1e12)]` never
/// allocates past the limit.
pub fn collect_vec<T>(iter: impl IntoIterator<Item = T>) -> Metered<Vec<T>> {
    let room = headroom() / core::mem::size_of::<T>().max(1);
    let mut v = Vec::new();
    for x in iter {
        if v.len() >= room {
            abort(Abort::Memory);
            break;
        }
        v.push(x);
    }
    Metered::new(v)
}
