//! ScriptedTool and the `ToolDef` layer for [bashkit](https://docs.rs/bashkit).
//!
//! Compose [`ToolDef`] + callback pairs into one LLM [`Tool`](bashkit::Tool)
//! that accepts a bash script. Each registered tool becomes a command inside a
//! logic-only shell, so a model can orchestrate many calls in one turn with
//! pipes, variables, loops and `jq`.
//!
//! - [`ScriptedTool`]: the LLM tool. Build with [`ScriptedTool::builder`].
//! - [`ScriptingToolSet`]: ScriptedTool plus optional discovery tool.
//! - [`ToolRegistry`]: one set of tool definitions shared by the shell,
//!   embedded Python and embedded TypeScript. Install into any
//!   [`BashBuilder`](bashkit::BashBuilder) with [`ToolRegistry::install`].
//! - [`ToolDefExtension`]: the shell surface alone, as a bashkit
//!   [`Extension`](bashkit::Extension).
//!
//! ```rust
//! use bashkit::Tool;
//! use bashkit_scripted_tool::{ScriptedTool, ToolArgs, ToolDef};
//!
//! # tokio_test::block_on(async {
//! let tool = ScriptedTool::builder("api")
//!     .tool_fn(
//!         ToolDef::new("greet", "Greet a user"),
//!         |args: &ToolArgs| Ok(format!("hello {}\n", args.param_str("name").unwrap_or("world"))),
//!     )
//!     .build();
//! let output = tool
//!     .execution(serde_json::json!({"commands": "greet --name Alice"}))
//!     .unwrap()
//!     .execute()
//!     .await
//!     .unwrap();
//! assert_eq!(output.result["stdout"], "hello Alice\n");
//! # });
//! ```
//!
//! Moved out of the `bashkit` crate (formerly feature `scripted_tool`). See
//! `docs/migrating-scripted-tool.md` in the repository for the migration.

mod scripted_tool;
mod tool_def;
mod tool_registry;

pub use scripted_tool::{
    AsyncToolCallback, AsyncToolExec, CallbackKind, DiscoverTool, DiscoveryMode,
    ScriptedCommandInvocation, ScriptedCommandKind, ScriptedExecutionTrace, ScriptedTool,
    ScriptedToolBuilder, ScriptingToolSet, ScriptingToolSetBuilder, SyncToolExec, ToolArgs,
    ToolCallback, ToolDef, ToolDefExtension, ToolDefExtensionBuilder, ToolDefInvocationTrace,
    ToolImpl,
};
pub use tool_registry::{
    ToolCall, ToolCallDecision, ToolCallRequest, ToolCallSurface, ToolRegistry, ToolRegistryBuilder,
};
