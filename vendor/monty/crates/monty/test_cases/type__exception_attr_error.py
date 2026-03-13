# Regression test for: "fmt() called on disabled variant" panic
# Type::Exception must be displayable in error messages.
e = ValueError('test')
e.nonexistent
"""
TRACEBACK:
Traceback (most recent call last):
  File "type__exception_attr_error.py", line 4, in <module>
    e.nonexistent
AttributeError: 'ValueError' object has no attribute 'nonexistent'
"""
