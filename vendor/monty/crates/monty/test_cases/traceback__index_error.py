def foo():
    a = []
    a[1]


foo()
"""
TRACEBACK:
Traceback (most recent call last):
  File "traceback__index_error.py", line 6, in <module>
    foo()
    ~~~~~
  File "traceback__index_error.py", line 3, in foo
    a[1]
    ~~~~
IndexError: list index out of range
"""
