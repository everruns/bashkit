// Interceptor hooks for the Bash execution pipeline.
//
// Decision: all hooks are interceptors (can inspect, modify, or cancel).
// Decision: sync callbacks — async consumers bridge via channels.
// Decision: zero cost when no hooks registered (Vec::is_empty check).
// Decision: hooks registered via BashBuilder, frozen at build() — no mutex.
//
// Only `on_exit` is wired up now.  Other hooks (before_exec, after_exec,
// before_tool, after_tool, before_http, after_http, on_error) will be
// added as needed — the infrastructure is ready.  See issue #1235.

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

/// Frozen registry of interceptor hooks.
///
/// Built via [`BashBuilder::on_exit`](crate::BashBuilder::on_exit) and
/// immutable after construction — no mutex needed.
#[derive(Default)]
pub struct Hooks {
    pub(crate) on_exit: Vec<Interceptor<ExitEvent>>,
}

impl Hooks {
    /// Fire `on_exit` hooks.  Returns the (possibly modified) event,
    /// or `None` if a hook cancelled the exit.
    pub(crate) fn fire_on_exit(&self, event: ExitEvent) -> Option<ExitEvent> {
        if self.on_exit.is_empty() {
            return Some(event);
        }
        let mut current = event;
        for hook in &self.on_exit {
            match hook(current) {
                HookAction::Continue(e) => current = e,
                HookAction::Cancel(_) => return None,
            }
        }
        Some(current)
    }
}

impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hooks")
            .field("on_exit", &format!("{} hook(s)", self.on_exit.len()))
            .finish()
    }
}
