# note cpython behaves weirdly if the dict is empty: https://github.com/python/cpython/issues/142396
d = {1: 2}
d.pop([], 'fallback')
# Raise=TypeError("cannot use 'list' as a dict key (unhashable type: 'list')")
