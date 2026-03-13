def foo():
    1 / 0


def bar():
    foo()


def baz():
    bar()


baz()
"""
TRACEBACK:
Traceback (most recent call last):
  File "traceback__division_error.py", line 13, in <module>
    baz()
    ~~~~~
  File "traceback__division_error.py", line 10, in baz
    bar()
    ~~~~~
  File "traceback__division_error.py", line 6, in bar
    foo()
    ~~~~~
  File "traceback__division_error.py", line 2, in foo
    1 / 0
    ~~~~~
ZeroDivisionError: division by zero
"""
