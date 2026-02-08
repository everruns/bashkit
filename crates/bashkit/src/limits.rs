//! Resource limits for sandboxed execution.
//!
//! These limits prevent runaway scripts from consuming excessive resources.
//!
//! # Security Mitigations
//!
//! This module mitigates the following threats (see `specs/006-threat-model.md`):
//!
//! - **TM-DOS-001**: Large script input → `max_input_bytes`
//! - **TM-DOS-002, TM-DOS-004, TM-DOS-019**: Command flooding → `max_commands`
//! - **TM-DOS-016, TM-DOS-017**: Infinite loops → `max_loop_iterations`
//! - **TM-DOS-018**: Nested loop multiplication → `max_total_loop_iterations`
//! - **TM-DOS-020, TM-DOS-021**: Function recursion → `max_function_depth`
//! - **TM-DOS-022**: Parser recursion → `max_ast_depth`
//! - **TM-DOS-023**: CPU exhaustion → `timeout`
//! - **TM-DOS-024**: Parser hang → `parser_timeout`, `max_parser_operations`
//! - **TM-DOS-027**: Builtin parser recursion → `MAX_AWK_PARSER_DEPTH`, `MAX_JQ_JSON_DEPTH` (in builtins)
//!
//! # Fail Points (enabled with `failpoints` feature)
//!
//! - `limits::tick_command` - Inject failures in command counting
//! - `limits::tick_loop` - Inject failures in loop iteration counting
//! - `limits::push_function` - Inject failures in function depth tracking

use std::time::Duration;

#[cfg(feature = "failpoints")]
use fail::fail_point;

/// Resource limits for script execution
#[derive(Debug, Clone)]
pub struct ExecutionLimits {
    /// Maximum number of commands that can be executed (fuel model)
    /// Default: 10,000
    pub max_commands: usize,

    /// Maximum iterations for a single loop
    /// Default: 10,000
    pub max_loop_iterations: usize,

    // THREAT[TM-DOS-018]: Nested loops each reset their per-loop counter,
    // allowing 10K^depth total iterations. This global cap prevents that.
    /// Maximum total loop iterations across all loops (nested and sequential).
    /// Prevents nested loop multiplication attack (TM-DOS-018).
    /// Default: 1,000,000
    pub max_total_loop_iterations: usize,

    /// Maximum function call depth (recursion limit)
    /// Default: 100
    pub max_function_depth: usize,

    /// Execution timeout
    /// Default: 30 seconds
    pub timeout: Duration,

    /// Parser timeout (separate from execution timeout)
    /// Default: 5 seconds
    /// This limits how long the parser can spend parsing a script before giving up.
    /// Protects against parser hang attacks (V3 in threat model).
    pub parser_timeout: Duration,

    /// Maximum input script size in bytes
    /// Default: 10MB (10,000,000 bytes)
    /// Protects against memory exhaustion from large scripts (V1 in threat model).
    pub max_input_bytes: usize,

    /// Maximum AST nesting depth during parsing
    /// Default: 100
    /// Protects against stack overflow from deeply nested scripts (V4 in threat model).
    pub max_ast_depth: usize,

    /// Maximum parser operations (fuel model for parsing)
    /// Default: 100,000
    /// Protects against parser DoS attacks that could otherwise cause CPU exhaustion.
    pub max_parser_operations: usize,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_commands: 10_000,
            max_loop_iterations: 10_000,
            max_total_loop_iterations: 1_000_000,
            max_function_depth: 100,
            timeout: Duration::from_secs(30),
            parser_timeout: Duration::from_secs(5),
            max_input_bytes: 10_000_000, // 10MB
            max_ast_depth: 100,
            max_parser_operations: 100_000,
        }
    }
}

impl ExecutionLimits {
    /// Create new limits with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum command count
    pub fn max_commands(mut self, count: usize) -> Self {
        self.max_commands = count;
        self
    }

    /// Set maximum loop iterations (per-loop)
    pub fn max_loop_iterations(mut self, count: usize) -> Self {
        self.max_loop_iterations = count;
        self
    }

    /// Set maximum total loop iterations (across all nested/sequential loops).
    /// Prevents TM-DOS-018 nested loop multiplication.
    pub fn max_total_loop_iterations(mut self, count: usize) -> Self {
        self.max_total_loop_iterations = count;
        self
    }

    /// Set maximum function depth
    pub fn max_function_depth(mut self, depth: usize) -> Self {
        self.max_function_depth = depth;
        self
    }

    /// Set execution timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set parser timeout
    pub fn parser_timeout(mut self, timeout: Duration) -> Self {
        self.parser_timeout = timeout;
        self
    }

    /// Set maximum input script size in bytes
    pub fn max_input_bytes(mut self, bytes: usize) -> Self {
        self.max_input_bytes = bytes;
        self
    }

    /// Set maximum AST nesting depth
    pub fn max_ast_depth(mut self, depth: usize) -> Self {
        self.max_ast_depth = depth;
        self
    }

    /// Set maximum parser operations
    pub fn max_parser_operations(mut self, ops: usize) -> Self {
        self.max_parser_operations = ops;
        self
    }
}

/// Execution counters for tracking resource usage
#[derive(Debug, Clone, Default)]
pub struct ExecutionCounters {
    /// Number of commands executed
    pub commands: usize,

    /// Current function call depth
    pub function_depth: usize,

    /// Number of iterations in current loop (reset per-loop)
    pub loop_iterations: usize,

    // THREAT[TM-DOS-018]: Nested loop multiplication
    // This counter never resets, tracking total iterations across all loops.
    /// Total loop iterations across all loops (never reset)
    pub total_loop_iterations: usize,
}

impl ExecutionCounters {
    /// Create new counters
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset counters for a new exec() invocation.
    /// Each exec() is a separate script and gets its own budget.
    /// This prevents a prior exec() from permanently poisoning the session.
    pub fn reset_for_execution(&mut self) {
        self.commands = 0;
        self.loop_iterations = 0;
        self.total_loop_iterations = 0;
        // function_depth should already be 0 between exec() calls,
        // but reset defensively to avoid stuck state
        self.function_depth = 0;
    }

    /// Increment command counter, returns error if limit exceeded
    pub fn tick_command(&mut self, limits: &ExecutionLimits) -> Result<(), LimitExceeded> {
        // Fail point: test behavior when counter increment is corrupted
        #[cfg(feature = "failpoints")]
        fail_point!("limits::tick_command", |action| {
            match action.as_deref() {
                Some("skip_increment") => {
                    // Simulate counter not incrementing (potential bypass)
                    return Ok(());
                }
                Some("force_overflow") => {
                    // Simulate counter overflow
                    self.commands = usize::MAX;
                    return Err(LimitExceeded::MaxCommands(limits.max_commands));
                }
                Some("corrupt_high") => {
                    // Simulate counter corruption to a high value
                    self.commands = limits.max_commands + 1;
                }
                _ => {}
            }
            Ok(())
        });

        self.commands += 1;
        if self.commands > limits.max_commands {
            return Err(LimitExceeded::MaxCommands(limits.max_commands));
        }
        Ok(())
    }

    /// Increment loop iteration counter, returns error if limit exceeded
    pub fn tick_loop(&mut self, limits: &ExecutionLimits) -> Result<(), LimitExceeded> {
        // Fail point: test behavior when loop counter is corrupted
        #[cfg(feature = "failpoints")]
        fail_point!("limits::tick_loop", |action| {
            match action.as_deref() {
                Some("skip_check") => {
                    // Simulate limit check being bypassed
                    self.loop_iterations += 1;
                    return Ok(());
                }
                Some("reset_counter") => {
                    // Simulate counter being reset (infinite loop potential)
                    self.loop_iterations = 0;
                    return Ok(());
                }
                _ => {}
            }
            Ok(())
        });

        self.loop_iterations += 1;
        self.total_loop_iterations += 1;
        if self.loop_iterations > limits.max_loop_iterations {
            return Err(LimitExceeded::MaxLoopIterations(limits.max_loop_iterations));
        }
        // THREAT[TM-DOS-018]: Check global cap to prevent nested loop multiplication
        if self.total_loop_iterations > limits.max_total_loop_iterations {
            return Err(LimitExceeded::MaxTotalLoopIterations(
                limits.max_total_loop_iterations,
            ));
        }
        Ok(())
    }

    /// Reset loop iteration counter (called when entering a new loop)
    pub fn reset_loop(&mut self) {
        self.loop_iterations = 0;
    }

    /// Push function call, returns error if depth exceeded
    pub fn push_function(&mut self, limits: &ExecutionLimits) -> Result<(), LimitExceeded> {
        // Fail point: test behavior when function depth tracking fails
        #[cfg(feature = "failpoints")]
        fail_point!("limits::push_function", |action| {
            match action.as_deref() {
                Some("skip_check") => {
                    // Simulate depth check being bypassed (stack overflow potential)
                    self.function_depth += 1;
                    return Ok(());
                }
                Some("corrupt_depth") => {
                    // Simulate depth counter corruption
                    self.function_depth = 0;
                    return Ok(());
                }
                _ => {}
            }
            Ok(())
        });

        // Check before incrementing so we don't leave invalid state on failure
        if self.function_depth >= limits.max_function_depth {
            return Err(LimitExceeded::MaxFunctionDepth(limits.max_function_depth));
        }
        self.function_depth += 1;
        Ok(())
    }

    /// Pop function call
    pub fn pop_function(&mut self) {
        if self.function_depth > 0 {
            self.function_depth -= 1;
        }
    }
}

/// Error returned when a resource limit is exceeded
#[derive(Debug, Clone, thiserror::Error)]
pub enum LimitExceeded {
    #[error("maximum command count exceeded ({0})")]
    MaxCommands(usize),

    #[error("maximum loop iterations exceeded ({0})")]
    MaxLoopIterations(usize),

    #[error("maximum total loop iterations exceeded ({0})")]
    MaxTotalLoopIterations(usize),

    #[error("maximum function depth exceeded ({0})")]
    MaxFunctionDepth(usize),

    #[error("execution timeout ({0:?})")]
    Timeout(Duration),

    #[error("parser timeout ({0:?})")]
    ParserTimeout(Duration),

    #[error("input too large ({0} bytes, max {1} bytes)")]
    InputTooLarge(usize, usize),

    #[error("AST nesting too deep ({0} levels, max {1})")]
    AstTooDeep(usize, usize),

    #[error("parser fuel exhausted ({0} operations, max {1})")]
    ParserExhausted(usize, usize),
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_default_limits() {
        let limits = ExecutionLimits::default();
        assert_eq!(limits.max_commands, 10_000);
        assert_eq!(limits.max_loop_iterations, 10_000);
        assert_eq!(limits.max_total_loop_iterations, 1_000_000);
        assert_eq!(limits.max_function_depth, 100);
        assert_eq!(limits.timeout, Duration::from_secs(30));
        assert_eq!(limits.parser_timeout, Duration::from_secs(5));
        assert_eq!(limits.max_input_bytes, 10_000_000);
        assert_eq!(limits.max_ast_depth, 100);
        assert_eq!(limits.max_parser_operations, 100_000);
    }

    #[test]
    fn test_builder_pattern() {
        let limits = ExecutionLimits::new()
            .max_commands(100)
            .max_loop_iterations(50)
            .max_function_depth(10)
            .timeout(Duration::from_secs(5));

        assert_eq!(limits.max_commands, 100);
        assert_eq!(limits.max_loop_iterations, 50);
        assert_eq!(limits.max_function_depth, 10);
        assert_eq!(limits.timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_command_counter() {
        let limits = ExecutionLimits::new().max_commands(5);
        let mut counters = ExecutionCounters::new();

        for _ in 0..5 {
            assert!(counters.tick_command(&limits).is_ok());
        }

        // 6th command should fail
        assert!(matches!(
            counters.tick_command(&limits),
            Err(LimitExceeded::MaxCommands(5))
        ));
    }

    #[test]
    fn test_loop_counter() {
        let limits = ExecutionLimits::new().max_loop_iterations(3);
        let mut counters = ExecutionCounters::new();

        for _ in 0..3 {
            assert!(counters.tick_loop(&limits).is_ok());
        }

        // 4th iteration should fail
        assert!(matches!(
            counters.tick_loop(&limits),
            Err(LimitExceeded::MaxLoopIterations(3))
        ));

        // Reset and try again
        counters.reset_loop();
        assert!(counters.tick_loop(&limits).is_ok());
    }

    #[test]
    fn test_total_loop_counter_accumulates() {
        let limits = ExecutionLimits::new()
            .max_loop_iterations(5)
            .max_total_loop_iterations(8);
        let mut counters = ExecutionCounters::new();

        // First loop: 5 iterations (per-loop limit)
        for _ in 0..5 {
            assert!(counters.tick_loop(&limits).is_ok());
        }
        assert_eq!(counters.total_loop_iterations, 5);

        // Reset per-loop counter (entering new loop)
        counters.reset_loop();
        assert_eq!(counters.loop_iterations, 0);
        // total_loop_iterations should NOT reset
        assert_eq!(counters.total_loop_iterations, 5);

        // Second loop: should fail after 3 more (total = 8 cap)
        assert!(counters.tick_loop(&limits).is_ok()); // total=6
        assert!(counters.tick_loop(&limits).is_ok()); // total=7
        assert!(counters.tick_loop(&limits).is_ok()); // total=8

        // 9th total iteration should fail
        assert!(matches!(
            counters.tick_loop(&limits),
            Err(LimitExceeded::MaxTotalLoopIterations(8))
        ));
    }

    #[test]
    fn test_function_depth() {
        let limits = ExecutionLimits::new().max_function_depth(2);
        let mut counters = ExecutionCounters::new();

        assert!(counters.push_function(&limits).is_ok());
        assert!(counters.push_function(&limits).is_ok());

        // 3rd call should fail
        assert!(matches!(
            counters.push_function(&limits),
            Err(LimitExceeded::MaxFunctionDepth(2))
        ));

        // Pop and try again
        counters.pop_function();
        assert!(counters.push_function(&limits).is_ok());
    }

    #[test]
    fn test_reset_for_execution() {
        let limits = ExecutionLimits::new().max_commands(5);
        let mut counters = ExecutionCounters::new();

        // Exhaust command budget
        for _ in 0..5 {
            counters.tick_command(&limits).unwrap();
        }
        assert!(counters.tick_command(&limits).is_err());

        // Also accumulate some loop/function state
        counters.loop_iterations = 42;
        counters.total_loop_iterations = 999;
        counters.function_depth = 3;

        // Reset should restore all counters
        counters.reset_for_execution();
        assert_eq!(counters.commands, 0);
        assert_eq!(counters.loop_iterations, 0);
        assert_eq!(counters.total_loop_iterations, 0);
        assert_eq!(counters.function_depth, 0);

        // Should be able to tick commands again
        assert!(counters.tick_command(&limits).is_ok());
    }
}
