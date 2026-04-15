"""String-handling and quoting-adjacent Python binding tests."""

from . import _bashkit_categories as _categories

_NAMES = ("test_bash_python_string_ops",)

globals().update({name: getattr(_categories, name) for name in _NAMES})

del _categories
del _NAMES
