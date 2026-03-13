def outer():
    def inner():
        nonlocal x
        return x

    inner()
    x = 1


outer()
"""
TRACEBACK:
Traceback (most recent call last):
  File "traceback__nonlocal_unbound.py", line 10, in <module>
    outer()
    ~~~~~~~
  File "traceback__nonlocal_unbound.py", line 6, in outer
    inner()
    ~~~~~~~
  File "traceback__nonlocal_unbound.py", line 4, in inner
    return x
           ~
NameError: cannot access free variable 'x' where it is not associated with a value in enclosing scope
"""
