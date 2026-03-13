# Test: missing all positional-only arguments
def f(a, b, /):
    return a + b


f()
# Raise=TypeError("f() missing 2 required positional arguments: 'a' and 'b'")
