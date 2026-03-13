# === Basic arithmetic ===
assert 1 + 1 == 2, 'add ints'
assert 'a' + 'b' == 'ab', 'add strs'

# === Equality tests ===
assert (1 == 1) == True, 'ints equal true'
assert (1 == 2) == False, 'ints equal false'
assert ('a' == 'a') == True, 'str equal'
assert ('a' == 'b') == False, 'str not equal'
assert ([1, 2] == [1, 2]) == True, 'list equal'
assert ((1, 2) == (1, 2)) == True, 'tuple equal'
assert (b'hello' == b'hello') == True, 'bytes equal'

# === Boolean repr/str ===
assert repr(True) == 'True', 'bool true repr'
assert str(True) == 'True', 'bool true str'
assert repr(False) == 'False', 'bool false repr'
assert str(False) == 'False', 'bool false str'

# === None repr/str ===
assert repr(None) == 'None', 'none repr'
assert str(None) == 'None', 'none str'

# === Ellipsis repr/str ===
assert repr(...) == 'Ellipsis', 'ellipsis repr'
assert str(...) == 'Ellipsis', 'ellipsis str'

# === List repr/str ===
assert repr([1, 2]) == '[1, 2]', 'list repr'
assert str([1, 2]) == '[1, 2]', 'list str'

# === Discard expression result ===
a = 1
[1, 2, 3]  # this list is created and discarded
assert a == 1, 'discard list'

# === Shared list append ===
a = [1]
b = a
b.append(2)
assert len(a) == 2, 'shared list append'

# === For loop string append ===
v = ''
for i in range(1000):
    if i % 13 == 0:
        v = v + 'x'
assert len(v) == 77, 'for loop str append assign'

v = ''
for i in range(1000):
    if i % 13 == 0:
        v += 'x'
assert len(v) == 77, 'for loop str append assign op'
