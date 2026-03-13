# Test: unexpected keyword argument
def f(a, b):
    return a + b


f(1, 2, c=3)
# Raise=TypeError("f() got an unexpected keyword argument 'c'")
