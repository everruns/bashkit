# Test that heap values are properly cleaned up when missing required arg error occurs
def f(a, b, c):
    return a


# The list [1, 2, 3] should be cleaned up when the error occurs
# because 'c' is missing
f([1, 2, 3], [4, 5])
# Raise=TypeError("f() missing 1 required positional argument: 'c'")
