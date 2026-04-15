"""Error-path coverage for the Python bindings."""

from . import _bashkit_categories as _categories

_NAMES = (
    "test_bash_invalid_snapshot_raises_bash_error",
    "test_bash_empty_input",
    "test_bash_nonexistent_command",
    "test_bashtool_invalid_snapshot_raises_bash_error",
    "test_malformed_bash_syntax",
    "test_nonexistent_command",
    "test_large_output",
    "test_empty_input",
    "test_scripted_tool_callback_error",
    "test_scripted_tool_callback_runtime_error",
    "test_scripted_tool_callback_type_error",
    "test_scripted_tool_large_callback_output",
    "test_scripted_tool_callback_returns_empty",
    "test_scripted_tool_empty_script",
    "test_deeply_nested_schema_rejected",
    "test_bash_pre_exec_error_in_stderr",
    "test_bashtool_pre_exec_error_in_stderr",
    "test_bash_pre_exec_error_in_stderr_async",
    "test_bash_external_handler_error_propagates",
    "test_bash_external_functions_without_handler_raises",
    "test_bash_non_callable_handler_raises",
    "test_bash_external_handler_requires_python_true",
    "test_bash_execute_sync_with_handler_raises",
    "test_bash_sync_handler_raises",
)

globals().update({name: getattr(_categories, name) for name in _NAMES})

del _categories
del _NAMES
