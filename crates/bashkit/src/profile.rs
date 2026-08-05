//! Named, validated policy bundles for Bashkit execution.
//!
//! Decision: profiles establish a complete baseline across policy families;
//! existing builder methods remain explicit, last-write overrides. This keeps
//! one source of defaults while preserving fine-grained host control.

use std::fmt;
use std::time::Duration;

use crate::{ExecutionLimits, FsLimits, MemoryLimits, NetworkAllowlist, SessionLimits};

/// Closed set of supported execution-profile baselines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionProfileName {
    /// Tighter resource ceilings and no network; isolated VFS writes remain available.
    Hardened,
    /// Secure library defaults. This is Bashkit's default profile.
    #[default]
    Standard,
    /// CLI/REPL execution semantics with secure memory, VFS, and network defaults.
    Interactive,
}

/// Network policy carried by an execution profile.
#[derive(Debug, Clone, Default)]
pub enum ProfileNetworkPolicy {
    /// Do not install an outbound-network capability.
    #[default]
    Disabled,
    /// Install the supplied default-deny allowlist.
    Allowlist(NetworkAllowlist),
}

/// Invalid or unsupported profile configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionProfileError {
    field: &'static str,
    reason: &'static str,
}

impl ExecutionProfileError {
    fn new(field: &'static str, reason: &'static str) -> Self {
        Self { field, reason }
    }

    /// Configuration field that failed validation.
    pub fn field(&self) -> &'static str {
        self.field
    }
}

impl fmt::Display for ExecutionProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid execution profile field `{}`: {}",
            self.field, self.reason
        )
    }
}

impl std::error::Error for ExecutionProfileError {}

/// A coherent, validated policy bundle.
#[derive(Debug, Clone)]
pub struct ExecutionProfile {
    name: ExecutionProfileName,
    execution: ExecutionLimits,
    session: SessionLimits,
    memory: MemoryLimits,
    filesystem: FsLimits,
    readonly_filesystem: bool,
    network: ProfileNetworkPolicy,
    #[cfg(feature = "http_client")]
    http: crate::HttpLimits,
    #[cfg(feature = "python")]
    python: crate::builtins::PythonLimits,
    #[cfg(feature = "typescript")]
    typescript: crate::builtins::TypeScriptLimits,
    #[cfg(feature = "sqlite")]
    sqlite: crate::builtins::SqliteLimits,
}

impl Default for ExecutionProfile {
    fn default() -> Self {
        Self::named(ExecutionProfileName::Standard)
    }
}

impl ExecutionProfile {
    /// Resolve a named profile. Built-in profiles are valid by construction.
    pub fn named(name: ExecutionProfileName) -> Self {
        match name {
            ExecutionProfileName::Standard => Self::standard(),
            ExecutionProfileName::Hardened => Self::hardened(),
            ExecutionProfileName::Interactive => Self::interactive(),
        }
    }

    /// Start a validated profile with explicit family-level overrides.
    pub fn builder(name: ExecutionProfileName) -> ExecutionProfileBuilder {
        ExecutionProfileBuilder {
            profile: Self::named(name),
        }
    }

    fn standard() -> Self {
        Self {
            name: ExecutionProfileName::Standard,
            execution: ExecutionLimits::default(),
            session: SessionLimits::default(),
            memory: MemoryLimits::default(),
            filesystem: FsLimits::default(),
            readonly_filesystem: false,
            network: ProfileNetworkPolicy::Disabled,
            #[cfg(feature = "http_client")]
            http: crate::HttpLimits::default(),
            #[cfg(feature = "python")]
            python: crate::builtins::PythonLimits::default(),
            #[cfg(feature = "typescript")]
            typescript: crate::builtins::TypeScriptLimits::default(),
            #[cfg(feature = "sqlite")]
            sqlite: crate::builtins::SqliteLimits::default(),
        }
    }

    fn interactive() -> Self {
        Self {
            name: ExecutionProfileName::Interactive,
            execution: ExecutionLimits::cli(),
            session: SessionLimits::unlimited(),
            ..Self::standard()
        }
    }

    fn hardened() -> Self {
        let execution = ExecutionLimits {
            max_work_units: 25_000_000,
            max_aggregate_input_bytes: 25_000_000,
            max_live_intermediate_bytes: 8_000_000,
            max_commands: 2_500,
            max_loop_iterations: 2_500,
            max_total_loop_iterations: 100_000,
            max_function_depth: 64,
            timeout: Duration::from_secs(10),
            parser_timeout: Duration::from_secs(2),
            max_input_bytes: 1_000_000,
            max_ast_depth: 64,
            max_parser_operations: 50_000,
            max_stdout_bytes: 262_144,
            max_stderr_bytes: 262_144,
            max_subst_depth: 16,
            max_subshell_depth: 16,
            max_file_descriptors: 128,
            max_history_entries: 250,
            max_history_bytes: 262_144,
            max_history_output_bytes: 262_144,
            max_word_split_fields: 10_000,
            max_word_split_bytes: 1_000_000,
            capture_final_env: false,
        };
        let session = SessionLimits::new()
            .max_total_commands(25_000)
            .max_exec_calls(250);
        let memory = MemoryLimits::new()
            .max_variable_count(2_500)
            .max_total_variable_bytes(2_000_000)
            .max_array_entries(25_000)
            .max_function_count(250)
            .max_function_body_bytes(262_144);
        let filesystem = FsLimits::new()
            .max_total_bytes(25_000_000)
            .max_file_size(2_000_000)
            .max_file_count(2_500)
            .max_dir_count(2_500)
            .max_path_depth(64)
            .max_filename_length(255)
            .max_path_length(2_048);

        Self {
            name: ExecutionProfileName::Hardened,
            execution,
            session,
            memory,
            filesystem,
            readonly_filesystem: false,
            network: ProfileNetworkPolicy::Disabled,
            #[cfg(feature = "http_client")]
            http: crate::HttpLimits::default()
                .timeout(Duration::from_secs(10))
                .max_response_bytes(2 * 1024 * 1024),
            #[cfg(feature = "python")]
            python: crate::builtins::PythonLimits::default()
                .max_duration(Duration::from_secs(10))
                .max_memory(16 * 1024 * 1024)
                .max_recursion(100),
            #[cfg(feature = "typescript")]
            typescript: crate::builtins::TypeScriptLimits::default()
                .max_duration(Duration::from_secs(10))
                .max_memory(16 * 1024 * 1024)
                .max_stack_depth(256)
                .max_allocations(250_000),
            #[cfg(feature = "sqlite")]
            sqlite: crate::builtins::SqliteLimits::default()
                .max_script_bytes(1024 * 1024)
                .max_rows_per_query(100_000)
                .max_value_bytes(2 * 1024 * 1024)
                .max_result_bytes(2 * 1024 * 1024)
                .max_output_bytes(8 * 1024 * 1024)
                .max_db_bytes(64 * 1024 * 1024)
                .max_duration(Duration::from_secs(10))
                .max_statements(2_500),
        }
    }

    /// Named baseline used to construct this profile.
    pub fn name(&self) -> ExecutionProfileName {
        self.name
    }

    /// Script execution and parser limits.
    pub fn execution_limits(&self) -> &ExecutionLimits {
        &self.execution
    }

    /// Cumulative per-session limits.
    pub fn session_limits(&self) -> &SessionLimits {
        &self.session
    }

    /// Interpreter state-memory limits.
    pub fn memory_limits(&self) -> &MemoryLimits {
        &self.memory
    }

    /// Quotas for the builder-managed in-memory filesystem.
    pub fn filesystem_limits(&self) -> &FsLimits {
        &self.filesystem
    }

    /// Whether the managed filesystem is wrapped read-only.
    pub fn readonly_filesystem(&self) -> bool {
        self.readonly_filesystem
    }

    /// Outbound destination policy.
    pub fn network_policy(&self) -> &ProfileNetworkPolicy {
        &self.network
    }

    /// Whether this profile installs any outbound destination capability.
    pub fn network_enabled(&self) -> bool {
        matches!(self.network, ProfileNetworkPolicy::Allowlist(_))
    }

    /// HTTP resource limits, when the HTTP runtime is compiled.
    #[cfg(feature = "http_client")]
    pub fn http_limits(&self) -> &crate::HttpLimits {
        &self.http
    }

    #[cfg(feature = "python")]
    /// Embedded Python runtime limits.
    pub fn python_limits(&self) -> &crate::builtins::PythonLimits {
        &self.python
    }

    #[cfg(feature = "typescript")]
    /// Embedded TypeScript runtime limits.
    pub fn typescript_limits(&self) -> &crate::builtins::TypeScriptLimits {
        &self.typescript
    }

    #[cfg(feature = "sqlite")]
    /// Embedded SQLite runtime limits.
    pub fn sqlite_limits(&self) -> &crate::builtins::SqliteLimits {
        &self.sqlite
    }

    fn validate(&self) -> Result<(), ExecutionProfileError> {
        // THREAT[TM-DOS-097]: reject contradictory or ineffective profile
        // configuration at construction instead of silently weakening a cap.
        if self.filesystem.max_file_size > self.filesystem.max_total_bytes {
            return Err(ExecutionProfileError::new(
                "filesystem.max_file_size",
                "must not exceed filesystem.max_total_bytes",
            ));
        }
        if self.filesystem.max_filename_length > self.filesystem.max_path_length {
            return Err(ExecutionProfileError::new(
                "filesystem.max_filename_length",
                "must not exceed filesystem.max_path_length",
            ));
        }
        if self.execution.max_ast_depth > crate::parser::HARD_MAX_AST_DEPTH {
            return Err(ExecutionProfileError::new(
                "execution.max_ast_depth",
                "must not exceed the parser hard limit of 100",
            ));
        }
        if self.execution.timeout.is_zero() {
            return Err(ExecutionProfileError::new(
                "execution.timeout",
                "must be greater than zero",
            ));
        }
        if self.execution.parser_timeout.is_zero() {
            return Err(ExecutionProfileError::new(
                "execution.parser_timeout",
                "must be greater than zero",
            ));
        }
        #[cfg(not(feature = "http_client"))]
        if self.network_enabled() {
            return Err(ExecutionProfileError::new(
                "network",
                "requires the `http_client` feature",
            ));
        }
        #[cfg(feature = "python")]
        if self.python.common.max_duration.is_zero()
            || self.python.common.max_memory == 0
            || self.python.common.max_call_depth == 0
        {
            return Err(ExecutionProfileError::new(
                "python",
                "duration, memory, and recursion limits must be greater than zero",
            ));
        }
        #[cfg(feature = "http_client")]
        if self.http.timeout.is_zero() || self.http.max_response_bytes == 0 {
            return Err(ExecutionProfileError::new(
                "http",
                "timeout and response-size limits must be greater than zero",
            ));
        }
        #[cfg(feature = "typescript")]
        if self.typescript.common.max_duration.is_zero()
            || self.typescript.common.max_memory == 0
            || self.typescript.common.max_call_depth == 0
            || self.typescript.max_allocations == 0
        {
            return Err(ExecutionProfileError::new(
                "typescript",
                "all runtime limits must be greater than zero",
            ));
        }
        #[cfg(feature = "sqlite")]
        if self.sqlite.max_duration.is_zero()
            || self.sqlite.max_script_bytes == 0
            || self.sqlite.max_rows_per_query == 0
            || self.sqlite.max_value_bytes == 0
            || self.sqlite.max_result_bytes == 0
            || self.sqlite.max_output_bytes == 0
            || self.sqlite.max_db_bytes == 0
            || self.sqlite.max_statements == 0
        {
            return Err(ExecutionProfileError::new(
                "sqlite",
                "all runtime limits must be greater than zero",
            ));
        }
        Ok(())
    }
}

/// Builder for a named profile plus explicit policy-family overrides.
pub struct ExecutionProfileBuilder {
    profile: ExecutionProfile,
}

impl ExecutionProfileBuilder {
    /// Replace the execution/parser family.
    pub fn execution_limits(mut self, limits: ExecutionLimits) -> Self {
        self.profile.execution = limits;
        self
    }

    /// Replace the cumulative session family.
    pub fn session_limits(mut self, limits: SessionLimits) -> Self {
        self.profile.session = limits;
        self
    }

    /// Replace the interpreter-memory family.
    pub fn memory_limits(mut self, limits: MemoryLimits) -> Self {
        self.profile.memory = limits;
        self
    }

    /// Replace the managed-filesystem family.
    pub fn filesystem_limits(mut self, limits: FsLimits) -> Self {
        self.profile.filesystem = limits;
        self
    }

    /// Override managed-filesystem mutability.
    pub fn readonly_filesystem(mut self, readonly: bool) -> Self {
        self.profile.readonly_filesystem = readonly;
        self
    }

    /// Replace outbound destination policy.
    pub fn network_policy(mut self, policy: ProfileNetworkPolicy) -> Self {
        self.profile.network = policy;
        self
    }

    /// Override HTTP runtime resource limits.
    #[cfg(feature = "http_client")]
    pub fn http_limits(mut self, limits: crate::HttpLimits) -> Self {
        self.profile.http = limits;
        self
    }

    #[cfg(feature = "python")]
    /// Replace embedded Python limits.
    pub fn python_limits(mut self, limits: crate::builtins::PythonLimits) -> Self {
        self.profile.python = limits;
        self
    }

    #[cfg(feature = "typescript")]
    /// Replace embedded TypeScript limits.
    pub fn typescript_limits(mut self, limits: crate::builtins::TypeScriptLimits) -> Self {
        self.profile.typescript = limits;
        self
    }

    #[cfg(feature = "sqlite")]
    /// Replace embedded SQLite limits.
    pub fn sqlite_limits(mut self, limits: crate::builtins::SqliteLimits) -> Self {
        self.profile.sqlite = limits;
        self
    }

    /// Validate all cross-family invariants and produce the profile.
    pub fn build(self) -> Result<ExecutionProfile, ExecutionProfileError> {
        self.profile.validate()?;
        Ok(self.profile)
    }
}
