def foo():
    raise snap


foo()
"""
TRACEBACK:
Traceback (most recent call last):
  File "traceback__undefined_raise.py", line 5, in <module>
    foo()
    ~~~~~
  File "traceback__undefined_raise.py", line 2, in foo
    raise snap
          ~~~~
NameError: name 'snap' is not defined
"""
