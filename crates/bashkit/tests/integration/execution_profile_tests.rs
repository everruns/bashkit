use bashkit::{
    Bash, ExecutionLimits, ExecutionProfile, ExecutionProfileName, FsLimits, MemoryLimits,
    SessionLimits,
};

#[test]
fn standard_profile_matches_secure_library_defaults() {
    let profile = ExecutionProfile::named(ExecutionProfileName::Standard);

    assert_eq!(
        profile.execution_limits().max_work_units,
        ExecutionLimits::default().max_work_units
    );
    assert_eq!(
        profile.execution_limits().max_commands,
        ExecutionLimits::default().max_commands
    );
    assert_eq!(
        profile.session_limits().max_exec_calls,
        SessionLimits::default().max_exec_calls
    );
    assert_eq!(
        profile.memory_limits().max_total_variable_bytes,
        MemoryLimits::default().max_total_variable_bytes
    );
    assert_eq!(
        profile.filesystem_limits().max_total_bytes,
        FsLimits::default().max_total_bytes
    );
    assert!(!profile.readonly_filesystem());
    assert!(!profile.network_enabled());
}

#[test]
fn interactive_profile_matches_current_cli_policy() {
    let profile = ExecutionProfile::named(ExecutionProfileName::Interactive);
    let cli = ExecutionLimits::cli();

    assert_eq!(profile.execution_limits().max_commands, cli.max_commands);
    assert_eq!(
        profile.execution_limits().max_loop_iterations,
        cli.max_loop_iterations
    );
    assert_eq!(
        profile.session_limits().max_total_commands,
        SessionLimits::unlimited().max_total_commands
    );
    assert_eq!(
        profile.memory_limits().max_total_variable_bytes,
        MemoryLimits::default().max_total_variable_bytes
    );
    assert_eq!(
        profile.filesystem_limits().max_total_bytes,
        FsLimits::default().max_total_bytes
    );
    assert!(!profile.readonly_filesystem());
    assert!(!profile.network_enabled());
}

#[test]
fn tm_dos_097_invalid_cross_family_limits_are_rejected_before_building_bash() {
    let error = ExecutionProfile::builder(ExecutionProfileName::Standard)
        .filesystem_limits(FsLimits::default().max_total_bytes(10).max_file_size(11))
        .build()
        .unwrap_err();

    assert_eq!(error.field(), "filesystem.max_file_size");
}

#[test]
fn runtime_profiles_reuse_runtime_defaults_and_tighten_hardened_limits() {
    let standard = ExecutionProfile::named(ExecutionProfileName::Standard);
    let hardened = ExecutionProfile::named(ExecutionProfileName::Hardened);
    let _ = (&standard, &hardened);

    assert!(
        hardened.execution_limits().max_work_units < standard.execution_limits().max_work_units
    );
    assert!(
        hardened.execution_limits().max_aggregate_input_bytes
            < standard.execution_limits().max_aggregate_input_bytes
    );
    assert!(
        hardened.execution_limits().max_live_intermediate_bytes
            < standard.execution_limits().max_live_intermediate_bytes
    );

    #[cfg(feature = "python")]
    {
        assert_eq!(
            standard.python_limits().common.max_memory,
            bashkit::PythonLimits::default().common.max_memory
        );
        assert!(
            hardened.python_limits().common.max_memory < standard.python_limits().common.max_memory
        );
    }
    #[cfg(feature = "http_client")]
    {
        assert_eq!(
            standard.http_limits().max_response_bytes,
            bashkit::HttpLimits::default().max_response_bytes
        );
        assert!(
            hardened.http_limits().max_response_bytes < standard.http_limits().max_response_bytes
        );
    }
    #[cfg(feature = "typescript")]
    {
        assert_eq!(
            standard.typescript_limits().max_allocations,
            bashkit::TypeScriptLimits::default().max_allocations
        );
        assert!(
            hardened.typescript_limits().max_allocations
                < standard.typescript_limits().max_allocations
        );
    }
    #[cfg(feature = "sqlite")]
    {
        assert_eq!(
            standard.sqlite_limits().max_db_bytes,
            bashkit::SqliteLimits::default().max_db_bytes
        );
        assert!(hardened.sqlite_limits().max_db_bytes < standard.sqlite_limits().max_db_bytes);
    }
}

#[cfg(feature = "bash_tool")]
#[tokio::test]
async fn bash_tool_builder_propagates_profile_before_explicit_limits() {
    use bashkit::{BashTool, Tool, ToolRequest};

    let profile = ExecutionProfile::builder(ExecutionProfileName::Standard)
        .execution_limits(ExecutionLimits::default().max_commands(0))
        .build()
        .unwrap();
    let tool = BashTool::builder().profile(profile).build();
    let response = tool
        .execute(ToolRequest {
            commands: ":".to_string(),
            timeout_ms: None,
        })
        .await;

    assert_ne!(response.exit_code, 0);
}

#[tokio::test]
async fn hardened_profile_applies_execution_and_filesystem_boundaries() {
    let hardened = ExecutionProfile::named(ExecutionProfileName::Hardened);
    let max_file_size = hardened.filesystem_limits().max_file_size;
    let mut bash = Bash::builder().profile(hardened).build();

    let result = bash.exec(":").await.unwrap();
    assert_eq!(result.exit_code, 0);

    let result = bash.exec("printf x > /tmp/profile-test").await.unwrap();
    assert_eq!(result.exit_code, 0, "isolated VFS writes remain available");

    let result = bash
        .exec(&format!(
            "truncate -s {} /tmp/profile-test",
            max_file_size + 1
        ))
        .await
        .unwrap();
    assert_ne!(result.exit_code, 0, "hardened file quota is enforced");
    assert!(max_file_size < FsLimits::default().max_file_size);
}

#[tokio::test]
async fn explicit_limit_override_after_profile_wins() {
    let mut bash = Bash::builder()
        .profile(ExecutionProfile::named(ExecutionProfileName::Hardened))
        .readonly_filesystem(false)
        .limits(ExecutionLimits::default().max_commands(0))
        .build();

    let error = bash.exec(":").await.unwrap_err();
    assert!(matches!(
        error,
        bashkit::Error::ResourceLimit(bashkit::LimitExceeded::MaxCommands(0))
    ));
}
