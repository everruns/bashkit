# Self-referential list: a contains itself
# This creates a cycle where a's refcount is 2 (variable + self-reference)
# Without cycle detection, when 'a' goes out of scope, refcount drops to 1
# but the object is never freed (memory leak)
#
# NOTE: We return len(a) instead of a because repr(a) would cause infinite
# recursion / stack overflow (a separate bug - Python handles this by printing [...]
# for cyclic references)
a = []
a.append(a)
len(a)
# ref-counts={'a': 2}
