# Test: missing required argument with positional-only params
def f(a, b, /, c):
    return a + b + c


f(1, 2)
# Raise=TypeError("f() missing 1 required positional argument: 'c'")
