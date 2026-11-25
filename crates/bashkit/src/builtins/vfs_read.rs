//! THREAT[TM-DOS-150]: VFS reads reserve host storage before the backend materializes it.
//! Keep the lease through VM resume so the response is covered at handoff.

use std::sync::Mutex;

use crate::error::Result;
use crate::limits::{ExecutionBudget, ExecutionBudgetLease};

pub(super) struct VfsRead {
    limit: usize,
    budget: Option<ExecutionBudget>,
    storage: Mutex<(usize, Option<ExecutionBudgetLease>)>,
}

impl VfsRead {
    pub(super) fn new(limit: usize, budget: Option<ExecutionBudget>) -> Self {
        Self {
            limit,
            budget,
            storage: Mutex::new((0, None)),
        }
    }

    pub(super) fn reserve(&self, bytes: usize) -> Result<()> {
        let mut storage = self.storage.lock().unwrap_or_else(|e| e.into_inner());
        let required = storage.0.saturating_add(bytes);
        if required > self.limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                "VFS read exceeds the runtime memory limit",
            )
            .into());
        }
        if let Some(budget) = &self.budget {
            budget.consume_input(bytes)?;
            if let Some(lease) = &mut storage.1 {
                lease.grow(bytes)?;
            } else {
                storage.1 = Some(budget.lease_bytes(bytes)?);
            }
        }
        storage.0 = required;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_input_is_cumulative_and_live_storage_is_released() {
        use crate::limits::ExecutionLimits;
        use std::sync::{Arc, atomic::AtomicBool};

        let limits = ExecutionLimits::new()
            .max_aggregate_input_bytes(13)
            .max_live_intermediate_bytes(10);
        let budget = ExecutionBudget::new(&limits, Arc::new(AtomicBool::new(false)));
        {
            let read = VfsRead::new(100, Some(budget.clone()));
            read.reserve(4).unwrap();
            read.reserve(5).unwrap();
            assert_eq!(budget.live_bytes_for_test(), 9);
        }
        assert_eq!(budget.live_bytes_for_test(), 0);

        let read = VfsRead::new(100, Some(budget.clone()));
        read.reserve(4).unwrap();
        assert!(read.reserve(1).is_err());
        assert_eq!(budget.live_bytes_for_test(), 4);
        drop(read);
        assert_eq!(budget.live_bytes_for_test(), 0);
    }

    #[test]
    fn storage_reservations_are_cumulative() {
        let read = VfsRead::new(12, None);
        assert!(read.reserve(7).is_ok());
        assert!(read.reserve(6).is_err());
        assert!(read.reserve(5).is_ok());
        assert!(read.reserve(1).is_err());
    }
}
