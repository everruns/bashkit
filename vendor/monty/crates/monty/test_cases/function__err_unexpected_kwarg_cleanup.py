# Test that heap values are properly cleaned up when unexpected kwarg error occurs
def f(a, b):
    return a


# The list [1, 2, 3] should be cleaned up when the error occurs
# because 'c' is an unexpected keyword argument
f([1, 2, 3], [4, 5], c=[6, 7])
# Raise=TypeError("f() got an unexpected keyword argument 'c'")
