use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::zapcode_browser::compiler::CompiledProgram;
use crate::zapcode_browser::error::{Result, ZapcodeError};
use crate::zapcode_browser::sandbox::ResourceLimits;
use crate::zapcode_browser::value::Value;
use crate::zapcode_browser::vm::{CallFrame, Continuation, TryInfo, Vm, VmState};

/// Internal serializable representation of VM state at a suspension point.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VmSnapshot {
    program: CompiledProgram,
    stack: Vec<Value>,
    frames: Vec<CallFrame>,
    /// User-defined globals only — builtins are re-registered on resume.
    globals: Vec<(String, Value)>,
    try_stack: Vec<TryInfo>,
    continuations: Vec<Continuation>,
    stdout: String,
    limits: ResourceLimits,
    external_functions: Vec<String>,
}

/// A snapshot of VM state at a suspension point.
/// Can be serialized to bytes and resumed later in any process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZapcodeSnapshot {
    data: Vec<u8>,
}

impl ZapcodeSnapshot {
    /// Capture the current VM state as a snapshot.
    pub(crate) fn capture(vm: &Vm) -> Result<Self> {
        // Filter out builtin globals — they'll be re-registered on resume.
        let builtin_names: HashSet<&str> = Vm::BUILTIN_GLOBAL_NAMES.iter().copied().collect();
        let user_globals: Vec<(String, Value)> = vm
            .globals
            .iter()
            .filter(|(k, _)| !builtin_names.contains(k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let snapshot = VmSnapshot {
            program: vm.program.clone(),
            stack: vm.stack.clone(),
            frames: vm.frames.clone(),
            globals: user_globals,
            try_stack: vm.try_stack.clone(),
            continuations: vm.continuations.clone(),
            stdout: vm.stdout.clone(),
            limits: vm.limits.clone(),
            external_functions: vm.external_functions.iter().cloned().collect(),
        };

        let data = postcard::to_allocvec(&snapshot)
            .map_err(|e| ZapcodeError::SnapshotError(format!("capture failed: {}", e)))?;

        Ok(Self { data })
    }

    /// Serialize the snapshot to bytes for storage / transport.
    pub fn dump(&self) -> Result<Vec<u8>> {
        postcard::to_allocvec(self)
            .map_err(|e| ZapcodeError::SnapshotError(format!("dump failed: {}", e)))
    }

    /// Deserialize a snapshot from bytes.
    pub fn load(bytes: &[u8]) -> Result<Self> {
        postcard::from_bytes(bytes)
            .map_err(|e| ZapcodeError::SnapshotError(format!("load failed: {}", e)))
    }

    /// Resume execution with a return value from the external function.
    /// Returns a `VmState` which may be `Complete` or another `Suspended`.
    pub fn resume(self, return_value: Value) -> Result<VmState> {
        let vm_snap: VmSnapshot = postcard::from_bytes(&self.data)
            .map_err(|e| ZapcodeError::SnapshotError(format!("resume decode failed: {}", e)))?;

        let user_globals: HashMap<String, Value> = vm_snap.globals.into_iter().collect();
        let ext_set: HashSet<String> = vm_snap.external_functions.into_iter().collect();

        let mut vm = Vm::from_snapshot(
            vm_snap.program,
            vm_snap.stack,
            vm_snap.frames,
            user_globals,
            vm_snap.try_stack,
            vm_snap.continuations,
            vm_snap.stdout,
            vm_snap.limits,
            ext_set,
        );

        // Push the return value onto the stack — this is the result the
        // `CallExternal` instruction was waiting for.
        vm.stack.push(return_value);

        vm.resume_execution()
    }
}
