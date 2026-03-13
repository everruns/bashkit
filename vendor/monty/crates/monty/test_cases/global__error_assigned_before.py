def f():
    x = 1
    global x  # type: ignore[reportAssignmentBeforeGlobalDeclaration]


f()
# Raise=SyntaxError("name 'x' is assigned to before global declaration")
