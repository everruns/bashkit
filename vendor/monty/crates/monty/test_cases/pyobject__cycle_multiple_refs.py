# Test multiple references to the same cyclic object
f = []
f.append(f)
g = [f, f]
g
# Return=[[[...]], [[...]]]
