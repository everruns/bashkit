def foo(a):
    return a


foo(**{1: 'value'})
# Raise=TypeError('keywords must be strings')
