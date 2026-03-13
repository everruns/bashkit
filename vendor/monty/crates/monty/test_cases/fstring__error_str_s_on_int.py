# string format specifier ':s' on integer raises ValueError
f'{42:s}'
# Raise=ValueError("Unknown format code 's' for object of type 'int'")
