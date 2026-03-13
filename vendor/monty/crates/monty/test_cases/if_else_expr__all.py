# === Basic if/else ===
assert (1 if True else 2) == 1, 'true condition'
assert (1 if False else 2) == 2, 'false condition'

# === Truthy/falsy values ===
assert ('yes' if 1 else 'no') == 'yes', 'truthy int'
assert ('yes' if 0 else 'no') == 'no', 'falsy int'
assert ('yes' if 'a' else 'no') == 'yes', 'truthy str'
assert ('yes' if '' else 'no') == 'no', 'falsy str'
assert ('yes' if [1] else 'no') == 'yes', 'truthy list'
assert ('yes' if [] else 'no') == 'no', 'falsy list'
assert ('yes' if None else 'no') == 'no', 'None is falsy'

# === Variables and comparisons ===
x = 5
assert (x if x > 0 else -x) == 5, 'positive x'
x = -3
assert (x if x > 0 else -x) == 3, 'negative x - abs'

# === Nested if/else ===
a = 1
b = 2
c = 3
assert ((a if a > b else b) if True else c) == 2, 'nested - outer true'
assert ((a if a > b else b) if False else c) == 3, 'nested - outer false'
assert (a if True else (b if True else c)) == 1, 'nested in else - not evaluated'

# === Complex expressions ===
assert (1 + 2 if True else 3 + 4) == 3, 'arithmetic in body'
assert (1 + 2 if False else 3 + 4) == 7, 'arithmetic in orelse'

# === With heap values (strings, lists) ===
s1 = 'hello'
s2 = 'world'
assert (s1 if True else s2) == 'hello', 'string true branch'
assert (s1 if False else s2) == 'world', 'string false branch'

l1 = [1, 2]
l2 = [3, 4]
result = l1 if True else l2
assert result == [1, 2], 'list true branch'
result = l1 if False else l2
assert result == [3, 4], 'list false branch'

# === In f-strings ===
val = 10
assert f'{val if val > 5 else 0}' == '10', 'fstring with true branch'
val = 3
assert f'{val if val > 5 else 0}' == '0', 'fstring with false branch'
assert f'value: {1 if True else 2}' == 'value: 1', 'fstring with prefix'
assert f'{"yes" if 1 else "no"}' == 'yes', 'fstring with string result'

# === F-string with format spec ===
x = 42
assert f'{x if True else 0:05d}' == '00042', 'fstring format spec with if/else'
