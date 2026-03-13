# integer format specifier ':d' on string raises ValueError
f'{"hello":d}'
# Raise=ValueError("Unknown format code 'd' for object of type 'str'")
