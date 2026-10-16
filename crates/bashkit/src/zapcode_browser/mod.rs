//! # zapcode-core
//!
//! A minimal, secure TypeScript interpreter for AI agent code execution.
//!
//! ## Architecture
//!
//! ```text
//! TypeScript source
//!     │
//!     ▼
//! ┌─────────┐
//! │  parser  │  oxc_parser → ZapcodeIR (parser/ir.rs)
//! └────┬────┘
//!      ▼
//! ┌──────────┐
//! │ compiler │  ZapcodeIR → stack-based bytecode (compiler/instruction.rs)
//! └────┬─────┘
//!      ▼
//! ┌─────────┐
//! │   vm    │  Execute bytecode, snapshot at external calls, resume later
//! └────┬────┘
//!      ▼
//!   VmState::Complete(value) | VmState::Suspended { snapshot }
//! ```
//!
//! ## Key modules
//!
//! - [`parser`] — Walks the oxc AST and emits [`parser::ir::ZapcodeIR`]
//! - [`compiler`] — Lowers IR to [`compiler::instruction::Instruction`] bytecode
//! - [`vm`] — Stack-based VM that executes bytecode; entry point is [`ZapcodeRun`]
//! - [`value`] — Runtime value types ([`Value`], closures, generators)
//! - [`snapshot`] — Serialize/deserialize VM state for suspension and resumption
//! - [`sandbox`] — Resource limits (memory, time, stack depth, allocations)
//! - [`error`] — Error types used across all modules
//!
//! ## Security model
//!
//! The sandbox is enforced at the language level: no filesystem, network, env,
//! `eval`, `import`, or `require`. The only way guest code can interact with the
//! host is through registered external functions that suspend the VM.

pub mod compiler;
pub mod error;
pub mod parser;
pub mod sandbox;
pub mod snapshot;
pub mod trace;
pub mod value;
pub mod vm;

pub use error::ZapcodeError;
pub use sandbox::ResourceLimits;
pub use snapshot::ZapcodeSnapshot;
pub use trace::{ExecutionTrace, TraceSpan, TraceStatus};
pub use value::Value;
pub use vm::{RunResult, VmState, ZapcodeRun};
