# Test: first argument passed both positionally and by keyword
def f(a, b):
    return a + b


f(1, a=2)
# Raise=TypeError("f() got multiple values for argument 'a'")
