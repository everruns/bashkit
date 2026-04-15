"""Builtin-command coverage pulled out of the legacy Python test module."""

from . import _bashkit_categories as _categories

_NAMES = ("test_bash_pipeline",)

globals().update({name: getattr(_categories, name) for name in _NAMES})

del _categories
del _NAMES
