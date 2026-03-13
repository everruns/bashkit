# Test: keyword-only argument passed positionally
def f(*, a, b):
    return a + b


f(1, 2)
# Raise=TypeError('f() takes 0 positional arguments but 2 were given')
