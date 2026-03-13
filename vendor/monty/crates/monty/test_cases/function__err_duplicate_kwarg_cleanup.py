# Test that heap values are properly cleaned up when duplicate kwarg error occurs
def f(a, b):
    return a


# The list [1, 2, 3] should be cleaned up when the error occurs
# because 'a' is passed both positionally and by keyword
f([1, 2, 3], a=[4, 5])
# Raise=TypeError("f() got multiple values for argument 'a'")
