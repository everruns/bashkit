// Interceptor hooks for the Bash execution pipeline.
//
// Decision: all hooks are interceptors (can inspect, modify, or cancel).
// Decision: sync callbacks — async consumers bridge via channels.
// Decision: zero cost when no hooks registered (Vec::is_empty check).
// Decision: hooks registered via BashBuilder, frozen at build() — no mutex.

/// Result returned by an interceptor hook.
///
/// Every hook receives owned data and must return it (possibly modified)
/// via `Continue`, or abort the operation via `Cancel`.
pub enum HookAction<T> {
    /// Proceed with the (possibly modified) value.
    Continue(T),
    /// Abort the operation with a reason.
    Cancel(String),
}

/// An interceptor hook: receives owned data, returns [`HookAction`].
///
/// Must be `Send + Sync` so hooks can be registered from any thread
/// and fired from the async interpreter.
pub type Interceptor<T> = Box<dyn Fn(T) -> HookAction<T> + Send + Sync>;

/// Payload for `on_exit` hooks.
#[derive(Debug, Clone)]
pub struct ExitEvent {
    /// Exit code passed to the `exit` builtin (0–255).
    pub code: i32,
}

/// Payload for `before_exec` hooks.
#[derive(Debug, Clone)]
pub struct ExecInput {
    /// The script text about to be executed.
    pub script: String,
}

/// Payload for `after_exec` hooks.
#[derive(Debug, Clone)]
pub struct ExecOutput {
    /// Script that was executed.
    pub script: String,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// Exit code.
    pub exit_code: i32,
}

/// Payload for `before_tool` / `after_tool` hooks.
#[derive(Debug, Clone)]
pub struct ToolEvent {
    /// Tool (builtin command) name.
    pub name: String,
    /// Arguments passed to the tool.
    pub args: Vec<String>,
}

/// Payload for `after_tool` hooks.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Tool (builtin command) name.
    pub name: String,
    /// Standard output from the tool.
    pub stdout: String,
    /// Exit code.
    pub exit_code: i32,
}

/// Payload for `on_error` hooks.
#[derive(Debug, Clone)]
pub struct ErrorEvent {
    /// Error message.
    pub message: String,
}

/// Frozen registry of interceptor hooks.
///
/// Built via [`BashBuilder`](crate::BashBuilder) methods and immutable
/// after construction — no mutex needed.
#[derive(Default)]
pub struct Hooks {
    pub(crate) on_exit: Vec<Interceptor<ExitEvent>>,
    pub(crate) before_exec: Vec<Interceptor<ExecInput>>,
    pub(crate) after_exec: Vec<Interceptor<ExecOutput>>,
    pub(crate) before_tool: Vec<Interceptor<ToolEvent>>,
    pub(crate) after_tool: Vec<Interceptor<ToolResult>>,
    pub(crate) on_error: Vec<Interceptor<ErrorEvent>>,
}

impl Hooks {
    /// Fire `on_exit` hooks.  Returns the (possibly modified) event,
    /// or `None` if a hook cancelled the exit.
    pub(crate) fn fire_on_exit(&self, event: ExitEvent) -> Option<ExitEvent> {
        fire_hooks(&self.on_exit, event)
    }

    /// Fire `before_exec` hooks. Returns the (possibly modified) input,
    /// or `None` if a hook cancelled the execution.
    pub(crate) fn fire_before_exec(&self, event: ExecInput) -> Option<ExecInput> {
        fire_hooks(&self.before_exec, event)
    }

    /// Fire `after_exec` hooks. Returns the (possibly modified) output.
    pub(crate) fn fire_after_exec(&self, event: ExecOutput) -> Option<ExecOutput> {
        fire_hooks(&self.after_exec, event)
    }

    /// Fire `before_tool` hooks. Returns the (possibly modified) event,
    /// or `None` if a hook cancelled the tool invocation.
    #[allow(dead_code)] // TODO: wire into builtin execution pipeline
    pub(crate) fn fire_before_tool(&self, event: ToolEvent) -> Option<ToolEvent> {
        fire_hooks(&self.before_tool, event)
    }

    /// Fire `after_tool` hooks.
    #[allow(dead_code)] // TODO: wire into builtin execution pipeline
    pub(crate) fn fire_after_tool(&self, event: ToolResult) -> Option<ToolResult> {
        fire_hooks(&self.after_tool, event)
    }

    /// Fire `on_error` hooks.
    pub(crate) fn fire_on_error(&self, event: ErrorEvent) -> Option<ErrorEvent> {
        fire_hooks(&self.on_error, event)
    }

    /// Returns true if any hooks are registered.
    pub fn has_hooks(&self) -> bool {
        !self.on_exit.is_empty()
            || !self.before_exec.is_empty()
            || !self.after_exec.is_empty()
            || !self.before_tool.is_empty()
            || !self.after_tool.is_empty()
            || !self.on_error.is_empty()
    }
}

/// Generic hook firing: runs hooks in order, stops on Cancel.
fn fire_hooks<T>(hooks: &[Interceptor<T>], event: T) -> Option<T> {
    if hooks.is_empty() {
        return Some(event);
    }
    let mut current = event;
    for hook in hooks {
        match hook(current) {
            HookAction::Continue(e) => current = e,
            HookAction::Cancel(_) => return None,
        }
    }
    Some(current)
}

impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hooks")
            .field("on_exit", &format!("{} hook(s)", self.on_exit.len()))
            .field(
                "before_exec",
                &format!("{} hook(s)", self.before_exec.len()),
            )
            .field("after_exec", &format!("{} hook(s)", self.after_exec.len()))
            .field(
                "before_tool",
                &format!("{} hook(s)", self.before_tool.len()),
            )
            .field("after_tool", &format!("{} hook(s)", self.after_tool.len()))
            .field("on_error", &format!("{} hook(s)", self.on_error.len()))
            .finish()
    }
}
