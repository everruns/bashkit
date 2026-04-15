"""Security tests for bashkit Python bindings."""

# Decision: collect the merged security suite through one public module so the
# file layout matches the JS parity target while keeping the original white-box
# and black-box implementations readable in hidden source modules.

import sys
from pathlib import Path

_TESTS_DIR = str(Path(__file__).parent)
if _TESTS_DIR not in sys.path:
    sys.path.insert(0, _TESTS_DIR)

_advanced = __import__("_security_advanced")
_core = __import__("_security_core")

for _module in (_core, _advanced):
    for _name in dir(_module):
        if _name.startswith("test_") or _name.startswith("Test"):
            globals()[_name] = getattr(_module, _name)

del _advanced
del _core
del _module
del _name
del _TESTS_DIR
