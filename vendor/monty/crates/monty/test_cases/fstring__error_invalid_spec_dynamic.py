# invalid format specifier with dynamic spec
spec = 'xyz'
f'{1:{spec}}'
# Raise=ValueError("Invalid format specifier 'xyz' for object of type 'int'")
