# Test: first positional-only parameter passed as keyword argument
def f(a, /, b):
    return a + b


f(a=1, b=2)
# Raise=TypeError("f() got some positional-only arguments passed as keyword arguments: 'a'")
