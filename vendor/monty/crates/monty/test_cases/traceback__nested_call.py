def foo():
    raise ValueError('xxx')


def bar():
    foo()


def baz():
    bar()


baz()
"""
TRACEBACK:
Traceback (most recent call last):
  File "traceback__nested_call.py", line 13, in <module>
    baz()
    ~~~~~
  File "traceback__nested_call.py", line 10, in baz
    bar()
    ~~~~~
  File "traceback__nested_call.py", line 6, in bar
    foo()
    ~~~~~
  File "traceback__nested_call.py", line 2, in foo
    raise ValueError('xxx')
ValueError: xxx
"""
