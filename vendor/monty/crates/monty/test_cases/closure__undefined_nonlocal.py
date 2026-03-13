# accessing nonlocal before assignment should raise NameError
def outer():
    def inner():
        nonlocal x
        return x  # x not yet defined

    result = inner()
    x = 10
    return result


outer()
# Raise=NameError("cannot access free variable 'x' where it is not associated with a value in enclosing scope")
