"""Control-flow and resource-limit behavior tests."""

from . import _bashkit_categories as _categories

_NAMES = (
    "test_bash_max_loop_iterations",
    "test_max_loop_iterations_prevents_infinite_loop",
    "test_max_commands_limits_execution",
    "test_scripted_tool_loop",
    "test_scripted_tool_conditional",
)

globals().update({name: getattr(_categories, name) for name in _NAMES})

del _categories
del _NAMES
