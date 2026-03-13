# === Basic f-strings ===
assert f'hello' == 'hello', 'basic f-string'
assert f'' == '', 'empty f-string'

# === Simple interpolation ===
x = 'world'
assert f'hello {x}' == 'hello world', 'simple interpolation'

# multiple interpolations
a = 1
b = 2
assert f'{a} + {b} = {a + b}' == '1 + 2 = 3', 'multiple interpolations'

# expression in f-string
assert f'{1 + 2 + 3}' == '6', 'expression'

# === Value types ===
# list value
x = [1, 2, 3]
assert f'list: {x}' == 'list: [1, 2, 3]', 'list value'

# bool value
x = True
assert f'value: {x}' == 'value: True', 'bool value'

# int value
assert f'{42}' == '42', 'int value'

# float value
assert f'{3.14}' == '3.14', 'float value'

# None value
assert f'{None}' == 'None', 'None value'

# === Conversion flags (!s, !r, !a) ===
# conversion !s (str)
assert f'{42!s}' == '42', 'conversion !s'

# conversion !r (repr)
assert f'{"hello"!r}' == "'hello'", 'conversion !r'

# conversion !r on int (should be same as str for int)
assert f'{42!r}' == '42', 'conversion !r on int'

# conversion !r on list
assert f'{[1, 2]!r}' == '[1, 2]', 'conversion !r on list'

# conversion !s on string (no quotes)
assert f'{"hello"!s}' == 'hello', 'conversion !s on string'

# conversion !a (ascii) - escapes non-ASCII characters
assert f'{"café"!a}' == "'caf\\xe9'", 'conversion !a'
assert f'{"hello"!a}' == "'hello'", 'conversion !a ascii only'
assert f'{"日本"!a}' == "'\\u65e5\\u672c'", 'conversion !a unicode'

# === String padding and alignment ===
# format spec: width (left-aligned by default for strings)
assert f'{"hi":10}' == 'hi        ', 'format width'

# format spec: left align
assert f'{"hi":<10}' == 'hi        ', 'format left align'

# format spec: right align
assert f'{"hi":>10}' == '        hi', 'format right align'

# format spec: center align
assert f'{"hi":^10}' == '    hi    ', 'format center align'

# center align with odd padding
assert f'{"zip":^6}' == ' zip  ', 'format center align odd'

# format spec: fill character
assert f'{"hi":*>10}' == '********hi', 'format fill right'
assert f'{"hi":_<10}' == 'hi________', 'format fill left'
assert f'{"hi":*^10}' == '****hi****', 'format fill center'

# string truncation with precision
assert f'{"xylophone":.5}' == 'xylop', 'string truncation'
assert f'{"xylophone":10.5}' == 'xylop     ', 'string truncation with width'

# === Integer formatting ===
# basic integer
assert f'{42}' == '42', 'basic integer'

# integer with :d type
assert f'{42:d}' == '42', 'integer :d'

# integer padding
assert f'{42:4d}' == '  42', 'integer padding'
assert f'{42:04d}' == '0042', 'integer zero padding'

# integer with sign
assert f'{42:+d}' == '+42', 'integer positive sign'
assert f'{42: d}' == ' 42', 'integer space for positive'
assert f'{-42:+d}' == '-42', 'integer negative with sign'
assert f'{-42: d}' == '-42', 'integer negative space'

# sign-aware padding
assert f'{-23:=5d}' == '-  23', 'sign-aware padding'

# === Float formatting ===
# basic float
assert f'{3.14159}' == '3.14159', 'basic float'

# float with :f type
assert f'{3.141592653589793:f}' == '3.141593', 'float :f'

# float precision
assert f'{3.141592653589793:.2f}' == '3.14', 'float precision'
assert f'{3.141592653589793:.4f}' == '3.1416', 'float precision 4'

# float width and precision
assert f'{3.141592653589793:06.2f}' == '003.14', 'float zero pad with precision'
assert f'{3.141592653589793:10.2f}' == '      3.14', 'float width with precision'

# float with sign
assert f'{3.14:+.2f}' == '+3.14', 'float positive sign'
assert f'{-3.14:+.2f}' == '-3.14', 'float negative with sign'
assert f'{3.14:-.2f}' == '3.14', 'float explicit minus sign'
assert f'{-3.14:-.2f}' == '-3.14', 'float explicit minus sign negative'

# exponential notation
assert f'{1234.5678:e}' == '1.234568e+03', 'exponential lowercase'
assert f'{1234.5678:E}' == '1.234568E+03', 'exponential uppercase'
assert f'{1234.5678:.2e}' == '1.23e+03', 'exponential with precision'
assert f'{0.00012345:.2e}' == '1.23e-04', 'exponential small number'

# general format (g/G) - uses exponential for very large/small numbers
assert f'{1.5:g}' == '1.5', 'general format simple'
assert f'{1.500:g}' == '1.5', 'general format strips trailing zeros'
assert f'{1234567890:g}' == '1.23457e+09', 'general format large number'

# percentage
assert f'{0.25:%}' == '25.000000%', 'percentage default precision'
assert f'{0.25:.1%}' == '25.0%', 'percentage with precision'
assert f'{0.125:.0%}' == '12%', 'percentage zero precision'

# === Nested format specs ===
width = 10
assert f'{"hi":{width}}' == 'hi        ', 'nested format spec width'

# nested alignment and width
align = '^'
assert f'{"test":{align}{width}}' == '   test   ', 'nested align and width'

# nested precision
prec = 3
assert f'{"xylophone":.{prec}}' == 'xyl', 'nested precision'


# === f-string in function ===
def greet(name):
    return f'Hello, {name}!'


assert greet('World') == 'Hello, World!', 'f-string in function'


# function returning formatted value
def format_num(n, w):
    return f'{n:>{w}}'


assert format_num('x', 5) == '    x', 'f-string with params'

# === Escaping ===
# double braces to escape
assert f'{{}}' == '{}', 'escaped braces'
assert f'{{x}}' == '{x}', 'escaped braces with content'
assert f'{{{42}}}' == '{42}', 'value inside escaped braces'

# === Complex expressions ===
# TODO: method call on literal - parser doesn't support this yet
# assert f'{"hello".upper()}' == 'HELLO', 'method call on literal'

# TODO: method call on variable - str.upper() not implemented yet
# s = 'hello'
# assert f'{s.upper()}' == 'HELLO', 'method call on variable'

# subscript in f-string
lst = [10, 20, 30]
assert f'{lst[1]}' == '20', 'subscript'

# dict lookup
d = {'a': 1, 'b': 2}
assert f'{d["a"]}' == '1', 'dict lookup'

# TODO: conditional expression - parser doesn't support IfExp yet
# x = 5
# assert f'{x if x > 0 else -x}' == '5', 'conditional positive'
# x = -5
# assert f'{-x if x < 0 else x}' == '5', 'conditional negative'

# === String concatenation ===
name = 'world'
# regular string + f-string (implicit concatenation)
assert f'hello {name}' == 'hello world', 'str concat with fstring'

# === Empty interpolation expression ===
# (this should be a syntax error, but test current behavior)
# assert f'{}' would be syntax error

# === Whitespace in format spec ===
# no extra whitespace handling needed, width handles it
assert f'{"x":5}' == 'x    ', 'single char width'

# === Unicode character counting in padding ===
x = 'café'
assert f'{x:_<10}' == 'café______'
assert f'{x:_>10}' == '______café'
assert f'{x:_^10}' == '___café___'
assert f'{x:_^11}' == '___café____'
assert f'{x:é<10}' == 'cafééééééé'
assert f'{x:é>10}' == 'éééééécafé'
assert f'{x:é^10}' == 'ééécaféééé'
assert f'{x:é^11}' == 'ééécafééééé'

# === Conversion flag with type spec ===
# conversion flag produces string, so 's' format should work
assert f'{42!r:s}' == '42', 'conversion with type spec'

# === Zero-padding with negative numbers ===
# zero-padding should use sign-aware alignment
x = -42
assert f'{x:05d}' == '-0042', 'zero pad negative'

# === Debug/self-documenting expressions (=) ===
a = 42
assert f'{a=}' == 'a=42', 'basic debug expression'
assert f'{a = }' == 'a = 42', 'debug with spaces'
name = 'test'
assert f'{name=}' == "name='test'", 'debug uses repr for strings'
assert f'{name = }' == "name = 'test'", 'debug uses repr for strings'
assert f'{name=!s}' == 'name=test', 'debug with !s conversion'
assert f'{name=!r}' == "name='test'", 'debug with !r conversion'
assert f'{1+1=}' == '1+1=2', 'debug with expression'
