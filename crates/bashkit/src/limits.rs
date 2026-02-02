//! Resource limits for sandboxed execution
//!
//! These limits prevent runaway scripts from consuming excessive resources.
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

    /// Maximum function call depth (recursion limit)
    /// Default: 100
    pub max_function_depth: usize,

    /// Execution timeout
    /// Default: 30 seconds
    pub timeout: Duration,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_commands: 10_000,
            max_loop_iterations: 10_000,
            max_function_depth: 100,
            timeout: Duration::from_secs(30),
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

    /// Set maximum loop iterations
    pub fn max_loop_iterations(mut self, count: usize) -> Self {
        self.max_loop_iterations = count;
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
}

/// Execution counters for tracking resource usage
#[derive(Debug, Clone, Default)]
pub struct ExecutionCounters {
    /// Number of commands executed
    pub commands: usize,

    /// Current function call depth
    pub function_depth: usize,

    /// Number of iterations in current loop
    pub loop_iterations: usize,
}

impl ExecutionCounters {
    /// Create new counters
    pub fn new() -> Self {
        Self::default()
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
        if self.loop_iterations > limits.max_loop_iterations {
            return Err(LimitExceeded::MaxLoopIterations(limits.max_loop_iterations));
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

    #[error("maximum function depth exceeded ({0})")]
    MaxFunctionDepth(usize),

    #[error("execution timeout ({0:?})")]
    Timeout(Duration),
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
        assert_eq!(limits.max_function_depth, 100);
        assert_eq!(limits.timeout, Duration::from_secs(30));
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
}
