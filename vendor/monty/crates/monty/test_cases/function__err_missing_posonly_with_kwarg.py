# Test: missing positional-only when keyword is provided for other param
def f(a, /, b):
    return a + b


f(b=2)
# Raise=TypeError("f() missing 1 required positional argument: 'a'")
