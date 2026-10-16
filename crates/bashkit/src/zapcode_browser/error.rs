use thiserror::Error;

#[derive(Debug, Error)]
pub enum ZapcodeError {
    #[error("parse error: {0}")]
    ParseError(String),

    #[error("unsupported syntax at {span}: {description}")]
    UnsupportedSyntax { span: String, description: String },

    #[error("compile error: {0}")]
    CompileError(String),

    #[error("runtime error: {0}")]
    RuntimeError(String),

    #[error("type error: {0}")]
    TypeError(String),

    #[error("reference error: {0} is not defined")]
    ReferenceError(String),

    #[error("unknown external function: {0}")]
    UnknownExternalFunction(String),

    #[error("memory limit exceeded: {0}")]
    MemoryLimitExceeded(String),

    #[error("execution time limit exceeded")]
    TimeLimitExceeded,

    #[error("stack overflow (depth {0})")]
    StackOverflow(usize),

    #[error("allocation limit exceeded")]
    AllocationLimitExceeded,

    #[error("snapshot error: {0}")]
    SnapshotError(String),

    #[error("sandbox violation: {0}")]
    SandboxViolation(String),
}

pub type Result<T> = std::result::Result<T, ZapcodeError>;
