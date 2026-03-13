# Tests for default parameter values in function definitions

# === Basic default values ===
def f_basic(a, b=10):
    return a + b


assert f_basic(1) == 11, 'default used'
assert f_basic(1, 2) == 3, 'default overridden'
assert f_basic(5) == 15, 'default used again'


# === Multiple defaults ===
def f_multi(a=1, b=2):
    return a + b


assert f_multi() == 3, 'both defaults'
assert f_multi(10) == 12, 'first provided'
assert f_multi(10, 20) == 30, 'both provided'


# === Mixed required and default ===
def f_mixed(a, b, c=3, d=4):
    return a + b + c + d


assert f_mixed(1, 2) == 10, 'required only'
assert f_mixed(1, 2, 30) == 37, 'one default overridden'
assert f_mixed(1, 2, 30, 40) == 73, 'all provided'


# === Default with keyword args ===
def f_kw(a, b=10):
    return a + b


assert f_kw(1, b=20) == 21, 'keyword override'
assert f_kw(a=5) == 15, 'keyword required, default used'
assert f_kw(a=5, b=3) == 8, 'both keywords'


# === Default expressions evaluated at definition ===
# Test that default is evaluated once at definition time
def value_maker():
    return 42


def f_eval(x=value_maker()):
    return x


# value_maker was called once at function definition time
assert f_eval() == 42, 'first call uses cached default'
assert f_eval() == 42, 'second call uses same default'


# === Mutable default (Python gotcha - shared across calls) ===
def f_mutable(lst=[]):
    lst.append(1)
    return lst


first_result = f_mutable()
assert first_result == [1], 'first call'
second_result = f_mutable()
assert second_result == [1, 1], 'second call appends to same list'
assert first_result is second_result, 'same list object'


# === Multiple functions with separate defaults ===
def f_sep1(x=[]):
    x.append('a')
    return x


def f_sep2(x=[]):
    x.append('b')
    return x


r1 = f_sep1()
r2 = f_sep2()
assert r1 == ['a'], 'f_sep1 default'
assert r2 == ['b'], 'f_sep2 default'
assert r1 is not r2, 'separate default lists'


# === Default referencing earlier param (not supported, different test) ===


# === Closure with defaults ===
def make_adder(n):
    def add(x, y=n):
        return x + y

    return add


add5 = make_adder(5)
assert add5(10) == 15, 'closure default from enclosing scope'
assert add5(10, 3) == 13, 'closure default overridden'

add10 = make_adder(10)
assert add10(1) == 11, 'different closure, different captured default'

# Verify the two closures have independent defaults
assert add5(1) == 6, 'add5 still uses 5'


# === Keyword-only defaults interleaved ===
def kwonly_mix(*, head=1, mid, tail=3):
    return head, mid, tail


assert kwonly_mix(mid=2) == (1, 2, 3), 'kw-only defaults applied per parameter'
assert kwonly_mix(head=5, mid=7) == (5, 7, 3), 'kw-only default overridden independently'
