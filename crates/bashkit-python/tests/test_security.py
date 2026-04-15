"""Security tests for bashkit Python bindings."""

# Decision: collect the merged security suite through one public module so the
# file layout matches the JS parity target while keeping the original white-box
# and black-box implementations readable in hidden source modules.

from . import _security_advanced as _advanced
from . import _security_core as _core

for _module in (_core, _advanced):
    for _name in dir(_module):
        if _name.startswith("test_") or _name.startswith("Test"):
            globals()[_name] = getattr(_module, _name)

del _advanced
del _core
del _module
del _name
