"""Script-shaped workflows and higher-level Python binding scenarios."""

# Decision: keep the legacy implementations centralized until parity follow-up
# issues add the larger dedicated suites for builtins, strings, and scripts.

from . import _bashkit_categories as _categories

_NAMES = (
    "test_bash_file_persistence",
    "test_file_persistence",
    "test_multiline",
    "test_scripted_tool_pipeline_with_jq",
    "test_scripted_tool_multi_step",
    "test_scripted_tool_error_fallback",
    "test_scripted_tool_stdin_pipe",
    "test_scripted_tool_env_var",
    "test_scripted_tool_boolean_flag",
    "test_scripted_tool_integer_coercion",
    "test_async_multiple_tools",
    "test_scripted_tool_dozen_tools",
    "test_bash_external_handler_called",
    "test_bash_external_handler_multiple_calls",
    "test_bash_external_handler_returns_dict",
    "test_bash_external_handler_vfs_and_tools_together",
    "test_bash_reset_preserves_python_and_handler",
    "test_bash_external_handler_without_external_functions",
    "test_bash_bigint_roundtrip",
    "test_bash_bigint_roundtrip_value",
    "test_bash_async_callable_object_accepted",
    "test_bash_handler_receives_set_as_set",
)

globals().update({name: getattr(_categories, name) for name in _NAMES})

del _categories
del _NAMES
