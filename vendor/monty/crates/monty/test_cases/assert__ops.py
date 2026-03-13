# Tests for assert statements that pass (failure cases are in separate files)
# === Basic assert ===
assert True, 'basic assert True'

# === Assert with expression ===
assert 1 == 1, 'assert equality expression'

# === Assert with function call style (assert is statement, not function) ===
# fmt: off
assert(123)
# fmt: on
