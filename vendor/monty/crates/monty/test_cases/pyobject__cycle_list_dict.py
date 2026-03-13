# Test composite cycle: list containing dict containing original list
c = []
e = {'list': c}
c.append(e)
c
# Return=[{'list': [...]}]
