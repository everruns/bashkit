# === sum() ===
# Basic sum operations
assert sum([1, 2, 3]) == 6, 'sum of list'
assert sum([1, 2, 3], 10) == 16, 'sum with start value'
assert sum(()) == 0, 'sum of empty tuple'
assert sum([], 5) == 5, 'sum of empty list with start'
assert sum(range(5)) == 10, 'sum of range'
assert sum([1.5, 2.5, 3.0], 0.0) == 7.0, 'sum of floats with float start'
# Note: sum of floats without start requires py_add to support int+float

# sum with different iterables
assert sum({1, 2, 3}) == 6, 'sum of set'
assert sum({1: 'a', 2: 'b', 3: 'c'}) == 6, 'sum of dict keys'

# === any() ===
# Basic any operations
assert any([True, False, False]) == True, 'any with one True'
assert any([False, False, False]) == False, 'any with all False'
assert any([]) == False, 'any of empty list'
assert any([0, 0, 1]) == True, 'any with truthy int'
assert any([0, '', None]) == False, 'any with all falsy'
assert any(['', 'hello']) == True, 'any with non-empty string'
assert any(range(0, 5)) == True, 'any of range (has non-zero)'
assert any(range(0, 1)) == False, 'any of range(0,1) is False (only 0)'

# === all() ===
# Basic all operations
assert all([True, True, True]) == True, 'all with all True'
assert all([True, False, True]) == False, 'all with one False'
assert all([]) == True, 'all of empty list'
assert all([1, 2, 3]) == True, 'all with truthy ints'
assert all([1, 0, 3]) == False, 'all with zero'
assert all(['a', 'b', 'c']) == True, 'all with non-empty strings'
assert all(['a', '', 'c']) == False, 'all with empty string'

# More edge cases with nested structures
assert any([[1], [], [3]]) == True, 'any with nested lists (some non-empty)'
assert all([[1], [2], [3]]) == True, 'all with non-empty nested lists'

# sum with lists (list + list is supported)
assert sum([[1], [2], [3]], []) == [1, 2, 3], 'sum lists with empty start'
# Note: sum with tuples requires Tuple py_add which is not implemented
