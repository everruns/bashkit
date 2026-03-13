# === Basic elif chains ===

# if branch taken
x = 0
if True:
    x = 1
elif True:
    x = 2
assert x == 1, 'if branch should be taken when condition is True'

# elif branch taken (first elif)
y = 0
if False:
    y = 1
elif True:
    y = 2
elif True:
    y = 3
assert y == 2, 'first elif should be taken when if is False and elif is True'

# second elif taken
z = 0
if False:
    z = 1
elif False:
    z = 2
elif True:
    z = 3
assert z == 3, 'second elif should be taken'

# else branch taken after if
a = 0
if False:
    a = 1
else:
    a = 2
assert a == 2, 'else should be taken when if is False'

# else branch taken after elif chain
b = 0
if False:
    b = 1
elif False:
    b = 2
elif False:
    b = 3
else:
    b = 4
assert b == 4, 'else should be taken when all conditions are False'

# === Value-based conditions ===

val = 5

c = 0
if val < 3:
    c = 1
elif val < 6:
    c = 2
elif val < 9:
    c = 3
else:
    c = 4
assert c == 2, 'elif condition val < 6 should match for val=5'

val2 = 10
d = 0
if val2 < 3:
    d = 1
elif val2 < 6:
    d = 2
elif val2 < 9:
    d = 3
else:
    d = 4
assert d == 4, 'else should be taken for val2=10'

# === Multiple statements in branches ===

e = 0
f = 0
if False:
    e = 1
    f = 1
elif True:
    e = 2
    f = 2
else:
    e = 3
    f = 3
assert e == 2, 'first statement in elif executed'
assert f == 2, 'second statement in elif executed'

# === Nested if inside elif ===

g = 0
if False:
    g = 1
elif True:
    if True:
        g = 100
    else:
        g = 200
else:
    g = 3
assert g == 100, 'nested if inside elif should work'

# nested if in else
h = 0
if False:
    h = 1
elif False:
    h = 2
else:
    if True:
        h = 300
    else:
        h = 400
assert h == 300, 'nested if inside else should work'

# === Short-circuit evaluation ===

# elif condition not evaluated if earlier branch taken
called = False


def set_called():
    global called
    called = True
    return True


i = 0
if True:
    i = 1
elif set_called():
    i = 2
assert i == 1, 'if branch taken'
assert called == False, 'elif condition should not be evaluated when if branch is taken'

# reset and test elif evaluation
called = False
j = 0
if False:
    j = 1
elif set_called():
    j = 2
assert j == 2, 'elif branch taken'
assert called == True, 'elif condition should be evaluated when if condition is False'

# === Empty body handling (pass) ===

k = 0
if False:
    pass
elif True:
    k = 1
else:
    pass
assert k == 1, 'elif body executes after if with pass'

# === Boolean expression conditions ===

and_result = 0
if False and True:
    and_result = 1
elif True and True:
    and_result = 2
else:
    and_result = 3
assert and_result == 2, 'elif with and condition'

or_result = 0
if False or False:
    or_result = 1
elif False or True:
    or_result = 2
else:
    or_result = 3
assert or_result == 2, 'elif with or condition'

# === Multiple conditions with and ===

n = 5
o = 0
if n > 1 and n < 3:
    o = 1
elif n > 3 and n < 7:
    o = 2
else:
    o = 3
assert o == 2, 'elif with multiple and conditions'

# === Variable assignment in conditions (walrus operator style via temp var) ===

# Test value propagation through elif chain
p = 0
temp = 10
if temp > 20:
    p = 1
elif temp > 5:
    p = 2
elif temp > 0:
    p = 3
else:
    p = 4
assert p == 2, 'second condition matches temp=10'
