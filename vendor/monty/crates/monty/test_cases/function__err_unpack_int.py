def f(a, b):
    return a + b


f(1, **42)
# Raise=TypeError('f() argument after ** must be a mapping, not int')
