# Test that returning a cyclic dict doesn't crash (MontyObject cycle detection)
d = {}
d['self'] = d
d
# Return={'self': {...}}
