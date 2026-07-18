//! Job table for background execution
//!
//! Tracks background jobs spawned with `&` and their exit status.
//! Background commands execute synchronously for deterministic output
//! ordering, but their results are stored here so `wait` and `$!` work
//! correctly.

use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::interpreter::ExecResult;

/// Job table for tracking background jobs.
///
/// Background commands run synchronously (for deterministic output ordering),
/// so by the time a job is registered its result is already fully computed. We
/// therefore store the finished [`ExecResult`] directly rather than a
/// `tokio::task::JoinHandle` — there is nothing left to await. This also keeps
/// the interpreter usable on `wasm32-unknown-unknown`, where `tokio::spawn`
/// panics (no reactor running).
pub struct JobTable {
    /// Active jobs indexed by ID
    jobs: BTreeMap<usize, ExecResult>,
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
            jobs: BTreeMap::new(),
            next_id: 1,
            last_job_id: None,
        }
    }

    /// Register a finished background job.
    ///
    /// Returns the job ID that can be used with wait.
    pub fn spawn(&mut self, result: ExecResult) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.jobs.insert(id, result);
        self.last_job_id = Some(id);
        id
    }

    /// Get the last spawned job ID (for $!)
    pub fn last_job_id(&self) -> Option<usize> {
        self.last_job_id
    }

    /// Wait for a specific job to complete
    pub async fn wait_for(&mut self, job_id: usize) -> Option<ExecResult> {
        self.jobs.remove(&job_id)
    }

    /// Wait for all jobs to complete
    ///
    /// Returns the exit code of the last job
    #[allow(dead_code)]
    pub async fn wait_all(&mut self) -> i32 {
        self.wait_all_results()
            .await
            .last()
            .map(|r| r.exit_code)
            .unwrap_or(0)
    }

    /// Wait for all jobs and return their results (preserving output).
    pub async fn wait_all_results(&mut self) -> Vec<ExecResult> {
        std::mem::take(&mut self.jobs).into_values().collect()
    }

    /// Check if there are any active jobs
    #[allow(dead_code)]
    pub fn has_jobs(&self) -> bool {
        !self.jobs.is_empty()
    }

    /// Get the number of active jobs
    #[allow(dead_code)]
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_and_wait() {
        let mut table = JobTable::new();

        // Register a finished job
        let job_id = table.spawn(ExecResult::ok("hello".to_string()));
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

        // Register multiple finished jobs
        for i in 0..3 {
            table.spawn(ExecResult::ok(format!("job {}", i)));
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
