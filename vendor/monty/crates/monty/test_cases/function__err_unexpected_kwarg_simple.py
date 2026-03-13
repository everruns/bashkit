# Test: unexpected keyword argument on single-param function
def f(a):
    return a


f(1, b=2)
# Raise=TypeError("f() got an unexpected keyword argument 'b'")
