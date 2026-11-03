use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub memory_limit_bytes: usize,
    pub time_limit_ms: u64,
    pub max_stack_depth: usize,
    pub max_allocations: usize,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            memory_limit_bytes: 32 * 1024 * 1024, // 32MB
            time_limit_ms: 5000,                  // 5s
            max_stack_depth: 512,
            max_allocations: 100_000,
        }
    }
}

/// Tracks resource usage during execution.
#[derive(Debug, Default)]
pub struct ResourceTracker {
    pub allocations: usize,
    pub current_stack_depth: usize,
    pub peak_stack_depth: usize,
    start_time: Option<crate::time::Instant>,
}

impl ResourceTracker {
    pub fn start(&mut self) {
        self.start_time = Some(crate::time::Instant::now());
    }

    pub fn check_time(&self, limits: &ResourceLimits) -> crate::zapcode_browser::error::Result<()> {
        if let Some(start) = self.start_time {
            if start.elapsed().as_millis() as u64 > limits.time_limit_ms {
                return Err(crate::zapcode_browser::ZapcodeError::TimeLimitExceeded);
            }
        }
        Ok(())
    }

    pub fn check_stack(&self, limits: &ResourceLimits) -> crate::zapcode_browser::error::Result<()> {
        if self.current_stack_depth > limits.max_stack_depth {
            return Err(crate::zapcode_browser::ZapcodeError::StackOverflow(self.current_stack_depth));
        }
        Ok(())
    }

    pub fn track_allocation(&mut self, limits: &ResourceLimits) -> crate::zapcode_browser::error::Result<()> {
        self.allocations += 1;
        if self.allocations > limits.max_allocations {
            return Err(crate::zapcode_browser::ZapcodeError::AllocationLimitExceeded);
        }
        Ok(())
    }

    pub fn push_frame(&mut self) {
        self.current_stack_depth += 1;
        if self.current_stack_depth > self.peak_stack_depth {
            self.peak_stack_depth = self.current_stack_depth;
        }
    }

    pub fn pop_frame(&mut self) {
        self.current_stack_depth = self.current_stack_depth.saturating_sub(1);
    }
}
