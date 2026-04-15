"""Control-flow and resource-limit behavior tests."""

import sys
from pathlib import Path

_TESTS_DIR = str(Path(__file__).parent)
if _TESTS_DIR not in sys.path:
    sys.path.insert(0, _TESTS_DIR)

_categories = __import__("_bashkit_categories")

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
del _TESTS_DIR
