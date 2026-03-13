# Test: too many positional arguments with keyword-only params
def f(a, *, b):
    return a + b


f(1, 2, b=3)
# Raise=TypeError('f() takes 1 positional argument but 2 positional arguments (and 1 keyword-only argument) were given')
