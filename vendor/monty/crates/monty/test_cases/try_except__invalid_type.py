try:
    raise ValueError('test')
except 123:
    pass
# Raise=TypeError('catching classes that do not inherit from BaseException is not allowed')
