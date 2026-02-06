### python3_hello_world
# Basic print statement
python3 -c "print('hello world')"
### expect
hello world
### end

### python_alias
# python command works same as python3
python -c "print('hello')"
### expect
hello
### end

### python3_expression_arithmetic
# Expression result displayed (REPL behavior)
python3 -c "2 + 3"
### expect
5
### end

### python3_expression_string
# String expression result
python3 -c "'hello'"
### expect
'hello'
### end

### python3_multiline
# Multiline script
python3 -c "x = 10
y = 20
print(x + y)"
### expect
30
### end

### python3_variables
# Variable assignment and use
python3 -c "name = 'world'
print(f'hello {name}')"
### expect
hello world
### end

### python3_for_loop
# For loop with range
python3 -c "for i in range(5):
    print(i)"
### expect
0
1
2
3
4
### end

### python3_while_loop
# While loop
python3 -c "i = 0
while i < 3:
    print(i)
    i += 1"
### expect
0
1
2
### end

### python3_function_def
# Function definition and call
python3 -c "def greet(name):
    return f'Hello, {name}!'
print(greet('Alice'))"
### expect
Hello, Alice!
### end

### python3_list_operations
# List creation and operations
python3 -c "nums = [3, 1, 4, 1, 5]
print(sorted(nums))
print(len(nums))"
### expect
[1, 1, 3, 4, 5]
5
### end

### python3_dict_basic
# Dictionary creation and access
python3 -c "d = dict()
d['a'] = 1
d['b'] = 2
print(d['a'])
print(len(d))"
### expect
1
2
### end

### python3_list_comprehension
# List comprehension
python3 -c "squares = [x**2 for x in range(5)]
print(squares)"
### expect
[0, 1, 4, 9, 16]
### end

### python3_conditional
# If/elif/else
python3 -c "x = 42
if x > 100:
    print('big')
elif x > 10:
    print('medium')
else:
    print('small')"
### expect
medium
### end

### python3_string_methods
# String methods
python3 -c "s = 'Hello, World!'
print(s.upper())
print(s.lower())
print(s.split(', '))"
### expect
HELLO, WORLD!
hello, world!
['Hello', 'World!']
### end

### python3_tuple_unpacking
# Tuple unpacking
python3 -c "a, b, c = 1, 2, 3
print(a, b, c)"
### expect
1 2 3
### end

### python3_try_except
# Exception handling
python3 -c "try:
    x = 1 / 0
except ZeroDivisionError:
    print('caught division by zero')"
### expect
caught division by zero
### end

### python3_nested_functions
# Nested function calls
python3 -c "def add(a, b):
    return a + b
def mul(a, b):
    return a * b
print(add(mul(2, 3), mul(4, 5)))"
### expect
26
### end

### python3_fibonacci
# Recursive fibonacci
python3 -c "def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)
print(fib(10))"
### expect
55
### end

### python3_multiple_prints
# Multiple print statements
python3 -c "print('one')
print('two')
print('three')"
### expect
one
two
three
### end

### python3_print_no_newline
# Print with end parameter
python3 -c "print('hello', end=' ')
print('world')"
### expect
hello world
### end

### python3_print_separator
# Print with sep parameter
python3 -c "print(1, 2, 3, sep='-')"
### expect
1-2-3
### end

### python3_boolean_logic
# Boolean operations
python3 -c "print(True and False)
print(True or False)
print(not True)"
### expect
False
True
False
### end

### python3_none_handling
# None printed as None
python3 -c "x = None
print(x)"
### expect
None
### end

### python3_version
# Version flag
python3 --version
### expect
Python 3.12.0 (monty)
### end

### python3_version_short
# Short version flag
python3 -V
### expect
Python 3.12.0 (monty)
### end

### python3_command_substitution
# Python in command substitution
result=$(python3 -c "print(6 * 7)")
echo "answer: $result"
### expect
answer: 42
### end

### python3_pipeline_output
# Python output in pipeline
python3 -c "for i in range(3):
    print(f'line {i}')" | grep "line 1"
### expect
line 1
### end

### python3_conditional_failure
# Python error triggers else branch
### bash_diff: real python not available
if python3 -c "1/0" 2>/dev/null; then echo "success"; else echo "fail"; fi
### expect
fail
### end

### python3_stdin_pipe
# Code from piped stdin
echo "print('from pipe')" | python3
### expect
from pipe
### end

### python3_enumerate
# Enumerate function
python3 -c "for i, c in enumerate('abc'):
    print(f'{i}:{c}')"
### expect
0:a
1:b
2:c
### end

### python3_zip
# Zip function
python3 -c "for a, b in zip([1,2,3], ['a','b','c']):
    print(f'{a}{b}')"
### expect
1a
2b
3c
### end

### python3_fstring_formatting
# f-string formatting (primary format method in Monty)
python3 -c "name = 'world'
print(f'hello {name}')"
### expect
hello world
### end

### python3_math_operations
# Math operations
python3 -c "print(2 ** 10)
print(17 // 3)
print(17 % 3)"
### expect
1024
5
2
### end

### python3_slice_operations
# List slicing
python3 -c "lst = [0, 1, 2, 3, 4, 5]
print(lst[1:4])
print(lst[::2])
print(lst[::-1])"
### expect
[1, 2, 3]
[0, 2, 4]
[5, 4, 3, 2, 1, 0]
### end

### python3_any_all
# any() and all() builtins
python3 -c "print(any([False, True, False]))
print(all([True, True, True]))
print(all([True, False, True]))"
### expect
True
True
False
### end

### python3_generator_expression
# Generator expression with sum
python3 -c "total = sum(x**2 for x in range(10))
print(total)"
### expect
285
### end

### python3_ternary
# Ternary expression
python3 -c "x = 42
result = 'even' if x % 2 == 0 else 'odd'
print(result)"
### expect
even
### end

### python3_star_args
# *args and **kwargs
python3 -c "def show(*args, **kwargs):
    print(args)
    print(kwargs)
show(1, 2, 3, x=4, y=5)"
### expect
(1, 2, 3)
{'x': 4, 'y': 5}
### end

### python3_set_operations
### skip: Monty does not support set & and | operators yet
# Set operations
python3 -c "a = {1, 2, 3}
b = {2, 3, 4}
print(sorted(a & b))
print(sorted(a | b))"
### expect
[2, 3]
[1, 2, 3, 4]
### end

### python3_map_filter
### skip: Monty does not have filter() builtin yet
# Map and filter
python3 -c "nums = [1, 2, 3, 4, 5]
evens = list(filter(lambda x: x % 2 == 0, nums))
print(evens)"
### expect
[2, 4]
### end

### python3_dict_comprehension
### skip: Monty does not support dict comprehension with enumerate yet
# Dictionary comprehension
python3 -c "d = {k: v for k, v in enumerate('abc')}
print(d)"
### expect
{0: 'a', 1: 'b', 2: 'c'}
### end

### python3_string_format_method
### skip: Monty does not support str.format() method yet
# str.format() method
python3 -c "print('hello {}'.format('world'))"
### expect
hello world
### end

### python3_sorted_with_key
### skip: Monty does not support sorted() key parameter yet
# sorted() with key function
python3 -c "words = ['banana', 'apple', 'cherry']
print(sorted(words, key=len))"
### expect
['apple', 'banana', 'cherry']
### end

### python3_chain_assignment
### skip: Monty does not support chain assignment (a = b = c = 0) yet
# Chain assignment
python3 -c "a = b = c = 0
print(a, b, c)"
### expect
0 0 0
### end

### python3_nested_dict_access
### skip: Monty dict literal in bash quoting needs single-quote support
# Nested data structures
python3 -c "data = {'users': [{'name': 'Alice'}]}
print(data['users'][0]['name'])"
### expect
Alice
### end
