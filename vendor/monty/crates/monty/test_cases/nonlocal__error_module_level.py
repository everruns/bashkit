# nonlocal at module level is a syntax error
nonlocal x  # type: ignore
# Raise=SyntaxError('nonlocal declaration not allowed at module level')
