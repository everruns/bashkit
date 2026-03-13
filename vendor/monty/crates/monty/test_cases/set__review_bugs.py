# Tests for review issues

# === frozenset repr for non-empty sets ===
# frozenset repr should show "frozenset({...})" not just "{...}"
fs_repr = repr(frozenset({1, 2}))
assert fs_repr == 'frozenset({1, 2})' or fs_repr == 'frozenset({2, 1})', 'frozenset repr should include type name'
assert repr(frozenset()) == 'frozenset()', 'empty frozenset repr'

# set repr should NOT have type prefix
s_repr = repr({1, 2})
assert s_repr == '{1, 2}' or s_repr == '{2, 1}', 'set repr should not have prefix'

# === issubset with range (non-Ref iterable) ===
# These should work, not raise TypeError
s = {1, 2, 3}
assert s.issubset(range(10)), 'issubset should accept range'
assert s.issuperset(range(1, 3)), 'issuperset should accept range'
assert s.isdisjoint(range(10, 20)), 'isdisjoint should accept range'

# === set construction with nested heap objects ===
# This tests ref counting - if refs are dropped before incrementing, this will fail
t = (1, 2)
s = set([t])
assert len(s) == 1, 'set should have one element'
assert repr(s) == '{(1, 2)}', 'set repr should not have prefix'

# More complex case - the list is temporary and will be dropped
s2 = set([(3, 4)])
assert len(s2) == 1, 'set from temp list should have one element'
assert repr(s2) == '{(3, 4)}', 'set repr should not have prefix'

# frozenset with nested objects
fs = frozenset([(5, 6)])
assert len(fs) == 1, 'frozenset from temp list should have one element'
assert repr(fs) == 'frozenset({(5, 6)})', 'frozenset repr should not have prefix'
