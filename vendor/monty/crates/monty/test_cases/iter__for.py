# === List iteration ===
result = []
for x in [1, 2, 3]:
    result.append(x)
assert result == [1, 2, 3], 'iterate over list'

# list with mixed types
result = []
for x in [1, 'a', True]:
    result.append(x)
assert result == [1, 'a', True], 'iterate over mixed list'

# empty list
result = []
for x in []:
    result.append(x)
assert result == [], 'iterate over empty list'

# nested list items
result = []
for x in [[1, 2], [3, 4]]:
    result.append(x)
assert result == [[1, 2], [3, 4]], 'iterate over nested lists'

# === Tuple iteration ===
result = []
for x in (1, 2, 3):
    result.append(x)
assert result == [1, 2, 3], 'iterate over tuple'

# empty tuple
result = []
for x in ():
    result.append(x)
assert result == [], 'iterate over empty tuple'

# tuple with mixed types
result = []
for x in (1, 'b', False):
    result.append(x)
assert result == [1, 'b', False], 'iterate over mixed tuple'

# === Dict iteration (yields keys) ===
result = []
for k in {'a': 1, 'b': 2, 'c': 3}:
    result.append(k)
assert result == ['a', 'b', 'c'], 'iterate over dict yields keys'

# empty dict
result = []
for k in {}:
    result.append(k)
assert result == [], 'iterate over empty dict'

# dict preserves insertion order
result = []
d = {'z': 1, 'a': 2, 'm': 3}
for k in d:
    result.append(k)
assert result == ['z', 'a', 'm'], 'dict iteration preserves insertion order'

# === String iteration (yields chars) ===
result = []
for c in 'abc':
    result.append(c)
assert result == ['a', 'b', 'c'], 'iterate over string yields chars'

# empty string
result = []
for c in '':
    result.append(c)
assert result == [], 'iterate over empty string'

# string with punctuation
result = []
for c in 'hi!':
    result.append(c)
assert result == ['h', 'i', '!'], 'iterate over string with punctuation'

# string with unicode (multi-byte UTF-8 characters)
result = []
for c in 'héllo':
    result.append(c)
assert result == ['h', 'é', 'l', 'l', 'o'], 'iterate over string with accented char'

# string with CJK characters
result = []
for c in '日本':
    result.append(c)
assert result == ['日', '本'], 'iterate over string with CJK chars'

# string with emoji
result = []
for c in 'a🎉b':
    result.append(c)
assert result == ['a', '🎉', 'b'], 'iterate over string with emoji'

# heap string
s = 'xyz'
s = s + '!'  # Force heap allocation
result = []
for c in s:
    result.append(c)
assert result == ['x', 'y', 'z', '!'], 'iterate over heap string'

# === Bytes iteration (yields ints) ===
result = []
for b in b'abc':
    result.append(b)
assert result == [97, 98, 99], 'iterate over bytes yields ints'

# empty bytes
result = []
for b in b'':
    result.append(b)
assert result == [], 'iterate over empty bytes'

# bytes with various values
result = []
for b in b'\x00\x01\xff':
    result.append(b)
assert result == [0, 1, 255], 'iterate over bytes with special values'

# === Range iteration (existing functionality) ===
result = []
for i in range(3):
    result.append(i)
assert result == [0, 1, 2], 'iterate over range'

# range with step
result = []
for i in range(0, 6, 2):
    result.append(i)
assert result == [0, 2, 4], 'iterate over range with step'

# === Nested iteration ===
result = []
for outer in [[1, 2], [3, 4]]:
    for inner in outer:
        result.append(inner)
assert result == [1, 2, 3, 4], 'nested for loops'

# iterate over string within list
result = []
for s in ['ab', 'cd']:
    for c in s:
        result.append(c)
assert result == ['a', 'b', 'c', 'd'], 'nested string iteration'

# === Using loop variable after loop ===
for x in [1, 2, 3]:
    pass
assert x == 3, 'loop variable persists after loop'

for y in 'abc':
    pass
assert y == 'c', 'string loop variable persists'

# === List mutation during iteration ===
# Python allows list mutation during iteration (unlike dict).
# The iterator checks current length on each iteration.

# appending during iteration - new items are seen
result = []
lst = [1, 2, 3]
for x in lst:
    result.append(x)
    if x == 2:
        lst.append(4)
assert result == [1, 2, 3, 4], 'appending to list during iteration sees new items'
assert lst == [1, 2, 3, 4], 'list was modified'

# appending multiple items
result = []
lst = [1]
for x in lst:
    result.append(x)
    if x < 5:
        lst.append(x + 1)
assert result == [1, 2, 3, 4, 5], 'can grow list dynamically during iteration'

# === Modifying via copy pattern ===
original = [1, 2, 3]
copy = list(original)
for x in copy:
    if x == 2:
        original.append(4)
assert original == [1, 2, 3, 4], 'modifying list via copy pattern'

# === Sum pattern ===
total = 0
for n in [1, 2, 3, 4, 5]:
    total = total + n
assert total == 15, 'sum pattern with list'

# === Early break simulation via flag ===
# (break not implemented, using flag pattern)
found = False
for x in [1, 2, 3, 4, 5]:
    if not found and x == 3:
        found = True
assert found == True, 'find pattern with flag'

# === Accumulator patterns ===
# count items
count = 0
for _ in ['a', 'b', 'c']:
    count = count + 1
assert count == 3, 'count items'

# concatenate strings
result = ''
for s in ['a', 'b', 'c']:
    result = result + s
assert result == 'abc', 'concatenate strings'

# === Dict key-value access pattern ===
d = {'x': 10, 'y': 20}
total = 0
for k in d:
    total = total + d[k]
assert total == 30, 'dict key-value access in loop'

# === Dict mutation during iteration ===
# Python allows modifying existing key values during iteration (no size change).
# It also allows pop + add that keeps size the same (iterator sees new keys).

# modifying existing values is allowed
d = {'a': 1, 'b': 2, 'c': 3}
for k in d:
    d[k] = d[k] * 10
assert d == {'a': 10, 'b': 20, 'c': 30}, 'modify dict values during iteration'

# pop + add keeping same size is allowed, iterator sees new keys
d = {'a': 1, 'b': 2, 'c': 3}
result = []
for k in d:
    result.append(k)
    if k == 'a':
        d.pop('b')
        d['x'] = 4  # size unchanged
assert result == ['a', 'c', 'x'], 'dict pop+add same size sees new keys'
assert d == {'a': 1, 'c': 3, 'x': 4}, 'dict was modified correctly'
