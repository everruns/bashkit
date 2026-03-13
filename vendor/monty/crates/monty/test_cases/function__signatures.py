# === Basic functions ===
def simple(a, b, c):
    return a + b + c


assert simple(1, 2, 3) == 6, 'simple function'
assert simple(10, 20, 30) == 60, 'simple function with larger values'


# === Positional-only parameters ===
def pos_only(a, b, /, c):
    return a + b + c


assert pos_only(1, 2, 3) == 6, 'positional-only params'
assert pos_only(5, 5, 5) == 15, 'positional-only all same'
assert pos_only(5, 5, c=5) == 15, 'positional-only all same'


# === All positional-only ===
def all_pos_only(a, b, c, /):
    return a + b + c


assert all_pos_only(1, 2, 3) == 6, 'all positional-only'


# === Multiple parameter groups ===
def multi_group(a, /, b, c):
    return f'a={a} b={b} c={c}'


assert multi_group(1, 2, 3) == 'a=1 b=2 c=3', 'mixed positional-only and regular'
assert multi_group(1, b=2, c=3) == 'a=1 b=2 c=3', 'mixed positional-only and regular'
assert multi_group(1, c=3, b=2) == 'a=1 b=2 c=3', 'mixed positional-only and regular'


# === Call-site *args unpacking ===
def collect_all(*values):
    return values


source_tuple = (1, 2, 3)
assert collect_all(*source_tuple) == (1, 2, 3), 'tuple unpacked with *args'

source_list = [4, 5]
assert collect_all(0, *source_list) == (0, 4, 5), 'positional args followed by *args'
