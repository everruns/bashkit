# Test: missing required keyword-only argument
def f(a, *, b):
    return a + b


f(1)
# Raise=TypeError("f() missing 1 required keyword-only argument: 'b'")
