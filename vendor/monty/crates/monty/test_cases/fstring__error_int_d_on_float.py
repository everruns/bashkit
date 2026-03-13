# integer format specifier ':d' on float raises ValueError
f'{3.14:d}'
# Raise=ValueError("Unknown format code 'd' for object of type 'float'")
