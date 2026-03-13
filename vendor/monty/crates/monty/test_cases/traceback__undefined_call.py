def foo():
    snap(1)


foo()
"""
TRACEBACK:
Traceback (most recent call last):
  File "traceback__undefined_call.py", line 5, in <module>
    foo()
    ~~~~~
  File "traceback__undefined_call.py", line 2, in foo
    snap(1)
    ~~~~
NameError: name 'snap' is not defined
"""
