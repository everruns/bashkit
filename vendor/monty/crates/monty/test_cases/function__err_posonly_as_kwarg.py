# Test: positional-only parameter passed as keyword argument
def f(a, b, /, c):
    return a + b + c


f(1, b=2, c=3)
# Raise=TypeError("f() got some positional-only arguments passed as keyword arguments: 'b'")
