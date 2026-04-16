"""Basic Bash and BashTool behavior tests."""

# Decision: keep implementations centralized in `_bashkit_categories.py` during
# the layout split so follow-up parity issues can add focused coverage without
# copy-pasting existing tests again.

import sys
from pathlib import Path

_TESTS_DIR = str(Path(__file__).parent)
if _TESTS_DIR not in sys.path:
    sys.path.insert(0, _TESTS_DIR)

_categories = __import__("_bashkit_categories")

_NAMES = (
    "test_bash_default_construction",
    "test_bash_custom_construction",
    "test_bash_echo",
    "test_bash_exit_code",
    "test_bash_stderr",
    "test_bash_state_persists",
    "test_bash_reset",
    "test_bash_snapshot_roundtrip_from_snapshot_preserves_state_and_kwargs",
    "test_bash_restore_snapshot_after_reset_restores_original_state",
    "test_bash_shell_state_exposes_read_only_snapshot_view",
    "test_bash_empty_snapshot_roundtrip",
    "test_bash_async_execute",
    "test_bash_async_state_persists",
    "test_default_construction",
    "test_custom_construction",
    "test_echo",
    "test_exit_code",
    "test_stderr",
    "test_state_persists",
    "test_async_execute",
    "test_async_state_persists",
    "test_reset",
    "test_bashtool_snapshot_roundtrip_from_snapshot_preserves_state_and_kwargs",
    "test_bashtool_restore_snapshot_after_reset_restores_original_state",
    "test_bashtool_shell_state_exposes_read_only_snapshot_view",
    "test_bashtool_empty_snapshot_roundtrip",
    "test_reset_preserves_config",
    "test_execute_sync_releases_gil_for_callback",
    "test_bash_execute_sync_releases_gil",
    "test_bashtool_execute_sync_releases_gil",
    "test_bash_rapid_sync_calls_no_resource_exhaustion",
    "test_bashtool_rapid_sync_calls_no_resource_exhaustion",
    "test_bashtool_rapid_reset_no_resource_exhaustion",
    "test_bashtool_reset_preserves_config",
    "test_scripted_tool_rapid_sync_calls_no_resource_exhaustion",
    "test_bash_python_enabled",
    "test_bash_python_disabled_by_default",
    "test_bash_python_basic_arithmetic",
    "test_bash_cancel_then_reset_clears_cancellation",
    "test_bash_cancel_after_reset_still_works",
    "test_bashtool_cancel_then_reset_clears_cancellation",
    "test_bashtool_cancel_after_reset_still_works",
    "test_bash_clear_cancel_allows_subsequent_execution",
    "test_bashtool_clear_cancel_allows_subsequent_execution",
    "test_bash_async_clear_cancel_after_cancelled_execute",
    "test_bash_async_clear_cancel_preserves_state_after_cancelled_execute",
    "test_bashtool_async_clear_cancel_after_cancelled_execute",
    "test_bashtool_async_clear_cancel_preserves_state_after_cancelled_execute",
    "test_bash_clear_cancel_on_fresh_instance_is_noop",
    "test_bashtool_clear_cancel_on_fresh_instance_is_noop",
    "test_bash_clear_cancel_after_reset_is_noop",
    "test_bashtool_clear_cancel_after_reset_is_noop",
)

globals().update({name: getattr(_categories, name) for name in _NAMES})

del _categories
del _NAMES
del _TESTS_DIR
