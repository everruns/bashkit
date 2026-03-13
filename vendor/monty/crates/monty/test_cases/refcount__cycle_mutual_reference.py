# Mutual reference cycle: a contains b, b contains a
# This creates a cycle where:
#   - a has refcount 2 (variable 'a' + being inside b)
#   - b has refcount 2 (variable 'b' + being inside a)
# Without cycle detection, when both variables go out of scope:
#   - a's refcount drops to 1 (still in b)
#   - b's refcount drops to 1 (still in a)
#   - Neither reaches 0, neither is freed (memory leak)
#
# NOTE: We return len(b) instead of b because repr(b) would cause infinite
# recursion / stack overflow (a separate bug - Python handles this by printing [...]
# for cyclic references)
a = []
b = []
a.append(b)
b.append(a)
len(b)
# ref-counts={'a': 2, 'b': 2}
