def recurse():
    recurse()


recurse()
"""
TRACEBACK:
Traceback (most recent call last):
  File "traceback__recursion_error.py", line 5, in <module>
    recurse()
    ~~~~~~~~~
  File "traceback__recursion_error.py", line 2, in recurse
    recurse()
    ~~~~~~~~~
  File "traceback__recursion_error.py", line 2, in recurse
    recurse()
    ~~~~~~~~~
  File "traceback__recursion_error.py", line 2, in recurse
    recurse()
    ~~~~~~~~~
  [Previous line repeated 47 more times]
RecursionError: maximum recursion depth exceeded
"""
