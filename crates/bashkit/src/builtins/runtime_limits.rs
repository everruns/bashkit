//! Shared resource limits for embedded language runtimes.
//!
//! The embedded VM builtins (Python via Monty, TypeScript via ZapCode) each
//! need the same core sandbox knobs: a wall-clock budget, a memory cap, an
//! allocation cap, and a call-depth cap. Rather than each runtime re-declaring
//! those four axes, their defaults, and their fluent setters, they share this
//! [`RuntimeLimits`] core and add only runtime-specific naming/defaults on top
//! (e.g. Python calls the depth knob `max_recursion`, TypeScript calls it
//! `max_stack_depth`).
//!
//! This is intentionally scoped to *general-purpose VM* runtimes. SQLite is a
//! query engine, not a VM — its limits (rows, statements, PRAGMAs, db size)
//! don't map onto these axes — so it keeps its own `SqliteLimits` and is not
//! forced into this shape.

use std::time::Duration;

/// Default wall-clock budget for one embedded-runtime invocation.
pub(crate) const DEFAULT_MAX_DURATION: Duration = Duration::from_secs(30);
/// Default memory cap (64 MB).
pub(crate) const DEFAULT_MAX_MEMORY: usize = 64 * 1024 * 1024;
/// Default heap-allocation cap.
pub(crate) const DEFAULT_MAX_ALLOCATIONS: usize = 1_000_000;
/// Neutral default call-depth cap; each runtime overrides with its own default.
pub(crate) const DEFAULT_MAX_CALL_DEPTH: usize = 256;

/// Core resource limits shared by embedded language-VM runtimes.
///
/// Carried as the `common` field of `PythonLimits` / `TypeScriptLimits`. Use
/// the runtime-specific limit types and their fluent setters to configure these
/// — they delegate here.
#[derive(Debug, Clone)]
pub struct RuntimeLimits {
    /// Maximum execution time for one invocation (default: 30s).
    pub max_duration: Duration,
    /// Maximum memory in bytes (default: 64 MB).
    pub max_memory: usize,
    /// Maximum heap allocations (default: 1,000,000).
    pub max_allocations: usize,
    /// Maximum call/recursion depth. Defaults vary per runtime.
    pub max_call_depth: usize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            max_duration: DEFAULT_MAX_DURATION,
            max_memory: DEFAULT_MAX_MEMORY,
            max_allocations: DEFAULT_MAX_ALLOCATIONS,
            max_call_depth: DEFAULT_MAX_CALL_DEPTH,
        }
    }
}

impl RuntimeLimits {
    /// Set the wall-clock budget.
    #[must_use]
    pub fn max_duration(mut self, d: Duration) -> Self {
        self.max_duration = d;
        self
    }

    /// Set the memory cap in bytes.
    #[must_use]
    pub fn max_memory(mut self, bytes: usize) -> Self {
        self.max_memory = bytes;
        self
    }

    /// Set the heap-allocation cap.
    #[must_use]
    pub fn max_allocations(mut self, n: usize) -> Self {
        self.max_allocations = n;
        self
    }

    /// Set the call/recursion-depth cap.
    #[must_use]
    pub fn max_call_depth(mut self, depth: usize) -> Self {
        self.max_call_depth = depth;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_uses_shared_constants() {
        let limits = RuntimeLimits::default();
        assert_eq!(limits.max_duration, DEFAULT_MAX_DURATION);
        assert_eq!(limits.max_memory, DEFAULT_MAX_MEMORY);
        assert_eq!(limits.max_allocations, DEFAULT_MAX_ALLOCATIONS);
        assert_eq!(limits.max_call_depth, DEFAULT_MAX_CALL_DEPTH);
    }

    #[test]
    fn fluent_setters_override_each_axis() {
        let limits = RuntimeLimits::default()
            .max_duration(Duration::from_secs(7))
            .max_memory(2048)
            .max_allocations(99)
            .max_call_depth(33);
        assert_eq!(limits.max_duration, Duration::from_secs(7));
        assert_eq!(limits.max_memory, 2048);
        assert_eq!(limits.max_allocations, 99);
        assert_eq!(limits.max_call_depth, 33);
    }
}
