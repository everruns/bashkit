//! Job table for background execution
//!
//! Tracks background jobs and their status.
//!
//! Note: This module provides infrastructure for background job tracking.
//! Currently, background commands run synchronously but the job table
//! is available for future async execution support.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::interpreter::ExecResult;

/// A background job
pub struct Job {
    /// Job ID (used with $!)
    pub id: usize,
    /// The task handle
    pub handle: JoinHandle<ExecResult>,
}

/// Job table for tracking background jobs
pub struct JobTable {
    /// Active jobs indexed by ID
    jobs: HashMap<usize, JoinHandle<ExecResult>>,
    /// Next job ID to assign
    next_id: usize,
    /// Last spawned job ID (for $!)
    last_job_id: Option<usize>,
}

impl Default for JobTable {
    fn default() -> Self {
        Self::new()
    }
}

impl JobTable {
    /// Create a new empty job table
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            next_id: 1,
            last_job_id: None,
        }
    }

    /// Spawn a new background job
    ///
    /// Returns the job ID that can be used with wait
    pub fn spawn(&mut self, handle: JoinHandle<ExecResult>) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.jobs.insert(id, handle);
        self.last_job_id = Some(id);
        id
    }

    /// Get the last spawned job ID (for $!)
    pub fn last_job_id(&self) -> Option<usize> {
        self.last_job_id
    }

    /// Wait for a specific job to complete
    pub async fn wait_for(&mut self, job_id: usize) -> Option<ExecResult> {
        if let Some(handle) = self.jobs.remove(&job_id) {
            match handle.await {
                Ok(result) => Some(result),
                Err(_) => Some(ExecResult::err("job panicked".to_string(), 1)),
            }
        } else {
            None
        }
    }

    /// Wait for all jobs to complete
    ///
    /// Returns the exit code of the last job
    pub async fn wait_all(&mut self) -> i32 {
        let mut last_exit_code = 0;

        // Drain all jobs
        let jobs: Vec<_> = self.jobs.drain().collect();

        for (_, handle) in jobs {
            match handle.await {
                Ok(result) => last_exit_code = result.exit_code,
                Err(_) => last_exit_code = 1,
            }
        }

        last_exit_code
    }

    /// Check if there are any active jobs
    pub fn has_jobs(&self) -> bool {
        !self.jobs.is_empty()
    }

    /// Get the number of active jobs
    pub fn job_count(&self) -> usize {
        self.jobs.len()
    }
}

/// Thread-safe wrapper around JobTable
pub type SharedJobTable = Arc<Mutex<JobTable>>;

/// Create a new shared job table
pub fn new_shared_job_table() -> SharedJobTable {
    Arc::new(Mutex::new(JobTable::new()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_and_wait() {
        let mut table = JobTable::new();

        // Spawn a simple job
        let handle = tokio::spawn(async { ExecResult::ok("hello".to_string()) });

        let job_id = table.spawn(handle);
        assert_eq!(job_id, 1);
        assert_eq!(table.last_job_id(), Some(1));

        // Wait for it
        let result = table.wait_for(job_id).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().exit_code, 0);
    }

    #[tokio::test]
    async fn test_wait_all() {
        let mut table = JobTable::new();

        // Spawn multiple jobs
        for i in 0..3 {
            let handle = tokio::spawn(async move { ExecResult::ok(format!("job {}", i)) });
            table.spawn(handle);
        }

        assert_eq!(table.job_count(), 3);

        let exit_code = table.wait_all().await;
        assert_eq!(exit_code, 0);
        assert!(!table.has_jobs());
    }

    #[tokio::test]
    async fn test_wait_for_nonexistent() {
        let mut table = JobTable::new();

        let result = table.wait_for(999).await;
        assert!(result.is_none());
    }
}
