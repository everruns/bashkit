# Test that heap values are cleaned up when duplicate kwargs error occurs
def f(a, b):
    return a


# The list value in the dict should be cleaned up when the error occurs
f(a=[1, 2, 3], **{'a': [4, 5, 6]})
# Raise=TypeError("f() got multiple values for keyword argument 'a'")
