def foo():
    snap.method()


foo()
"""
TRACEBACK:
Traceback (most recent call last):
  File "traceback__undefined_attr_call.py", line 5, in <module>
    foo()
    ~~~~~
  File "traceback__undefined_attr_call.py", line 2, in foo
    snap.method()
    ~~~~
NameError: name 'snap' is not defined
"""
