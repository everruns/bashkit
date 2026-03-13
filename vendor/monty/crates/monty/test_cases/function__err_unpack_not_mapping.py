def f(a, b):
    return a + b


f(1, **[2])
# Raise=TypeError('f() argument after ** must be a mapping, not list')
