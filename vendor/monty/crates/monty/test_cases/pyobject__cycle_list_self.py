# Test that returning a cyclic list doesn't crash (MontyObject cycle detection)
a = []
a.append(a)
a
# Return=[[...]]
