//! Consolidated integration test binary.
//!
//! Each `tests/*.rs` would normally become its own integration-test binary,
//! statically linking every embedded interpreter (monty, zapcode, turso,
//! russh, jaq, reqwest+rustls, ed25519-dalek). With ~80 such files the link
//! step alone blew out CI disk on the hosted runner (`rustc-LLVM ERROR: IO
//! failure on output stream: No space left on device`).
//!
//! Per `specs/testing.md`, default integration tests live as `mod`s under
//! `tests/integration/`, declared here. Tests that genuinely need their own
//! binary (process-global env mutation, `--test-threads=1`, feature-isolation
//! sweeps) stay as siblings at `tests/<name>.rs`.

#![allow(clippy::single_component_path_imports)]

pub mod agent_skills_publication_tests;
pub mod allexport_tests;
pub mod awk_fuzz_scaffold_tests;
pub mod awk_newline_tests;
pub mod awk_pattern_tests;
pub mod awk_printf_expr_test;
pub mod awk_range_pattern_tests;
pub mod background_exec_tests;
pub mod base64_binary_tests;
pub mod bash_source_tests;
pub mod blackbox_security_tests;
pub mod builtin_error_security_tests;
pub mod builtin_registry_tests;
pub mod byte_range_panic_tests;
pub mod cancellation_tests;
pub mod cmdsub_quote_test;
pub mod compgen_tests;
pub mod coproc_tests;
pub mod coreutils_differential_tests;
pub mod credential_injection_tests;
pub mod custom_builtins_tests;
pub mod custom_fs_tests;
pub mod dev_null_tests;
pub mod final_env_tests;
pub mod find_multi_path_tests;
pub mod git_advanced_tests;
pub mod git_inspection_tests;
pub mod git_integration_tests;
pub mod git_remote_security_tests;
pub mod git_security_tests;
pub mod harness_example_tests;
pub mod history_tests;
pub mod issue_1175_test;
pub mod issue_1776_test;
pub mod issue_1777_test;
pub mod issue_274_test;
pub mod issue_275_279_282_test;
pub mod issue_276_test;
pub mod issue_277_test;
pub mod issue_289_test;
pub mod issue_290_test;
pub mod issue_291_test;
pub mod issue_853_test;
pub mod issue_872_test;
pub mod issue_873_test;
pub mod issue_875_test;
pub mod jq_fuzz_scaffold_tests;
pub mod limitations_doc_tests;
pub mod live_mount_tests;
pub mod mkfifo_tests;
pub mod nested_subscript_tests;
pub mod network_security_tests;
pub mod output_truncation_tests;
pub mod proptest_differential;
pub mod python_integration_tests;
pub mod python_security_tests;
pub mod regex_limit_tests;
pub mod release_profile_tests;
pub mod script_execution_tests;
pub mod security_audit_pocs;
pub mod set_e_and_or_tests;
pub mod shuf_resource_tests;
pub mod skills_tests;
pub mod snapshot_tests;
pub mod source_function_tests;
pub mod spec_runner;
pub mod spec_tests;
pub mod sqlite_compat_tests;
pub mod sqlite_differential_tests;
pub mod sqlite_fuzz_tests;
pub mod sqlite_integration_tests;
pub mod sqlite_security_tests;
pub mod stack_overflow_regression_tests;
pub mod subst_depth_limit_tests;
pub mod symlink_overlay_security_tests;
pub mod threat_model_doc_tests;
pub mod threat_model_tests;
pub mod tty_tests;
pub mod typescript_integration_tests;
pub mod typescript_security_tests;
pub mod unicode_security_tests;
pub mod unset_function_tests;
pub mod urandom_tests;
pub mod workflow_security_tests;
