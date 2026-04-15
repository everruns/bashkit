"""Security tests for bashkit Python bindings."""

# Decision: collect the merged security suite through one public module so the
# file layout matches the JS parity target while keeping the original white-box
# and black-box implementations readable in hidden source modules.
# Naming: `test_tm_<category>_<id>_<scenario>` maps directly to
# `specs/006-threat-model.md` so grepable Python test names line up with the
# documented threat model.

import sys
from pathlib import Path

_TESTS_DIR = str(Path(__file__).parent)
if _TESTS_DIR not in sys.path:
    sys.path.insert(0, _TESTS_DIR)

_advanced = __import__("_security_advanced")
_core = __import__("_security_core")

_TM_TEST_EXPORTS = (
    "test_tm_inf_001_vfs_cannot_read_host_etc_passwd",
    "test_tm_esc_003_vfs_cannot_read_host_proc",
    "test_tm_iso_002_vfs_writes_are_isolated_between_instances",
    "test_tm_inj_005_vfs_directory_traversal_is_blocked",
    "test_tm_dos_016_max_loop_iterations_enforced",
    "test_tm_dos_021_fork_bomb_prevented",
    "test_tm_dos_005_large_file_write_limited",
    "test_tm_dos_006_vfs_file_count_limit",
    "test_tm_dos_012_deep_directory_nesting_limited",
    "test_tm_dos_013_long_filename_rejected",
    "test_tm_dos_013_long_path_rejected",
    "test_tm_dos_029_arithmetic_overflow_does_not_crash",
    "test_tm_dos_029_division_by_zero_does_not_crash",
    "test_tm_dos_029_modulo_by_zero_does_not_crash",
    "test_tm_dos_029_negative_exponent_does_not_crash",
    "test_tm_dos_059_default_memory_limit_prevents_oom_without_max_memory",
    "test_tm_inf_002_env_builtins_do_not_leak_host_env",
    "test_tm_inf_002_default_env_vars_are_sandboxed",
    "test_tm_esc_002_process_substitution_stays_in_sandbox",
    "test_tm_esc_005_signal_trap_commands_stay_bounded",
    "test_tm_inj_005_direct_read_file_traversal_is_blocked",
    "test_tm_int_001_shell_errors_do_not_leak_host_paths",
    "test_tm_int_001_direct_fs_errors_do_not_leak_host_paths",
    "test_tm_int_002_parse_errors_do_not_leak_addresses_or_traces",
    "test_tm_int_002_callback_errors_do_not_leak_rust_internals",
    "test_tm_net_001_dev_tcp_network_escape",
    "test_tm_dos_002_max_commands_exact_boundary",
    "test_tm_iso_001_env_pollution_between_calls",
    "test_tm_iso_003_function_persistence_is_per_instance",
    "test_tm_uni_003_unicode_path_normalization_escape",
    "test_tm_uni_004_emoji_and_rtl_in_commands",
)

assert len(_TM_TEST_EXPORTS) >= 20

for _module in (_core, _advanced):
    for _name in dir(_module):
        if _name.startswith("test_") or _name.startswith("Test"):
            globals()[_name] = getattr(_module, _name)

del _advanced
del _core
del _TM_TEST_EXPORTS
del _module
del _name
del _TESTS_DIR
