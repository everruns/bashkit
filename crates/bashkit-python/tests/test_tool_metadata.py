"""Tool metadata and schema-surface tests."""

from . import _bashkit_categories as _categories

_NAMES = (
    "test_exec_result_to_dict",
    "test_exec_result_repr",
    "test_exec_result_str_success",
    "test_exec_result_str_failure",
    "test_description",
    "test_help",
    "test_system_prompt",
    "test_system_prompt_reflects_configured_home_path",
    "test_input_schema",
    "test_output_schema",
    "test_langchain_tool_spec",
    "test_scripted_tool_construction",
    "test_scripted_tool_custom_description",
    "test_scripted_tool_repr",
    "test_add_tool_increments_count",
    "test_add_tool_with_schema",
    "test_add_tool_no_schema",
    "test_scripted_tool_single_call",
    "test_scripted_tool_multiple_execute",
    "test_scripted_tool_async_execute",
    "test_scripted_tool_system_prompt",
    "test_scripted_tool_description",
    "test_scripted_tool_help",
    "test_scripted_tool_schemas",
    "test_scripted_tool_version",
)

globals().update({name: getattr(_categories, name) for name in _NAMES})

del _categories
del _NAMES
