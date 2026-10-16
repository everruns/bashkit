use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use indexmap::IndexMap;

use crate::zapcode_browser::compiler::instruction::{Constant, Instruction};
use crate::zapcode_browser::compiler::CompiledProgram;
use crate::zapcode_browser::error::{Result, ZapcodeError};
use crate::zapcode_browser::sandbox::{ResourceLimits, ResourceTracker};
use crate::zapcode_browser::snapshot::ZapcodeSnapshot;
use crate::zapcode_browser::trace::{ExecutionTrace, SpanBuilder, TraceStatus};
use crate::zapcode_browser::value::{Closure, FunctionId, GeneratorObject, SuspendedFrame, Value};

mod builtins;

/// The result of VM execution.
#[derive(Debug)]
pub enum VmState {
    Complete(Value),
    Suspended {
        function_name: String,
        args: Vec<Value>,
        snapshot: ZapcodeSnapshot,
    },
}

/// Tracks where a method receiver originated so that mutations to `this`
/// inside the method can be written back to the source variable.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum ReceiverSource {
    /// The receiver was loaded from a global variable with the given name.
    Global(String),
    /// The receiver was loaded from a local variable at the given slot index
    /// in the frame at the given depth (index into `self.frames`).
    Local { frame_index: usize, slot: usize },
}

/// A call frame in the VM stack.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CallFrame {
    pub(crate) func_index: Option<usize>,
    pub(crate) ip: usize,
    pub(crate) locals: Vec<Value>,
    pub(crate) stack_base: usize,
    /// The `this` value for method/constructor calls.
    pub(crate) this_value: Option<Value>,
    /// Where the method receiver came from, so we can write back mutations.
    pub(crate) receiver_source: Option<ReceiverSource>,
}

/// A continuation for array callback methods that may suspend (e.g., `.map()` with async callbacks).
/// Instead of running callbacks in a Rust for-loop (which can't be suspended), the continuation
/// tracks progress so the main `execute()` loop can drive iteration one callback at a time.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Continuation {
    /// Collecting `.map()` results element-by-element.
    ArrayMap {
        callback: Value,
        source: Vec<Value>,
        results: Vec<Value>,
        next_index: usize,
        /// Frame depth of the caller — the continuation fires when
        /// we return to this depth AND the callback's frame has been popped.
        caller_frame_depth: usize,
        /// The frame index of the currently-executing callback. Only when
        /// this specific frame is popped does the continuation advance.
        callback_frame_index: usize,
    },
    /// Collecting `.forEach()` calls element-by-element.
    ArrayForEach {
        callback: Value,
        source: Vec<Value>,
        next_index: usize,
        caller_frame_depth: usize,
        callback_frame_index: usize,
    },
}

/// The Zapcode VM.
pub struct Vm {
    pub(crate) program: CompiledProgram,
    pub(crate) stack: Vec<Value>,
    pub(crate) frames: Vec<CallFrame>,
    pub(crate) globals: HashMap<String, Value>,
    pub(crate) stdout: String,
    pub(crate) limits: ResourceLimits,
    pub(crate) tracker: ResourceTracker,
    pub(crate) external_functions: HashSet<String>,
    pub(crate) try_stack: Vec<TryInfo>,
    /// Active continuations for array callback methods that may suspend.
    pub(crate) continuations: Vec<Continuation>,
    /// The last object a property was accessed on — used for method dispatch.
    last_receiver: Option<Value>,
    /// Where the last receiver came from — used to write back `this` mutations.
    last_receiver_source: Option<ReceiverSource>,
    /// The name of the last global loaded — used to identify known globals.
    last_global_name: Option<String>,
    /// Tracks the source of the most recent Load instruction for receiver tracking.
    last_load_source: Option<ReceiverSource>,
    /// Counter for assigning unique generator IDs.
    next_generator_id: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct TryInfo {
    pub(crate) catch_ip: usize,
    pub(crate) frame_depth: usize,
    pub(crate) stack_depth: usize,
}

impl Vm {
    fn new(
        program: CompiledProgram,
        limits: ResourceLimits,
        external_functions: HashSet<String>,
    ) -> Self {
        let mut globals = HashMap::new();

        // Register built-in globals
        builtins::register_globals(&mut globals);

        Self {
            program,
            stack: Vec::new(),
            frames: Vec::new(),
            globals,
            stdout: String::new(),
            limits,
            tracker: ResourceTracker::default(),
            external_functions,
            try_stack: Vec::new(),
            continuations: Vec::new(),
            last_receiver: None,
            last_receiver_source: None,
            last_global_name: None,
            last_load_source: None,
            next_generator_id: 0,
        }
    }

    /// Names of all builtin globals registered by `register_globals`.
    pub(crate) const BUILTIN_GLOBAL_NAMES: &'static [&'static str] =
        &["console", "JSON", "Object", "Array", "Math", "Promise"];

    /// Restore a VM from snapshot state and continue execution.
    /// Builtins are re-registered after restoring user globals.
    /// The return_value is pushed onto the stack (result of the external call).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_snapshot(
        program: CompiledProgram,
        stack: Vec<Value>,
        frames: Vec<CallFrame>,
        user_globals: HashMap<String, Value>,
        try_stack: Vec<TryInfo>,
        continuations: Vec<Continuation>,
        stdout: String,
        limits: ResourceLimits,
        external_functions: HashSet<String>,
    ) -> Self {
        let mut globals = HashMap::new();
        // Re-register builtins first
        builtins::register_globals(&mut globals);
        // Then overlay user globals (user globals take precedence if names collide)
        for (k, v) in user_globals {
            globals.insert(k, v);
        }

        Self {
            program,
            stack,
            frames,
            globals,
            stdout,
            limits,
            tracker: ResourceTracker::default(),
            external_functions,
            try_stack,
            continuations,
            last_receiver: None,
            last_receiver_source: None,
            last_global_name: None,
            last_load_source: None,
            next_generator_id: 0,
        }
    }

    /// Resume execution after a snapshot restore. The return value from
    /// the external function should already be pushed onto the stack.
    pub(crate) fn resume_execution(&mut self) -> Result<VmState> {
        self.tracker.start();
        self.execute()
    }

    fn push(&mut self, value: Value) -> Result<()> {
        self.tracker.track_allocation(&self.limits)?;
        self.stack.push(value);
        Ok(())
    }

    fn pop(&mut self) -> Result<Value> {
        self.stack
            .pop()
            .ok_or_else(|| ZapcodeError::RuntimeError("stack underflow".to_string()))
    }

    fn peek(&self) -> Result<&Value> {
        self.stack
            .last()
            .ok_or_else(|| ZapcodeError::RuntimeError("stack underflow".to_string()))
    }

    fn current_frame(&self) -> &CallFrame {
        // Frames are always non-empty during execution (run() pushes the initial frame).
        // This is an internal invariant, not reachable by guest code.
        self.frames.last().expect("internal error: no active frame")
    }

    fn current_frame_mut(&mut self) -> &mut CallFrame {
        self.frames
            .last_mut()
            .expect("internal error: no active frame")
    }

    #[allow(dead_code)]
    fn instructions(&self) -> &[Instruction] {
        match self.current_frame().func_index {
            Some(idx) => &self.program.functions[idx].instructions,
            None => &self.program.instructions,
        }
    }

    /// Build the locals vec by binding `args` to the function's declared `params`.
    /// Handles positional, rest, and default-value patterns.
    fn bind_params(params: &[ParamPattern], args: &[Value], local_count: usize) -> Vec<Value> {
        let mut locals = Vec::with_capacity(local_count);
        for (i, param) in params.iter().enumerate() {
            match param {
                ParamPattern::Ident(_) => {
                    locals.push(args.get(i).cloned().unwrap_or(Value::Undefined));
                }
                ParamPattern::Rest(_) => {
                    let rest: Vec<Value> = args[i..].to_vec();
                    locals.push(Value::Array(rest));
                }
                ParamPattern::DefaultValue { .. } => {
                    let val = args.get(i).cloned().unwrap_or(Value::Undefined);
                    // Keep Undefined so the compiler-emitted default init can fire
                    locals.push(val);
                }
                _ => {
                    locals.push(args.get(i).cloned().unwrap_or(Value::Undefined));
                }
            }
        }
        locals
    }

    /// Common setup for calling a closure: inject captures, bind params, push frame.
    fn push_call_frame(
        &mut self,
        closure: &Closure,
        args: &[Value],
        this_value: Option<Value>,
    ) -> Result<()> {
        self.tracker.push_frame();
        self.tracker.check_stack(&self.limits)?;

        // Inject captured variables as globals
        for (name, val) in &closure.captured {
            if !self.globals.contains_key(name) {
                self.globals.insert(name.clone(), val.clone());
            }
        }

        let func = &self.program.functions[closure.func_id.0];
        let locals = Self::bind_params(&func.params, args, func.local_count);

        // If this is a method call (has this_value from a receiver), transfer
        // the receiver source so we can write back mutations on return.
        let receiver_source = if this_value.is_some() {
            self.last_receiver_source.take()
        } else {
            self.last_receiver_source = None;
            None
        };

        self.frames.push(CallFrame {
            func_index: Some(closure.func_id.0),
            ip: 0,
            locals,
            stack_base: self.stack.len(),
            this_value,
            receiver_source,
        });
        Ok(())
    }

    fn run(&mut self) -> Result<VmState> {
        self.tracker.start();

        // Set up top-level frame
        self.frames.push(CallFrame {
            func_index: None,
            ip: 0,
            locals: Vec::new(),
            stack_base: 0,
            this_value: None,
            receiver_source: None,
        });

        self.execute()
    }

    fn execute(&mut self) -> Result<VmState> {
        loop {
            // Resource checks
            self.tracker.check_time(&self.limits)?;

            let frame = self.frames.last().unwrap();
            let instructions = match frame.func_index {
                Some(idx) => &self.program.functions[idx].instructions,
                None => &self.program.instructions,
            };

            if frame.ip >= instructions.len() {
                // End of function/program
                if self.frames.len() <= 1 {
                    // Top-level: return last value on stack or undefined
                    let result = if self.stack.is_empty() {
                        Value::Undefined
                    } else {
                        self.stack.pop().unwrap_or(Value::Undefined)
                    };
                    return Ok(VmState::Complete(result));
                } else {
                    // Return from function
                    let frame = self.frames.pop().unwrap();
                    self.tracker.pop_frame();
                    // If this was a constructor, return `this`
                    if let Some(this_val) = frame.this_value {
                        self.stack.truncate(frame.stack_base);
                        self.push(this_val)?;
                    } else {
                        self.push(Value::Undefined)?;
                    }
                    // Check if a continuation callback just completed
                    self.process_continuation()?;
                    continue;
                }
            }

            let instr = instructions[frame.ip].clone();
            let result = self.dispatch(instr);

            match result {
                Ok(Some(state)) => return Ok(state),
                Ok(None) => {
                    // After dispatch, check if a continuation callback returned
                    // (via Return instruction or ip overflow)
                    if self.process_continuation()? {
                        continue;
                    }
                }
                Err(err) => {
                    // Try to catch the error
                    if let Some(try_info) = self.try_stack.pop() {
                        // Unwind to catch block
                        while self.frames.len() > try_info.frame_depth {
                            self.frames.pop();
                            self.tracker.pop_frame();
                        }
                        self.stack.truncate(try_info.stack_depth);

                        // Push error value
                        let error_val = Value::String(Arc::from(err.to_string()));
                        self.push(error_val)?;

                        // Jump to catch
                        self.current_frame_mut().ip = try_info.catch_ip;
                    } else {
                        return Err(err);
                    }
                }
            }
        }
    }

    /// Process the top continuation if the current frame depth indicates a callback
    /// has returned. Returns `true` if a continuation was processed (caller should
    /// `continue` the execute loop).
    fn process_continuation(&mut self) -> Result<bool> {
        let cont = match self.continuations.last() {
            Some(c) => c,
            None => return Ok(false),
        };

        // Check if the callback's specific frame has been popped — only then
        // has the callback returned. This avoids false triggers when inner
        // helper functions return to the same depth.
        let (callback_frame_index, caller_frame_depth) = match cont {
            Continuation::ArrayMap {
                callback_frame_index,
                caller_frame_depth,
                ..
            } => (*callback_frame_index, *caller_frame_depth),
            Continuation::ArrayForEach {
                callback_frame_index,
                caller_frame_depth,
                ..
            } => (*callback_frame_index, *caller_frame_depth),
        };

        // The callback frame is still active — not done yet
        if self.frames.len() > callback_frame_index {
            return Ok(false);
        }

        // Guard against stale continuations on stack unwinds — we must be
        // back at the original caller's frame depth.
        if self.frames.len() != caller_frame_depth {
            return Ok(false);
        }

        // The callback just returned — collect its result from the stack.
        // The compiler always emits PushUndefined+Return for implicit returns,
        // so an empty stack here indicates a VM bug.
        let callback_result = self.pop()?;

        // Unwrap internal promise values: async callbacks return
        // {__promise__: true, status: "resolved", value: X} or {status: "rejected", ...}.
        // Only unwrap objects with the __promise__ marker to avoid mangling user objects.
        let callback_result = if let Value::Object(ref map) = callback_result {
            if !matches!(map.get("__promise__"), Some(Value::Bool(true))) {
                // Not an internal promise — leave untouched
                callback_result
            } else {
                match map.get("status") {
                    Some(Value::String(s)) if s.as_ref() == "resolved" => {
                        map.get("value").cloned().unwrap_or(Value::Undefined)
                    }
                    Some(Value::String(s)) if s.as_ref() == "rejected" => {
                        let reason = map.get("reason").cloned().unwrap_or(Value::Undefined);
                        // Clean up the continuation before returning error
                        self.continuations.pop();
                        return Err(ZapcodeError::RuntimeError(format!(
                            "Unhandled promise rejection: {}",
                            reason.to_js_string()
                        )));
                    }
                    _ => callback_result,
                }
            }
        } else {
            callback_result
        };

        // Pop the continuation, take ownership to avoid cloning results
        let cont = self.continuations.pop().unwrap();

        match cont {
            Continuation::ArrayMap {
                callback,
                source,
                mut results,
                next_index,
                caller_frame_depth,
                ..
            } => {
                results.push(callback_result);
                let next = next_index + 1;

                if next < source.len() {
                    // Set up next callback call
                    let item = source[next].clone();
                    let closure = match &callback {
                        Value::Function(c) => c.clone(),
                        _ => unreachable!("callback validated at start"),
                    };
                    self.push_call_frame(&closure, &[item, Value::Int(next as i64)], None)?;
                    let new_frame_index = self.frames.len() - 1;
                    // Push updated continuation back
                    self.continuations.push(Continuation::ArrayMap {
                        callback,
                        source,
                        results,
                        next_index: next,
                        caller_frame_depth,
                        callback_frame_index: new_frame_index,
                    });
                    Ok(true)
                } else {
                    // All done — push final array, no clone needed
                    self.push(Value::Array(results))?;
                    Ok(true)
                }
            }
            Continuation::ArrayForEach {
                callback,
                source,
                next_index,
                caller_frame_depth,
                ..
            } => {
                let next = next_index + 1;

                if next < source.len() {
                    let item = source[next].clone();
                    let closure = match &callback {
                        Value::Function(c) => c.clone(),
                        _ => unreachable!("callback validated at start"),
                    };
                    self.push_call_frame(&closure, &[item, Value::Int(next as i64)], None)?;
                    let new_frame_index = self.frames.len() - 1;
                    self.continuations.push(Continuation::ArrayForEach {
                        callback,
                        source,
                        next_index: next,
                        caller_frame_depth,
                        callback_frame_index: new_frame_index,
                    });
                    Ok(true)
                } else {
                    self.push(Value::Undefined)?;
                    Ok(true)
                }
            }
        }
    }

    /// Call a function value with the given arguments and run it to completion.
    /// Returns the function's return value.
    fn call_function_internal(&mut self, callee: &Value, args: Vec<Value>) -> Result<Value> {
        let closure = match callee {
            Value::Function(c) => c.clone(),
            other => {
                return Err(ZapcodeError::TypeError(format!(
                    "{} is not a function",
                    other.to_js_string()
                )));
            }
        };

        let target_frame_depth = self.frames.len();
        self.push_call_frame(&closure, &args, None)?;

        // Run until the new frame returns
        loop {
            self.tracker.check_time(&self.limits)?;

            let frame = self.frames.last().unwrap();
            let instructions = match frame.func_index {
                Some(idx) => &self.program.functions[idx].instructions,
                None => &self.program.instructions,
            };

            if frame.ip >= instructions.len() {
                // End of function without explicit return
                if self.frames.len() > target_frame_depth + 1 {
                    // Inner function ended, pop and continue
                    self.frames.pop();
                    self.tracker.pop_frame();
                    self.push(Value::Undefined)?;
                    continue;
                } else {
                    // Our target function ended
                    self.frames.pop();
                    self.tracker.pop_frame();
                    return Ok(Value::Undefined);
                }
            }

            let instr = instructions[frame.ip].clone();
            let result = self.dispatch(instr);

            match result {
                Ok(Some(VmState::Complete(val))) => {
                    // A return happened that completed the top-level program.
                    // This shouldn't happen inside a callback but handle gracefully.
                    return Ok(val);
                }
                Ok(Some(VmState::Suspended { .. })) => {
                    return Err(ZapcodeError::RuntimeError(
                        "cannot suspend inside array callback".to_string(),
                    ));
                }
                Ok(None) => {
                    // Check if the frame was popped by a Return instruction
                    if self.frames.len() == target_frame_depth {
                        // The function returned; return value is on the stack
                        return Ok(self.pop().unwrap_or(Value::Undefined));
                    }
                }
                Err(err) => {
                    // Try to catch the error within the callback
                    if let Some(try_info) = self.try_stack.pop() {
                        while self.frames.len() > try_info.frame_depth {
                            self.frames.pop();
                            self.tracker.pop_frame();
                        }
                        self.stack.truncate(try_info.stack_depth);
                        let error_val = Value::String(Arc::from(err.to_string()));
                        self.push(error_val)?;
                        self.current_frame_mut().ip = try_info.catch_ip;
                    } else {
                        return Err(err);
                    }
                }
            }
        }
    }

    /// Call a callback for each array element. Passes (item, index) — the full
    /// array reference (3rd JS argument) is only built lazily if the callback
    /// actually uses 3+ params, avoiding O(n²) cloning.
    fn call_element_callback(
        &mut self,
        callback: &Value,
        item: &Value,
        index: usize,
    ) -> Result<Value> {
        self.call_function_internal(callback, vec![item.clone(), Value::Int(index as i64)])
    }

    /// Check if a callback value is an async function that might suspend.
    fn is_async_callback(&self, callback: &Value) -> bool {
        if let Value::Function(closure) = callback {
            if let Some(func) = self.program.functions.get(closure.func_id.0) {
                return func.is_async;
            }
        }
        false
    }

    /// Start a continuation-based `.map()` call: push the continuation and set up
    /// the first callback invocation. Returns `None` to signal that the main
    /// `execute()` loop should drive the iteration.
    fn start_continuation_map(
        &mut self,
        callback: Value,
        arr: Vec<Value>,
    ) -> Result<Option<Value>> {
        if arr.is_empty() {
            return Ok(Some(Value::Array(Vec::new())));
        }

        // Validate callback type BEFORE pushing continuation
        let closure = match &callback {
            Value::Function(c) => c.clone(),
            _ => {
                return Err(ZapcodeError::TypeError(
                    "map callback is not a function".to_string(),
                ))
            }
        };

        let caller_frame_depth = self.frames.len();
        let first_item = arr[0].clone();

        self.push_call_frame(&closure, &[first_item, Value::Int(0)], None)?;
        let callback_frame_index = self.frames.len() - 1;

        self.continuations.push(Continuation::ArrayMap {
            callback,
            source: arr,
            results: Vec::new(),
            next_index: 0,
            caller_frame_depth,
            callback_frame_index,
        });

        Ok(None) // Signal: continuation in progress
    }

    /// Start a continuation-based `.forEach()` call.
    fn start_continuation_foreach(
        &mut self,
        callback: Value,
        arr: Vec<Value>,
    ) -> Result<Option<Value>> {
        if arr.is_empty() {
            return Ok(Some(Value::Undefined));
        }

        // Validate callback type BEFORE pushing continuation
        let closure = match &callback {
            Value::Function(c) => c.clone(),
            _ => {
                return Err(ZapcodeError::TypeError(
                    "forEach callback is not a function".to_string(),
                ))
            }
        };

        let caller_frame_depth = self.frames.len();
        let first_item = arr[0].clone();

        self.push_call_frame(&closure, &[first_item, Value::Int(0)], None)?;
        let callback_frame_index = self.frames.len() - 1;

        self.continuations.push(Continuation::ArrayForEach {
            callback,
            source: arr,
            next_index: 0,
            caller_frame_depth,
            callback_frame_index,
        });

        Ok(None)
    }

    /// Execute an array callback method (map, filter, reduce, forEach, etc.)
    /// Returns `Ok(Some(value))` if the method completed synchronously, or
    /// `Ok(None)` if a continuation was started (async callback).
    fn execute_array_callback_method(
        &mut self,
        arr: Vec<Value>,
        method: &str,
        all_args: Vec<Value>,
    ) -> Result<Option<Value>> {
        let callback = all_args.first().cloned().unwrap_or(Value::Undefined);

        match method {
            "map" => {
                // Use continuation-based execution for async callbacks
                if self.is_async_callback(&callback) {
                    return self.start_continuation_map(callback, arr);
                }
                let mut result = Vec::with_capacity(arr.len());
                for (i, item) in arr.iter().enumerate() {
                    result.push(self.call_element_callback(&callback, item, i)?);
                }
                Ok(Some(Value::Array(result)))
            }
            "filter" | "find" | "findIndex" | "every" | "some" | "reduce" | "sort" | "flatMap" => {
                // Async callbacks are not supported for these methods
                if self.is_async_callback(&callback) {
                    return Err(ZapcodeError::RuntimeError(format!(
                        ".{}() does not support async callbacks — use .map() or a for-of loop instead",
                        method
                    )));
                }
                match method {
                    "filter" => {
                        let mut result = Vec::new();
                        for (i, item) in arr.iter().enumerate() {
                            if self.call_element_callback(&callback, item, i)?.is_truthy() {
                                result.push(item.clone());
                            }
                        }
                        Ok(Some(Value::Array(result)))
                    }
                    "find" => {
                        for (i, item) in arr.iter().enumerate() {
                            if self.call_element_callback(&callback, item, i)?.is_truthy() {
                                return Ok(Some(item.clone()));
                            }
                        }
                        Ok(Some(Value::Undefined))
                    }
                    "findIndex" => {
                        for (i, item) in arr.iter().enumerate() {
                            if self.call_element_callback(&callback, item, i)?.is_truthy() {
                                return Ok(Some(Value::Int(i as i64)));
                            }
                        }
                        Ok(Some(Value::Int(-1)))
                    }
                    "every" => {
                        for (i, item) in arr.iter().enumerate() {
                            if !self.call_element_callback(&callback, item, i)?.is_truthy() {
                                return Ok(Some(Value::Bool(false)));
                            }
                        }
                        Ok(Some(Value::Bool(true)))
                    }
                    "some" => {
                        for (i, item) in arr.iter().enumerate() {
                            if self.call_element_callback(&callback, item, i)?.is_truthy() {
                                return Ok(Some(Value::Bool(true)));
                            }
                        }
                        Ok(Some(Value::Bool(false)))
                    }
                    "reduce" => {
                        let mut acc = match all_args.get(1).cloned() {
                            Some(init) => Some(init),
                            None if !arr.is_empty() => Some(arr[0].clone()),
                            None => {
                                return Err(ZapcodeError::TypeError(
                                    "Reduce of empty array with no initial value".to_string(),
                                ));
                            }
                        };
                        let start = if all_args.get(1).is_some() { 0 } else { 1 };
                        for (i, item) in arr.iter().enumerate().skip(start) {
                            acc = Some(self.call_function_internal(
                                &callback,
                                vec![acc.unwrap(), item.clone(), Value::Int(i as i64)],
                            )?);
                        }
                        Ok(Some(acc.unwrap_or(Value::Undefined)))
                    }
                    "sort" => {
                        let mut result = arr;
                        if matches!(callback, Value::Function(_)) {
                            let len = result.len();
                            for i in 1..len {
                                let mut j = i;
                                while j > 0 {
                                    let cmp = self
                                        .call_function_internal(
                                            &callback,
                                            vec![result[j - 1].clone(), result[j].clone()],
                                        )?
                                        .to_number();
                                    if cmp > 0.0 {
                                        result.swap(j - 1, j);
                                        j -= 1;
                                    } else {
                                        break;
                                    }
                                }
                            }
                        } else {
                            result.sort_by_key(|a| a.to_js_string());
                        }
                        Ok(Some(Value::Array(result)))
                    }
                    "flatMap" => {
                        let mut result = Vec::new();
                        for (i, item) in arr.iter().enumerate() {
                            match self.call_element_callback(&callback, item, i)? {
                                Value::Array(inner) => result.extend(inner),
                                other => result.push(other),
                            }
                        }
                        Ok(Some(Value::Array(result)))
                    }
                    _ => unreachable!(),
                }
            }
            "forEach" => {
                // Use continuation-based execution for async callbacks
                if self.is_async_callback(&callback) {
                    return self.start_continuation_foreach(callback, arr);
                }
                for (i, item) in arr.iter().enumerate() {
                    self.call_element_callback(&callback, item, i)?;
                }
                Ok(Some(Value::Undefined))
            }
            _ => Err(ZapcodeError::TypeError(format!(
                "Unknown array callback method: {}",
                method
            ))),
        }
    }

    /// Execute .then(), .catch(), or .finally() on a resolved/rejected promise.
    /// Synchronously invokes the callback and returns a new promise wrapping the result.
    fn execute_promise_method(
        &mut self,
        promise: Value,
        method: &str,
        args: Vec<Value>,
    ) -> Result<Option<Value>> {
        let (status, value, reason) = if let Value::Object(ref map) = promise {
            let status = match map.get("status") {
                Some(Value::String(s)) => s.to_string(),
                _ => "pending".to_string(),
            };
            let value = map.get("value").cloned().unwrap_or(Value::Undefined);
            let reason = map.get("reason").cloned().unwrap_or(Value::Undefined);
            (status, value, reason)
        } else {
            return Ok(None);
        };

        let on_fulfilled = args.first().cloned().unwrap_or(Value::Undefined);
        let on_rejected = args.get(1).cloned().unwrap_or(Value::Undefined);

        match method {
            "then" => {
                if status == "resolved" {
                    if matches!(on_fulfilled, Value::Function(_)) {
                        let result = self.call_function_internal(&on_fulfilled, vec![value])?;
                        Ok(Some(builtins::make_resolved_promise(result)))
                    } else {
                        // No callback — pass through the promise
                        Ok(Some(promise))
                    }
                } else if status == "rejected" {
                    if matches!(on_rejected, Value::Function(_)) {
                        let result = self.call_function_internal(&on_rejected, vec![reason])?;
                        Ok(Some(builtins::make_resolved_promise(result)))
                    } else {
                        // No onRejected — pass through the rejection
                        Ok(Some(promise))
                    }
                } else {
                    Ok(Some(promise))
                }
            }
            "catch" => {
                if status == "rejected" {
                    let handler = args.first().cloned().unwrap_or(Value::Undefined);
                    if matches!(handler, Value::Function(_)) {
                        let result = self.call_function_internal(&handler, vec![reason])?;
                        Ok(Some(builtins::make_resolved_promise(result)))
                    } else {
                        Ok(Some(promise))
                    }
                } else {
                    // Resolved — pass through
                    Ok(Some(promise))
                }
            }
            "finally" => {
                let handler = args.first().cloned().unwrap_or(Value::Undefined);
                if matches!(handler, Value::Function(_)) {
                    // finally callback receives no arguments
                    self.call_function_internal(&handler, vec![])?;
                }
                // finally always passes through the original promise
                Ok(Some(promise))
            }
            _ => Ok(None),
        }
    }

    fn alloc_generator_id(&mut self) -> u64 {
        let id = self.next_generator_id;
        self.next_generator_id += 1;
        id
    }

    fn generator_next(&mut self, mut gen_obj: GeneratorObject, arg: Value) -> Result<Value> {
        if gen_obj.done {
            return Ok(self.make_iterator_result(Value::Undefined, true));
        }
        for (name, val) in &gen_obj.captured {
            if !self.globals.contains_key(name) {
                self.globals.insert(name.clone(), val.clone());
            }
        }
        let func_idx = gen_obj.func_id.0;
        match gen_obj.suspended.take() {
            None => {
                let func = &self.program.functions[func_idx];
                self.tracker.push_frame();
                let mut locals = Vec::with_capacity(func.local_count);
                for param in func.params.iter() {
                    match param {
                        ParamPattern::Ident(name) => {
                            let val = gen_obj
                                .captured
                                .iter()
                                .find(|(n, _)| n == name)
                                .map(|(_, v)| v.clone())
                                .unwrap_or(Value::Undefined);
                            locals.push(val);
                        }
                        ParamPattern::Rest(name) => {
                            let val = gen_obj
                                .captured
                                .iter()
                                .find(|(n, _)| n == name)
                                .map(|(_, v)| v.clone())
                                .unwrap_or(Value::Array(Vec::new()));
                            locals.push(val);
                        }
                        _ => {
                            locals.push(Value::Undefined);
                        }
                    }
                }
                let stack_base = self.stack.len();
                self.frames.push(CallFrame {
                    func_index: Some(func_idx),
                    ip: 0,
                    locals,
                    stack_base,
                    this_value: None,
                    receiver_source: None,
                });
                self.run_generator_until_yield_or_return(gen_obj)
            }
            Some(suspended) => {
                self.tracker.push_frame();
                let stack_base = self.stack.len();
                for val in &suspended.stack {
                    self.push(val.clone())?;
                }
                self.push(arg)?;
                self.frames.push(CallFrame {
                    func_index: Some(func_idx),
                    ip: suspended.ip,
                    locals: suspended.locals,
                    stack_base,
                    this_value: None,
                    receiver_source: None,
                });
                self.run_generator_until_yield_or_return(gen_obj)
            }
        }
    }

    /// Store generator state back into the globals registry.
    /// For done generators, the key is removed to prevent unbounded growth.
    fn store_generator(&mut self, gen_obj: GeneratorObject) {
        let gen_key = format!("__gen_{}", gen_obj.id);
        if gen_obj.done {
            self.globals.remove(&gen_key);
        } else {
            self.globals.insert(gen_key, Value::Generator(gen_obj));
        }
    }

    /// Mark a generator as done, store it, and return the final iterator result.
    fn finish_generator(&mut self, mut gen_obj: GeneratorObject, value: Value) -> Value {
        gen_obj.done = true;
        gen_obj.suspended = None;
        self.store_generator(gen_obj);
        self.make_iterator_result(value, true)
    }

    fn run_generator_until_yield_or_return(
        &mut self,
        mut gen_obj: GeneratorObject,
    ) -> Result<Value> {
        let target_frame_depth = self.frames.len() - 1;
        loop {
            self.tracker.check_time(&self.limits)?;
            let frame = self.frames.last().unwrap();
            let instructions = match frame.func_index {
                Some(idx) => &self.program.functions[idx].instructions,
                None => &self.program.instructions,
            };
            if frame.ip >= instructions.len() {
                if self.frames.len() > target_frame_depth + 1 {
                    let frame = self.frames.pop().unwrap();
                    self.tracker.pop_frame();
                    if let Some(this_val) = frame.this_value {
                        self.stack.truncate(frame.stack_base);
                        self.push(this_val)?;
                    } else {
                        self.push(Value::Undefined)?;
                    }
                    continue;
                }
                let frame = self.frames.pop().unwrap();
                self.tracker.pop_frame();
                self.stack.truncate(frame.stack_base);
                let result = self.finish_generator(gen_obj, Value::Undefined);
                return Ok(result);
            }
            let instr = instructions[frame.ip].clone();
            if matches!(instr, Instruction::Yield) {
                self.current_frame_mut().ip += 1;
                let yielded_value = self.pop()?;
                let frame = self.frames.pop().unwrap();
                self.tracker.pop_frame();
                let frame_stack: Vec<Value> = self.stack.drain(frame.stack_base..).collect();
                gen_obj.suspended = Some(SuspendedFrame {
                    ip: frame.ip,
                    locals: frame.locals,
                    stack: frame_stack,
                });
                gen_obj.done = false;
                self.store_generator(gen_obj);
                return Ok(self.make_iterator_result(yielded_value, false));
            }
            if matches!(instr, Instruction::Return) {
                self.current_frame_mut().ip += 1;
                let return_val = self.pop().unwrap_or(Value::Undefined);
                if self.frames.len() > target_frame_depth + 1 {
                    let frame = self.frames.pop().unwrap();
                    self.tracker.pop_frame();
                    self.stack.truncate(frame.stack_base);
                    self.push(return_val)?;
                    continue;
                }
                let frame = self.frames.pop().unwrap();
                self.tracker.pop_frame();
                self.stack.truncate(frame.stack_base);
                let result = self.finish_generator(gen_obj, return_val);
                return Ok(result);
            }
            let result = self.dispatch(instr);
            match result {
                Ok(Some(VmState::Complete(val))) => return Ok(val),
                Ok(Some(VmState::Suspended { .. })) => {
                    return Err(ZapcodeError::RuntimeError(
                        "cannot suspend inside a generator".to_string(),
                    ));
                }
                Ok(None) => {
                    if self.frames.len() == target_frame_depth {
                        let return_val = self.pop().unwrap_or(Value::Undefined);
                        let result = self.finish_generator(gen_obj, return_val);
                        return Ok(result);
                    }
                }
                Err(err) => {
                    if let Some(try_info) = self.try_stack.pop() {
                        while self.frames.len() > try_info.frame_depth {
                            self.frames.pop();
                            self.tracker.pop_frame();
                        }
                        self.stack.truncate(try_info.stack_depth);
                        let error_val = Value::String(Arc::from(err.to_string()));
                        self.push(error_val)?;
                        self.current_frame_mut().ip = try_info.catch_ip;
                    } else {
                        return Err(err);
                    }
                }
            }
        }
    }

    fn make_iterator_result(&self, value: Value, done: bool) -> Value {
        let mut obj = IndexMap::new();
        obj.insert(Arc::from("value"), value);
        obj.insert(Arc::from("done"), Value::Bool(done));
        Value::Object(obj)
    }

    fn dispatch(&mut self, instr: Instruction) -> Result<Option<VmState>> {
        self.current_frame_mut().ip += 1;

        match instr {
            Instruction::Push(constant) => {
                let value = match constant {
                    Constant::Undefined => Value::Undefined,
                    Constant::Null => Value::Null,
                    Constant::Bool(b) => Value::Bool(b),
                    Constant::Int(n) => Value::Int(n),
                    Constant::Float(n) => Value::Float(n),
                    Constant::String(s) => Value::String(Arc::from(s.as_str())),
                };
                self.push(value)?;
            }
            Instruction::Pop => {
                self.pop()?;
            }
            Instruction::Dup => {
                let val = self.peek()?.clone();
                self.push(val)?;
            }
            Instruction::LoadLocal(idx) => {
                let frame_index = self.frames.len() - 1;
                let frame = self.current_frame();
                let val = frame.locals.get(idx).cloned().unwrap_or(Value::Undefined);
                self.last_load_source = Some(ReceiverSource::Local {
                    frame_index,
                    slot: idx,
                });
                self.push(val)?;
            }
            Instruction::StoreLocal(idx) => {
                let val = self.pop()?;
                let frame = self.current_frame_mut();
                while frame.locals.len() <= idx {
                    frame.locals.push(Value::Undefined);
                }
                frame.locals[idx] = val;
            }
            Instruction::LoadGlobal(name) => {
                let val = self.globals.get(&name).cloned().unwrap_or(Value::Undefined);
                self.last_global_name = Some(name.clone());
                // Only track receiver source for user-defined globals — builtins
                // (console, Math, JSON, etc.) contain non-serializable BuiltinMethod
                // values that would break snapshot serialization if written back.
                if Self::BUILTIN_GLOBAL_NAMES.contains(&name.as_str()) {
                    self.last_load_source = None;
                } else {
                    self.last_load_source = Some(ReceiverSource::Global(name));
                }
                self.push(val)?;
            }
            Instruction::StoreGlobal(name) => {
                let val = self.pop()?;
                self.globals.insert(name, val);
            }
            Instruction::DeclareLocal(_) => {
                let frame = self.current_frame_mut();
                frame.locals.push(Value::Undefined);
            }

            // Arithmetic
            Instruction::Add => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => match a.checked_add(*b) {
                        Some(r) => Value::Int(r),
                        None => Value::Float(*a as f64 + *b as f64),
                    },
                    (Value::Float(a), Value::Float(b)) => Value::Float(a + b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(*a as f64 + b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a + *b as f64),
                    (Value::String(a), _) => {
                        let rhs = right.to_js_string();
                        let new_len = a.len().saturating_add(rhs.len());
                        if new_len > 10_000_000 {
                            return Err(ZapcodeError::AllocationLimitExceeded);
                        }
                        let mut s = a.to_string();
                        s.push_str(&rhs);
                        Value::String(Arc::from(s.as_str()))
                    }
                    (_, Value::String(b)) => {
                        let lhs = left.to_js_string();
                        let new_len = lhs.len().saturating_add(b.len());
                        if new_len > 10_000_000 {
                            return Err(ZapcodeError::AllocationLimitExceeded);
                        }
                        let mut s = lhs;
                        s.push_str(b);
                        Value::String(Arc::from(s.as_str()))
                    }
                    _ => Value::Float(left.to_number() + right.to_number()),
                };
                self.push(result)?;
            }
            Instruction::Sub => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => match a.checked_sub(*b) {
                        Some(r) => Value::Int(r),
                        None => Value::Float(*a as f64 - *b as f64),
                    },
                    _ => Value::Float(left.to_number() - right.to_number()),
                };
                self.push(result)?;
            }
            Instruction::Mul => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) => match a.checked_mul(*b) {
                        Some(r) => Value::Int(r),
                        None => Value::Float(*a as f64 * *b as f64),
                    },
                    _ => Value::Float(left.to_number() * right.to_number()),
                };
                self.push(result)?;
            }
            Instruction::Div => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = Value::Float(left.to_number() / right.to_number());
                self.push(result)?;
            }
            Instruction::Rem => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = match (&left, &right) {
                    (Value::Int(a), Value::Int(b)) if *b != 0 => Value::Int(a % b),
                    _ => Value::Float(left.to_number() % right.to_number()),
                };
                self.push(result)?;
            }
            Instruction::Pow => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = Value::Float(left.to_number().powf(right.to_number()));
                self.push(result)?;
            }
            Instruction::Neg => {
                let val = self.pop()?;
                let result = match val {
                    Value::Int(n) => Value::Int(-n),
                    _ => Value::Float(-val.to_number()),
                };
                self.push(result)?;
            }
            Instruction::BitNot => {
                let val = self.pop()?;
                let n = val.to_number() as i32;
                self.push(Value::Int(!n as i64))?;
            }
            Instruction::BitAnd => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = (left.to_number() as i32) & (right.to_number() as i32);
                self.push(Value::Int(result as i64))?;
            }
            Instruction::BitOr => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = (left.to_number() as i32) | (right.to_number() as i32);
                self.push(Value::Int(result as i64))?;
            }
            Instruction::BitXor => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = (left.to_number() as i32) ^ (right.to_number() as i32);
                self.push(Value::Int(result as i64))?;
            }
            Instruction::Shl => {
                let right = self.pop()?;
                let left = self.pop()?;
                let shift = (right.to_number() as u32) & 0x1f;
                let result = (left.to_number() as i32) << shift;
                self.push(Value::Int(result as i64))?;
            }
            Instruction::Shr => {
                let right = self.pop()?;
                let left = self.pop()?;
                let shift = (right.to_number() as u32) & 0x1f;
                let result = (left.to_number() as i32) >> shift;
                self.push(Value::Int(result as i64))?;
            }
            Instruction::Ushr => {
                let right = self.pop()?;
                let left = self.pop()?;
                let shift = (right.to_number() as u32) & 0x1f;
                let result = (left.to_number() as u32) >> shift;
                self.push(Value::Int(result as i64))?;
            }

            // Comparison
            Instruction::Eq | Instruction::StrictEq => {
                let right = self.pop()?;
                let left = self.pop()?;
                self.push(Value::Bool(left.strict_eq(&right)))?;
            }
            Instruction::Neq | Instruction::StrictNeq => {
                let right = self.pop()?;
                let left = self.pop()?;
                self.push(Value::Bool(!left.strict_eq(&right)))?;
            }
            Instruction::Lt => {
                let right = self.pop()?;
                let left = self.pop()?;
                self.push(Value::Bool(left.to_number() < right.to_number()))?;
            }
            Instruction::Lte => {
                let right = self.pop()?;
                let left = self.pop()?;
                self.push(Value::Bool(left.to_number() <= right.to_number()))?;
            }
            Instruction::Gt => {
                let right = self.pop()?;
                let left = self.pop()?;
                self.push(Value::Bool(left.to_number() > right.to_number()))?;
            }
            Instruction::Gte => {
                let right = self.pop()?;
                let left = self.pop()?;
                self.push(Value::Bool(left.to_number() >= right.to_number()))?;
            }

            // Logical
            Instruction::Not => {
                let val = self.pop()?;
                self.push(Value::Bool(!val.is_truthy()))?;
            }

            // Objects & Arrays
            Instruction::CreateArray(count) => {
                self.tracker.track_allocation(&self.limits)?;
                let mut arr = Vec::with_capacity(count);
                for _ in 0..count {
                    arr.push(self.pop()?);
                }
                arr.reverse();
                self.push(Value::Array(arr))?;
            }
            Instruction::CreateObject(count) => {
                self.tracker.track_allocation(&self.limits)?;
                let mut obj = IndexMap::new();
                // Pop key-value pairs (or spread values)
                let mut entries = Vec::new();
                for _ in 0..count {
                    let val = self.pop()?;
                    let key = self.pop()?;
                    entries.push((key, val));
                }
                entries.reverse();
                for (key, val) in entries {
                    match key {
                        Value::String(k) => {
                            obj.insert(k, val);
                        }
                        _ => {
                            let k: Arc<str> = Arc::from(key.to_js_string().as_str());
                            obj.insert(k, val);
                        }
                    }
                }
                self.push(Value::Object(obj))?;
            }
            Instruction::GetProperty(name) => {
                let obj = self.pop()?;
                let result = self.get_property(&obj, &name)?;
                // Store receiver for method calls
                if matches!(result, Value::BuiltinMethod { .. } | Value::Function(_)) {
                    self.last_receiver = Some(obj);
                    self.last_receiver_source = self.last_load_source.take();
                } else {
                    self.last_receiver_source = None;
                }
                self.push(result)?;
            }
            Instruction::SetProperty(name) => {
                // Stack: [value_to_store, object] with object on top
                // (compile_store pushes object after the value)
                let obj_val = self.pop()?;
                let value = self.pop()?;
                match obj_val {
                    Value::Object(mut obj) => {
                        obj.insert(Arc::from(name.as_str()), value);
                        // Push modified object back so compile_store can store it
                        self.push(Value::Object(obj))?;
                    }
                    _ => {
                        return Err(ZapcodeError::TypeError(format!(
                            "cannot set property '{}' on {}",
                            name,
                            obj_val.type_name()
                        )));
                    }
                }
            }
            Instruction::GetIndex => {
                let index = self.pop()?;
                let obj = self.pop()?;
                let result = match (&obj, &index) {
                    (Value::Array(arr), Value::Int(i)) => {
                        arr.get(*i as usize).cloned().unwrap_or(Value::Undefined)
                    }
                    (Value::Array(arr), Value::Float(f)) => {
                        arr.get(*f as usize).cloned().unwrap_or(Value::Undefined)
                    }
                    (Value::Object(map), Value::String(key)) => {
                        map.get(key.as_ref()).cloned().unwrap_or(Value::Undefined)
                    }
                    (Value::Object(map), _) => {
                        let key: Arc<str> = Arc::from(index.to_js_string().as_str());
                        map.get(key.as_ref()).cloned().unwrap_or(Value::Undefined)
                    }
                    (Value::String(s), Value::Int(i)) => s
                        .chars()
                        .nth(*i as usize)
                        .map(|c| Value::String(Arc::from(c.to_string().as_str())))
                        .unwrap_or(Value::Undefined),
                    _ => Value::Undefined,
                };
                self.push(result)?;
            }
            Instruction::SetIndex => {
                let index = self.pop()?;
                let mut obj = self.pop()?;
                let value = self.pop()?;
                match &mut obj {
                    Value::Array(arr) => {
                        let idx = match &index {
                            Value::Int(i) if *i >= 0 => *i as usize,
                            Value::Float(f) if *f >= 0.0 && *f == (*f as usize as f64) => {
                                *f as usize
                            }
                            _ => {
                                // Negative or non-numeric index: treat as no-op (like JS)
                                self.push(obj)?;
                                return Ok(None);
                            }
                        };
                        // Cap maximum sparse array growth to prevent memory exhaustion
                        if idx > arr.len() + 1024 {
                            return Err(ZapcodeError::RuntimeError(format!(
                                "array index {} too far beyond length {}",
                                idx,
                                arr.len()
                            )));
                        }
                        while arr.len() <= idx {
                            arr.push(Value::Undefined);
                        }
                        arr[idx] = value;
                    }
                    Value::Object(map) => {
                        let key: Arc<str> = Arc::from(index.to_js_string().as_str());
                        map.insert(key, value);
                    }
                    _ => {}
                }
                // Push modified object back so compile_store can store it to the variable
                self.push(obj)?;
            }
            Instruction::Spread => {
                // Handled contextually in CreateArray/CreateObject
            }
            Instruction::In => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = match &right {
                    Value::Object(map) => {
                        let key = left.to_js_string();
                        map.contains_key(key.as_str())
                    }
                    Value::Array(arr) => {
                        if let Value::Int(i) = left {
                            (i as usize) < arr.len()
                        } else {
                            false
                        }
                    }
                    _ => false,
                };
                self.push(Value::Bool(result))?;
            }
            Instruction::InstanceOf => {
                let right = self.pop()?;
                let left = self.pop()?;
                // Check if left's __class__ matches right's __class_name__
                let result = match (&left, &right) {
                    (Value::Object(instance), Value::Object(class_obj)) => {
                        if let (Some(inst_class), Some(class_name)) =
                            (instance.get("__class__"), class_obj.get("__class_name__"))
                        {
                            inst_class == class_name
                        } else {
                            false
                        }
                    }
                    _ => false,
                };
                self.push(Value::Bool(result))?;
            }

            // Functions
            Instruction::CreateClosure(func_idx) => {
                // Capture current scope for closure
                let mut captured = Vec::new();
                // Capture all locals from all active frames using local_names
                for frame in &self.frames {
                    let local_names = if let Some(fidx) = frame.func_index {
                        &self.program.functions[fidx].local_names
                    } else {
                        &self.program.local_names
                    };
                    for (i, val) in frame.locals.iter().enumerate() {
                        if let Some(name) = local_names.get(i) {
                            captured.push((name.clone(), val.clone()));
                        }
                    }
                }
                // Also capture all globals that are user-defined (not builtins)
                let builtins = ["console", "Math", "JSON", "Object", "Array"];
                for (name, val) in &self.globals {
                    if !builtins.contains(&name.as_str()) {
                        captured.push((name.clone(), val.clone()));
                    }
                }
                let closure = Closure {
                    func_id: FunctionId(func_idx),
                    captured,
                };
                self.push(Value::Function(closure))?;
            }
            Instruction::Call(arg_count) => {
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.pop()?);
                }
                args.reverse();

                let callee = self.pop()?;
                match callee {
                    Value::Function(closure) => {
                        let func_idx = closure.func_id.0;
                        let is_generator = self.program.functions[func_idx].is_generator;

                        // Generator function: create a Generator object instead of running
                        if is_generator {
                            let params = self.program.functions[func_idx].params.clone();
                            let gen_id = self.alloc_generator_id();
                            // Capture args as named params so generator_next can restore them
                            let mut captured = closure.captured.clone();
                            for (i, param) in params.iter().enumerate() {
                                match param {
                                    ParamPattern::Ident(name) => {
                                        captured.push((
                                            name.clone(),
                                            args.get(i).cloned().unwrap_or(Value::Undefined),
                                        ));
                                    }
                                    ParamPattern::Rest(name) => {
                                        let rest: Vec<Value> = args[i..].to_vec();
                                        captured.push((name.clone(), Value::Array(rest)));
                                    }
                                    _ => {}
                                }
                            }
                            let gen_obj = GeneratorObject {
                                id: gen_id,
                                func_id: closure.func_id,
                                captured,
                                suspended: None,
                                done: false,
                            };
                            // Store in globals registry so we can look it up by ID later
                            self.globals.insert(
                                format!("__gen_{}", gen_id),
                                Value::Generator(gen_obj.clone()),
                            );
                            self.push(Value::Generator(gen_obj))?;
                            self.last_receiver = None;
                            self.last_receiver_source = None;
                        } else {
                            let this_value = self.last_receiver.take();
                            self.push_call_frame(&closure, &args, this_value)?;
                        }
                    }
                    Value::BuiltinMethod {
                        object_name,
                        method_name,
                    } => {
                        let receiver = self.last_receiver.take();
                        let result = match object_name.as_ref() {
                            "__array__" => {
                                if let Some(Value::Array(arr)) = &receiver {
                                    // Check if this is a callback method first
                                    match method_name.as_ref() {
                                        "map" | "filter" | "forEach" | "find" | "findIndex"
                                        | "every" | "some" | "reduce" | "sort" | "flatMap" => {
                                            match self.execute_array_callback_method(
                                                arr.clone(),
                                                &method_name,
                                                args,
                                            )? {
                                                Some(val) => Some(val),
                                                None => {
                                                    // Continuation started — the main execute()
                                                    // loop will drive the callbacks. Don't push
                                                    // a result; just return Ok(None).
                                                    return Ok(None);
                                                }
                                            }
                                        }
                                        _ => builtins::call_builtin(
                                            &Value::Array(arr.clone()),
                                            &method_name,
                                            &args,
                                            &mut self.stdout,
                                        )?,
                                    }
                                } else {
                                    None
                                }
                            }
                            "__string__" => {
                                if let Some(Value::String(s)) = &receiver {
                                    builtins::call_builtin(
                                        &Value::String(s.clone()),
                                        &method_name,
                                        &args,
                                        &mut self.stdout,
                                    )?
                                } else {
                                    None
                                }
                            }
                            "__generator__" => {
                                if let Some(Value::Generator(gen_obj)) = receiver {
                                    match method_name.as_ref() {
                                        "next" => {
                                            let arg =
                                                args.into_iter().next().unwrap_or(Value::Undefined);
                                            // Get the latest generator state from registry.
                                            // If the key is missing, the generator has finished
                                            // and was cleaned up — return done immediately.
                                            let gen_key = format!("__gen_{}", gen_obj.id);
                                            if let Some(Value::Generator(g)) =
                                                self.globals.remove(&gen_key)
                                            {
                                                let result = self.generator_next(g, arg)?;
                                                Some(result)
                                            } else {
                                                Some(
                                                    self.make_iterator_result(
                                                        Value::Undefined,
                                                        true,
                                                    ),
                                                )
                                            }
                                        }
                                        "return" => {
                                            let val =
                                                args.into_iter().next().unwrap_or(Value::Undefined);
                                            let gen_key = format!("__gen_{}", gen_obj.id);
                                            if let Some(Value::Generator(g)) =
                                                self.globals.remove(&gen_key)
                                            {
                                                let result = self.finish_generator(g, val);
                                                Some(result)
                                            } else {
                                                Some(self.make_iterator_result(val, true))
                                            }
                                        }
                                        _ => None,
                                    }
                                } else {
                                    None
                                }
                            }
                            "__promise__" => {
                                if let Some(promise) = receiver {
                                    self.execute_promise_method(promise, &method_name, args)?
                                } else {
                                    None
                                }
                            }
                            global_name => builtins::call_global_method(
                                global_name,
                                &method_name,
                                &args,
                                &mut self.stdout,
                            )?,
                        };
                        match result {
                            Some(val) => self.push(val)?,
                            None => {
                                return Err(ZapcodeError::TypeError(format!(
                                    "{}.{} is not a function",
                                    object_name, method_name
                                )));
                            }
                        }
                    }
                    _ => {
                        return Err(ZapcodeError::TypeError(format!(
                            "{} is not a function",
                            callee.to_js_string()
                        )));
                    }
                }
            }
            Instruction::Return => {
                let return_val = self.pop().unwrap_or(Value::Undefined);

                if self.frames.len() <= 1 {
                    return Ok(Some(VmState::Complete(return_val)));
                }

                let frame = self.frames.pop().unwrap();
                self.tracker.pop_frame();

                // If this was a constructor frame (has this_value), return the
                // updated `this` instead of the explicit return value (unless
                // the constructor explicitly returns an object).
                let actual_return = if let Some(ref this_val) = frame.this_value {
                    // Also propagate this back to parent frame (for super() calls)
                    if let Some(parent) = self.frames.last_mut() {
                        if parent.this_value.is_some() {
                            parent.this_value = Some(this_val.clone());
                        }
                    }
                    // Write back the mutated `this` to the original variable
                    // that the method receiver came from. This ensures that
                    // value-type semantics work correctly for method calls
                    // that mutate `this` properties (e.g., this.count += 1).
                    if let Some(ref source) = frame.receiver_source {
                        match source {
                            ReceiverSource::Global(name) => {
                                self.globals.insert(name.clone(), this_val.clone());
                            }
                            ReceiverSource::Local { frame_index, slot } => {
                                if let Some(target_frame) = self.frames.get_mut(*frame_index) {
                                    while target_frame.locals.len() <= *slot {
                                        target_frame.locals.push(Value::Undefined);
                                    }
                                    target_frame.locals[*slot] = this_val.clone();
                                }
                            }
                        }
                    }
                    if matches!(return_val, Value::Undefined) {
                        this_val.clone()
                    } else {
                        return_val
                    }
                } else {
                    return_val
                };

                self.stack.truncate(frame.stack_base);
                self.push(actual_return)?;
            }
            Instruction::CallExternal(name, arg_count) => {
                if !self.external_functions.contains(&name) {
                    return Err(ZapcodeError::UnknownExternalFunction(name));
                }
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.pop()?);
                }
                args.reverse();
                // Suspend execution
                let snapshot = ZapcodeSnapshot::capture(self)?;
                return Ok(Some(VmState::Suspended {
                    function_name: name,
                    args,
                    snapshot,
                }));
            }

            // Control flow
            Instruction::Jump(target) => {
                self.current_frame_mut().ip = target;
            }
            Instruction::JumpIfFalse(target) => {
                let val = self.pop()?;
                if !val.is_truthy() {
                    self.current_frame_mut().ip = target;
                }
            }
            Instruction::JumpIfTrue(target) => {
                let val = self.pop()?;
                if val.is_truthy() {
                    self.current_frame_mut().ip = target;
                }
            }
            Instruction::JumpIfNullish(target) => {
                let val = self.peek()?;
                if matches!(val, Value::Null | Value::Undefined) {
                    self.current_frame_mut().ip = target;
                }
            }

            // Loops
            Instruction::SetupLoop => {}
            Instruction::Break | Instruction::Continue => {
                // These should have been compiled to jumps
            }

            // Iterators
            Instruction::GetIterator => {
                let val = self.pop()?;
                match val {
                    Value::Array(arr) => {
                        // Push an iterator object: [array, index]
                        let iter_obj = Value::Array(vec![Value::Array(arr), Value::Int(0)]);
                        self.push(iter_obj)?;
                    }
                    Value::String(s) => {
                        let chars: Vec<Value> = s
                            .chars()
                            .map(|c| Value::String(Arc::from(c.to_string().as_str())))
                            .collect();
                        let iter_obj = Value::Array(vec![Value::Array(chars), Value::Int(0)]);
                        self.push(iter_obj)?;
                    }
                    Value::Generator(gen_obj) => {
                        let iter_obj = Value::Array(vec![
                            Value::String(Arc::from("__gen__")),
                            Value::Int(gen_obj.id as i64),
                            Value::Bool(false),
                        ]);
                        self.push(iter_obj)?;
                    }
                    _ => {
                        return Err(ZapcodeError::TypeError(format!(
                            "{} is not iterable",
                            val.type_name()
                        )));
                    }
                }
            }
            Instruction::IteratorNext => {
                let iter = self.pop()?;
                // Check for generator iterator (3-element sentinel)
                if let Value::Array(ref items) = iter {
                    if items.len() == 3 {
                        if let Value::String(ref s) = items[0] {
                            if s.as_ref() == "__gen__" {
                                let gen_id = match &items[1] {
                                    Value::Int(id) => *id as u64,
                                    _ => {
                                        return Err(ZapcodeError::RuntimeError(
                                            "bad gen iter".into(),
                                        ))
                                    }
                                };
                                let gen_key = format!("__gen_{}", gen_id);
                                let gen_obj = if let Some(Value::Generator(g)) =
                                    self.globals.remove(&gen_key)
                                {
                                    g
                                } else {
                                    self.push(Value::Array(vec![
                                        Value::String(Arc::from("__gen__")),
                                        Value::Int(gen_id as i64),
                                        Value::Bool(true),
                                    ]))?;
                                    self.push(Value::Undefined)?;
                                    return Ok(None);
                                };
                                let result = self.generator_next(gen_obj, Value::Undefined)?;
                                if let Value::Object(ref obj) = result {
                                    let done = obj
                                        .get("done")
                                        .is_some_and(|v| matches!(v, Value::Bool(true)));
                                    let value =
                                        obj.get("value").cloned().unwrap_or(Value::Undefined);
                                    self.push(Value::Array(vec![
                                        Value::String(Arc::from("__gen__")),
                                        Value::Int(gen_id as i64),
                                        Value::Bool(done),
                                    ]))?;
                                    self.push(value)?;
                                } else {
                                    self.push(iter)?;
                                    self.push(Value::Undefined)?;
                                }
                                return Ok(None);
                            }
                        }
                    }
                }
                match iter {
                    Value::Array(ref items) if items.len() == 2 => {
                        let arr = match &items[0] {
                            Value::Array(a) => a,
                            _ => return Err(ZapcodeError::RuntimeError("invalid iterator".into())),
                        };
                        let idx = match &items[1] {
                            Value::Int(i) => *i as usize,
                            _ => return Err(ZapcodeError::RuntimeError("invalid iterator".into())),
                        };
                        if idx < arr.len() {
                            let value = arr[idx].clone();
                            // Update iterator
                            let new_iter =
                                Value::Array(vec![items[0].clone(), Value::Int((idx + 1) as i64)]);
                            // Push updated iterator back, then the value
                            self.push(new_iter)?;
                            self.push(value)?;
                        } else {
                            // Done — increment index past the end so IteratorDone sees idx > len
                            let new_iter =
                                Value::Array(vec![items[0].clone(), Value::Int((idx + 1) as i64)]);
                            self.push(new_iter)?;
                            self.push(Value::Undefined)?;
                        }
                    }
                    _ => {
                        return Err(ZapcodeError::RuntimeError("invalid iterator state".into()));
                    }
                }
            }
            Instruction::IteratorDone => {
                let value = self.pop()?;
                let iter = self.peek()?;
                // Check for generator iterator first
                if let Value::Array(items) = iter {
                    if items.len() == 3 {
                        if let Value::String(ref s) = items[0] {
                            if s.as_ref() == "__gen__" {
                                let done = matches!(&items[2], Value::Bool(true));
                                if !done {
                                    self.push(value)?;
                                }
                                self.push(Value::Bool(done))?;
                                return Ok(None);
                            }
                        }
                    }
                }
                let iter = self.peek()?;
                match iter {
                    Value::Array(items) if items.len() == 2 => {
                        let arr = match &items[0] {
                            Value::Array(a) => a,
                            _ => {
                                self.push(value)?;
                                self.push(Value::Bool(true))?;
                                return Ok(None);
                            }
                        };
                        let idx = match &items[1] {
                            Value::Int(i) => *i as usize,
                            _ => {
                                self.push(value)?;
                                self.push(Value::Bool(true))?;
                                return Ok(None);
                            }
                        };
                        let done = idx > arr.len();
                        if !done {
                            // Push value back for the binding
                            self.push(value)?;
                        }
                        self.push(Value::Bool(done))?;
                    }
                    _ => {
                        self.push(value)?;
                        self.push(Value::Bool(true))?;
                    }
                }
            }

            // Error handling
            Instruction::SetupTry(catch_ip, _) => {
                self.try_stack.push(TryInfo {
                    catch_ip,
                    frame_depth: self.frames.len(),
                    stack_depth: self.stack.len(),
                });
            }
            Instruction::Throw => {
                let val = self.pop()?;
                let msg = val.to_js_string();
                return Err(ZapcodeError::RuntimeError(msg));
            }
            Instruction::EndTry => {
                self.try_stack.pop();
            }

            // Typeof
            Instruction::TypeOf => {
                let val = self.pop()?;
                let type_str = val.type_name();
                self.push(Value::String(Arc::from(type_str)))?;
            }

            // Void
            Instruction::Void => {
                self.pop()?;
                self.push(Value::Undefined)?;
            }

            // Update
            Instruction::Increment => {
                let val = self.pop()?;
                let result = match val {
                    Value::Int(n) => Value::Int(n + 1),
                    _ => Value::Float(val.to_number() + 1.0),
                };
                self.push(result)?;
            }
            Instruction::Decrement => {
                let val = self.pop()?;
                let result = match val {
                    Value::Int(n) => Value::Int(n - 1),
                    _ => Value::Float(val.to_number() - 1.0),
                };
                self.push(result)?;
            }

            // Template literals
            Instruction::ConcatStrings(count) => {
                let mut parts = Vec::with_capacity(count);
                for _ in 0..count {
                    parts.push(self.pop()?);
                }
                parts.reverse();
                let result: String = parts.iter().map(|v| v.to_js_string()).collect();
                self.push(Value::String(Arc::from(result.as_str())))?;
            }

            // Destructuring
            Instruction::DestructureObject(keys) => {
                let obj = self.pop()?;
                for key in keys {
                    let val = self.get_property(&obj, &key)?;
                    self.push(val)?;
                }
            }
            Instruction::DestructureArray(count) => {
                let arr = self.pop()?;
                match arr {
                    Value::Array(items) => {
                        for i in 0..count {
                            self.push(items.get(i).cloned().unwrap_or(Value::Undefined))?;
                        }
                    }
                    _ => {
                        for _ in 0..count {
                            self.push(Value::Undefined)?;
                        }
                    }
                }
            }

            Instruction::Nop => {}

            // Generators
            Instruction::CreateGenerator(_func_idx) => {
                // Generator creation is handled at Call time via is_generator check.
            }
            Instruction::Yield => {
                // Yield is handled in run_generator_until_yield_or_return.
                // Reaching here means yield outside a generator function.
                return Err(ZapcodeError::RuntimeError(
                    "yield can only be used inside a generator function".to_string(),
                ));
            }

            Instruction::Await => {
                // Check if the value on the stack is a Promise object.
                // If resolved, unwrap its value. If rejected, throw its reason.
                // If it's a regular (non-promise) value, leave it as-is.
                let val = self.pop()?;
                if builtins::is_promise(&val) {
                    if let Value::Object(map) = &val {
                        let status = map.get("status").cloned().unwrap_or(Value::Undefined);
                        match status {
                            Value::String(s) if s.as_ref() == "resolved" => {
                                let inner = map.get("value").cloned().unwrap_or(Value::Undefined);
                                self.push(inner)?;
                            }
                            Value::String(s) if s.as_ref() == "rejected" => {
                                let reason = map.get("reason").cloned().unwrap_or(Value::Undefined);
                                return Err(ZapcodeError::RuntimeError(format!(
                                    "Unhandled promise rejection: {}",
                                    reason.to_js_string()
                                )));
                            }
                            _ => {
                                // Unknown status — pass through
                                self.push(val)?;
                            }
                        }
                    } else {
                        self.push(val)?;
                    }
                } else {
                    // Not a promise — pass through (await on non-promise returns the value)
                    self.push(val)?;
                }
            }

            // Classes
            Instruction::CreateClass {
                name,
                n_methods,
                n_statics,
                has_super,
            } => {
                // Stack layout (top to bottom):
                // constructor closure (or undefined)
                // n_methods * (closure, method_name_string) pairs
                // n_statics * (closure, method_name_string) pairs
                // [optional super class if has_super]

                let constructor = self.pop()?;

                // Pop instance methods
                let mut prototype = IndexMap::new();
                for _ in 0..n_methods {
                    let method_closure = self.pop()?;
                    let method_name = self.pop()?;
                    if let Value::String(mn) = method_name {
                        prototype.insert(mn, method_closure);
                    }
                }

                // Pop static methods
                let mut statics = IndexMap::new();
                for _ in 0..n_statics {
                    let method_closure = self.pop()?;
                    let method_name = self.pop()?;
                    if let Value::String(mn) = method_name {
                        statics.insert(mn, method_closure);
                    }
                }

                // Pop super class if present
                let super_class = if has_super { Some(self.pop()?) } else { None };

                // If super class, copy its prototype methods to ours (inheritance)
                if let Some(Value::Object(ref sc)) = super_class {
                    if let Some(Value::Object(super_proto)) = sc.get("__prototype__").cloned() {
                        // Super prototype methods go first, then our own (which override)
                        let mut merged = super_proto;
                        for (k, v) in prototype {
                            merged.insert(k, v);
                        }
                        prototype = merged;
                    }
                }

                // Build the class object
                let mut class_obj = IndexMap::new();
                class_obj.insert(
                    Arc::from("__class_name__"),
                    Value::String(Arc::from(name.as_str())),
                );
                class_obj.insert(Arc::from("__constructor__"), constructor);
                class_obj.insert(Arc::from("__prototype__"), Value::Object(prototype));

                // Store super class reference for super() calls
                if let Some(sc) = super_class {
                    class_obj.insert(Arc::from("__super__"), sc);
                }

                // Add static methods directly on the class object
                for (k, v) in statics {
                    class_obj.insert(k, v);
                }

                self.push(Value::Object(class_obj))?;
            }

            Instruction::Construct(arg_count) => {
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.pop()?);
                }
                args.reverse();

                let callee = self.pop()?;

                match &callee {
                    Value::Object(class_obj) if class_obj.contains_key("__class_name__") => {
                        // Create a new instance object
                        let mut instance = IndexMap::new();

                        // Copy prototype methods onto the instance
                        if let Some(Value::Object(proto)) = class_obj.get("__prototype__") {
                            for (k, v) in proto {
                                instance.insert(k.clone(), v.clone());
                            }
                        }

                        // Store class reference for instanceof
                        if let Some(class_name) = class_obj.get("__class_name__") {
                            instance.insert(Arc::from("__class__"), class_name.clone());
                        }

                        let instance_val = Value::Object(instance);

                        // Call the constructor with `this` bound to the instance
                        if let Some(ctor) = class_obj.get("__constructor__") {
                            if let Value::Function(closure) = ctor {
                                // Clear receiver source — constructors should not
                                // write back to a receiver variable.
                                self.last_receiver_source = None;
                                self.push_call_frame(closure, &args, Some(instance_val))?;
                                self.last_receiver = None;
                            } else {
                                // No valid constructor, just return the instance
                                self.push(instance_val)?;
                            }
                        } else {
                            // No constructor, just return the instance
                            self.push(instance_val)?;
                        }
                    }
                    Value::Function(closure) => {
                        // `new` on a plain function — just call it
                        self.push_call_frame(closure, &args, None)?;
                        self.last_receiver = None;
                    }
                    _ => {
                        return Err(ZapcodeError::TypeError(format!(
                            "{} is not a constructor",
                            callee.to_js_string()
                        )));
                    }
                }
            }

            Instruction::LoadThis => {
                // Walk frames from top to find the nearest `this` value
                let this_val = self
                    .frames
                    .iter()
                    .rev()
                    .find_map(|f| f.this_value.clone())
                    .unwrap_or(Value::Undefined);
                self.push(this_val)?;
            }
            Instruction::StoreThis => {
                let val = self.pop()?;
                // Update this_value in the nearest frame that has one
                for frame in self.frames.iter_mut().rev() {
                    if frame.this_value.is_some() {
                        frame.this_value = Some(val);
                        break;
                    }
                }
            }
            Instruction::CallSuper(arg_count) => {
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.pop()?);
                }
                args.reverse();

                // Get current `this` value (the instance being constructed)
                let this_val = self
                    .frames
                    .iter()
                    .rev()
                    .find_map(|f| f.this_value.clone())
                    .unwrap_or(Value::Undefined);

                // Find the super class constructor from the class that's being constructed.
                // We need to look it up from the globals — the class with __super__ key.
                // The super class info is stored on the class object.
                // We'll look through globals for the class that has __super__.
                let mut super_ctor = None;
                for val in self.globals.values() {
                    if let Value::Object(obj) = val {
                        if let Some(Value::Object(super_class)) = obj.get("__super__") {
                            if let Some(ctor) = super_class.get("__constructor__") {
                                super_ctor = Some(ctor.clone());
                                break;
                            }
                        }
                    }
                }

                if let Some(Value::Function(closure)) = super_ctor {
                    self.last_receiver_source = None;
                    self.push_call_frame(&closure, &args, Some(this_val))?;
                    self.last_receiver = None;
                } else {
                    // No super constructor found — push undefined
                    self.push(Value::Undefined)?;
                }
            }
        }

        Ok(None)
    }

    fn get_property(&self, obj: &Value, name: &str) -> Result<Value> {
        // Property access on null/undefined throws TypeError (like JS)
        if matches!(obj, Value::Null | Value::Undefined) {
            return Err(ZapcodeError::TypeError(format!(
                "Cannot read properties of {} (reading '{}')",
                obj.to_js_string(),
                name
            )));
        }
        match obj {
            Value::Object(map) => {
                // Check if property exists as a real value on the object
                if let Some(val) = map.get(name) {
                    if !matches!(val, Value::Undefined) {
                        return Ok(val.clone());
                    }
                }
                // Check if this is a promise instance — expose .then/.catch/.finally
                if builtins::is_promise(obj) && is_promise_method(name) {
                    return Ok(Value::BuiltinMethod {
                        object_name: Arc::from("__promise__"),
                        method_name: Arc::from(name),
                    });
                }
                // Check if this is a known global object — return builtin method handle
                if let Some(global_name) = &self.last_global_name {
                    let known_globals = ["console", "Math", "JSON", "Object", "Array", "Promise"];
                    if known_globals.contains(&global_name.as_str()) {
                        return Ok(Value::BuiltinMethod {
                            object_name: Arc::from(global_name.as_str()),
                            method_name: Arc::from(name),
                        });
                    }
                }
                Ok(Value::Undefined)
            }
            Value::Array(arr) => match name {
                "length" => Ok(Value::Int(arr.len() as i64)),
                _ if is_array_method(name) => Ok(Value::BuiltinMethod {
                    object_name: Arc::from("__array__"),
                    method_name: Arc::from(name),
                }),
                _ => {
                    if let Ok(idx) = name.parse::<usize>() {
                        Ok(arr.get(idx).cloned().unwrap_or(Value::Undefined))
                    } else {
                        Ok(Value::Undefined)
                    }
                }
            },
            Value::String(s) => match name {
                "length" => Ok(Value::Int(s.chars().count() as i64)),
                _ if is_string_method(name) => Ok(Value::BuiltinMethod {
                    object_name: Arc::from("__string__"),
                    method_name: Arc::from(name),
                }),
                _ => Ok(Value::Undefined),
            },
            Value::Generator(_) => match name {
                "next" | "return" | "throw" => Ok(Value::BuiltinMethod {
                    object_name: Arc::from("__generator__"),
                    method_name: Arc::from(name),
                }),
                _ => Ok(Value::Undefined),
            },
            _ => Ok(Value::Undefined),
        }
    }
}

// Re-export for the ParamPattern type used in function calls
use crate::zapcode_browser::parser::ir::ParamPattern;

fn is_array_method(name: &str) -> bool {
    matches!(
        name,
        "push"
            | "pop"
            | "shift"
            | "unshift"
            | "splice"
            | "slice"
            | "concat"
            | "join"
            | "reverse"
            | "sort"
            | "indexOf"
            | "lastIndexOf"
            | "includes"
            | "find"
            | "findIndex"
            | "map"
            | "filter"
            | "reduce"
            | "forEach"
            | "every"
            | "some"
            | "flat"
            | "flatMap"
            | "fill"
            | "at"
            | "entries"
            | "keys"
            | "values"
    )
}

fn is_string_method(name: &str) -> bool {
    matches!(
        name,
        "charAt"
            | "charCodeAt"
            | "indexOf"
            | "lastIndexOf"
            | "includes"
            | "startsWith"
            | "endsWith"
            | "slice"
            | "substring"
            | "substr"
            | "toUpperCase"
            | "toLowerCase"
            | "trim"
            | "trimStart"
            | "trimEnd"
            | "trimLeft"
            | "trimRight"
            | "padStart"
            | "padEnd"
            | "repeat"
            | "replace"
            | "replaceAll"
            | "split"
            | "concat"
            | "at"
            | "match"
            | "search"
            | "normalize"
    )
}

fn is_promise_method(name: &str) -> bool {
    matches!(name, "then" | "catch" | "finally")
}

/// Main entry point: compile and run TypeScript code.
pub struct ZapcodeRun {
    source: String,
    #[allow(dead_code)]
    inputs: Vec<String>,
    external_functions: Vec<String>,
    limits: ResourceLimits,
}

impl ZapcodeRun {
    pub fn new(
        source: String,
        inputs: Vec<String>,
        external_functions: Vec<String>,
        limits: ResourceLimits,
    ) -> Result<Self> {
        Ok(Self {
            source,
            inputs,
            external_functions,
            limits,
        })
    }

    pub fn run(&self, input_values: Vec<(String, Value)>) -> Result<RunResult> {
        let mut root_span = SpanBuilder::new("zapcode.run");

        // Parse
        let parse_span = SpanBuilder::new("parse");
        let program = match crate::zapcode_browser::parser::parse(&self.source) {
            Ok(p) => {
                root_span.add_child(parse_span.finish_ok());
                p
            }
            Err(e) => {
                root_span.add_child(parse_span.finish_error(&e.to_string()));
                let _trace = ExecutionTrace {
                    root: root_span.finish(TraceStatus::Error),
                };
                return Err(e);
            }
        };

        // Compile
        let compile_span = SpanBuilder::new("compile");
        let ext_set: HashSet<String> = self.external_functions.iter().cloned().collect();
        let compiled = match crate::zapcode_browser::compiler::compile_with_externals(&program, ext_set.clone()) {
            Ok(c) => {
                root_span.add_child(compile_span.finish_ok());
                c
            }
            Err(e) => {
                root_span.add_child(compile_span.finish_error(&e.to_string()));
                let _trace = ExecutionTrace {
                    root: root_span.finish(TraceStatus::Error),
                };
                return Err(e);
            }
        };

        // Execute
        let execute_span = SpanBuilder::new("execute");
        let mut vm = Vm::new(compiled, self.limits.clone(), ext_set);

        for (name, value) in input_values {
            vm.globals.insert(name, value);
        }

        let state = match vm.run() {
            Ok(s) => {
                let status = match &s {
                    VmState::Complete(_) => TraceStatus::Ok,
                    VmState::Suspended {
                        function_name,
                        args,
                        ..
                    } => {
                        let mut span = execute_span;
                        span.set_attr("zapcode.suspended_on", function_name);
                        span.set_attr("zapcode.args_count", args.len());
                        root_span.add_child(span.finish(TraceStatus::Ok));
                        let trace = ExecutionTrace {
                            root: root_span.finish_ok(),
                        };
                        return Ok(RunResult {
                            state: s,
                            stdout: vm.stdout,
                            trace,
                        });
                    }
                };
                root_span.add_child(execute_span.finish(status));
                s
            }
            Err(e) => {
                root_span.add_child(execute_span.finish_error(&e.to_string()));
                let _trace = ExecutionTrace {
                    root: root_span.finish(TraceStatus::Error),
                };
                return Err(e);
            }
        };

        let trace = ExecutionTrace {
            root: root_span.finish_ok(),
        };

        Ok(RunResult {
            state,
            stdout: vm.stdout,
            trace,
        })
    }

    /// Start execution. Like `run()`, but returns the raw `VmState` directly
    /// instead of wrapping it in a `RunResult`. This is the primary entry point
    /// for code that needs to handle suspension / snapshot / resume.
    pub fn start(&self, input_values: Vec<(String, Value)>) -> Result<VmState> {
        let result = self.run(input_values)?;
        Ok(result.state)
    }

    pub fn run_simple(&self) -> Result<Value> {
        let result = self.run(Vec::new())?;
        match result.state {
            VmState::Complete(v) => Ok(v),
            VmState::Suspended { function_name, .. } => Err(ZapcodeError::RuntimeError(format!(
                "execution suspended on external function '{}' — use run() instead",
                function_name
            ))),
        }
    }
}

/// Result of running a Zapcode program.
pub struct RunResult {
    pub state: VmState,
    pub stdout: String,
    /// Execution trace covering parse → compile → execute.
    pub trace: ExecutionTrace,
}

/// Quick helper to evaluate a TypeScript expression.
pub fn eval_ts(source: &str) -> Result<Value> {
    let runner = ZapcodeRun::new(
        source.to_string(),
        Vec::new(),
        Vec::new(),
        ResourceLimits::default(),
    )?;
    runner.run_simple()
}

/// Evaluate TypeScript and return both the value and stdout output.
pub fn eval_ts_with_output(source: &str) -> Result<(Value, String)> {
    let runner = ZapcodeRun::new(
        source.to_string(),
        Vec::new(),
        Vec::new(),
        ResourceLimits::default(),
    )?;
    let result = runner.run(Vec::new())?;
    match result.state {
        VmState::Complete(v) => Ok((v, result.stdout)),
        VmState::Suspended { function_name, .. } => Err(ZapcodeError::RuntimeError(format!(
            "execution suspended on external function '{}'",
            function_name
        ))),
    }
}
