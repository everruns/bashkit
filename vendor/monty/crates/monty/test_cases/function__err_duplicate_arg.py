# Test: same argument passed both positionally and by keyword
def f(a, b, c):
    return a + b + c


f(1, 2, b=3)
# Raise=TypeError("f() got multiple values for argument 'b'")
