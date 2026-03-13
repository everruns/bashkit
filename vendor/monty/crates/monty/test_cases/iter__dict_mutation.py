d = {'a': 1, 'b': 2}
for k in d:
    d['c'] = 3
# Raise=RuntimeError('dictionary changed size during iteration')
