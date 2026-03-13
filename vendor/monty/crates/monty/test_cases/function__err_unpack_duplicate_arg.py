def f(a, b):
    return a + b


f(a=1, **{'a': 2})
# Raise=TypeError("f() got multiple values for keyword argument 'a'")
