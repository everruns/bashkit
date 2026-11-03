//! Concurrent pipeline producers use the existing session-wide job cap and
//! request storage leases before forking. Shared CoW state is not copied;
//! owned fork fields are measured conservatively, and stage ASTs are borrowed.
//! Keep this inventory aligned with `fork_for_job` when its ownership changes.

use super::*;
use crate::limits::{ExecutionBudget, ExecutionBudgetLease};
use std::mem::size_of;

pub(super) struct ForkStorage<'a> {
    budget: &'a ExecutionBudget,
    bytes: usize,
}

impl<'a> ForkStorage<'a> {
    fn new(budget: &'a ExecutionBudget) -> Self {
        Self { budget, bytes: 0 }
    }

    pub(super) fn add(&mut self, bytes: usize) -> Result<()> {
        self.budget.consume_work(1 + bytes.div_ceil(1024) as u64)?;
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| LimitExceeded::Memory("pipeline fork storage size overflow".into()))?;
        Ok(())
    }

    pub(super) fn slice<T>(&mut self, len: usize) -> Result<()> {
        self.add(
            len.checked_mul(size_of::<T>()).ok_or_else(|| {
                LimitExceeded::Memory("pipeline fork storage size overflow".into())
            })?,
        )
    }

    fn map<K, V>(&mut self, capacity: usize) -> Result<()> {
        // HashMap cloning retains the table capacity. Two buckets per usable
        // entry cover spare/control buckets, including small tables.
        self.slice::<(K, V)>(
            capacity.checked_mul(2).ok_or_else(|| {
                LimitExceeded::Memory("pipeline fork storage size overflow".into())
            })?,
        )?;
        self.add(capacity.saturating_mul(2).saturating_add(16))
    }

    fn strings(&mut self, strings: &[String]) -> Result<()> {
        self.slice::<String>(strings.len())?;
        for value in strings {
            self.add(value.len())?;
        }
        Ok(())
    }

    fn path(&mut self, path: &PathBuf) -> Result<()> {
        // Also conservative for platforms whose OsString uses wide units.
        self.add(path.capacity().saturating_mul(2))
    }
}

impl Interpreter {
    pub(super) fn pipeline_fork_storage(
        &self,
        stdin: Option<&crate::StreamData>,
    ) -> Result<ExecutionBudgetLease> {
        let mut storage = ForkStorage::new(&self.execution_budget);
        // Fixed heap owners created by the fork. Pipes hold their own lease. The future's
        // actual layout is leased separately before Box::pin.
        storage.add(
            size_of::<jobs::JobControl>()
                + size_of::<NoforkScope>()
                + size_of::<[usize; 8]>()
                + size_of::<Arc<pipe::Pipe>>(),
        )?;
        storage.add(self.proc_subs.fork_storage_bytes())?;
        if let Some(stdin) = stdin {
            storage.add(stdin.len())?;
            storage.add(stdin.text_lossy().len())?;
        }
        storage.path(&self.cwd)?;
        storage.add(self.last_command_name.len())?;
        storage.add(self.prompt_user.len())?;
        storage.add(self.prompt_host.len())?;
        storage.add(self.last_bg_pid.as_ref().map_or(0, String::len))?;
        storage.map::<String, Arc<dyn Builtin>>(self.builtins.capacity())?;
        for name in self.builtins.keys() {
            storage.add(name.len())?;
        }
        storage.slice::<CallFrame>(self.call_stack.len())?;
        for frame in &self.call_stack {
            storage.add(frame.name.len())?;
            storage.strings(&frame.positional)?;
            storage.map::<String, SavedVar>(frame.saved_vars.capacity())?;
            for (name, value) in &frame.saved_vars {
                storage.add(name.len())?;
                for value in [&value.value, &value.nameref, &value.env]
                    .into_iter()
                    .flatten()
                {
                    storage.add(value.len())?;
                }
            }
            storage.map::<String, Option<HashMap<usize, String>>>(frame.local_arrays.capacity())?;
            for (name, values) in &frame.local_arrays {
                storage.add(name.len())?;
                if let Some(values) = values {
                    storage.map::<usize, String>(values.capacity())?;
                    for value in values.values() {
                        storage.add(value.len())?;
                    }
                }
            }
            storage.map::<String, Option<HashMap<String, String>>>(
                frame.local_assoc_arrays.capacity(),
            )?;
            for (name, values) in &frame.local_assoc_arrays {
                storage.add(name.len())?;
                if let Some(values) = values {
                    storage.map::<String, String>(values.capacity())?;
                    for (name, value) in values {
                        storage.add(name.len())?;
                        storage.add(value.len())?;
                    }
                }
            }
        }
        storage.slice::<SourceFrame>(self.bash_source_stack.len())?;
        for frame in &self.bash_source_stack {
            storage.add(frame.funcname.len())?;
        }
        storage.slice::<usize>(self.counters.loop_iterations.len())?;
        storage.map::<i32, FdTarget>(self.exec_fd_table.capacity())?;
        for target in self.exec_fd_table.values() {
            storage.add(0)?;
            if let FdTarget::WriteFile(path, text) | FdTarget::AppendFile(path, text) = target {
                storage.path(path)?;
                storage.add(text.len())?;
            }
        }
        storage.map::<i32, ()>(self.exec_input_fds.capacity())?;
        storage.slice::<i32>(self.fd_redirect_scope.len())?;
        if let Some(completion) = &self.completion {
            storage.add(size_of::<completion::CompletionState>())?;
            completion.fork_storage(&mut storage)?;
        }
        self.execution_budget
            .lease_bytes(storage.bytes)
            .map_err(Into::into)
    }
}
