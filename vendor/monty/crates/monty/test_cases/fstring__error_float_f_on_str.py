# float format specifier ':f' on string raises ValueError
f'{"hello":f}'
# Raise=ValueError("Unknown format code 'f' for object of type 'str'")
