# === Boolean 'and' operator ===
# returns first falsy value, or last value if all truthy
assert (5 and 3) == 3, 'and truthy'
assert (0 and 3) == 0, 'and falsy'
assert (1 and 2 and 3) == 3, 'and chained'

# === Boolean 'or' operator ===
# returns first truthy value, or last value if all falsy
assert (5 or 3) == 5, 'or truthy'
assert (0 or 3) == 3, 'or falsy'
assert (0 or 0 or 3) == 3, 'or chained'

# === Boolean 'not' operator ===
assert (not 5) == False, 'not truthy'
assert (not 0) == True, 'not falsy'
assert (not None) == True, 'not None'

# === Complex boolean expressions ===
assert ((1 and 2) or (3 and 0)) == 2, 'complex and/or'
assert (not (0 and 1)) == True, 'not and combined'
