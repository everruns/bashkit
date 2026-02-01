### External test cases imported from GNU Bash test patterns
### These tests verify BashKit compatibility with real-world bash patterns.

### arith_addition
# Basic arithmetic addition
echo $((1+1))
### expect
2
### end

### arith_multiplication
# Arithmetic multiplication
echo $((2*3))
### expect
6
### end

### arith_division
# Integer division
echo $((10/3))
### expect
3
### end

### arith_modulo
# Modulo operation
echo $((10%3))
### expect
1
### end

### arith_negative
# Negative number
echo $((-5))
### expect
-5
### end

### arith_exponentiation
### skip: exponentiation not implemented
# Exponentiation
echo $((2 ** 3))
### expect
8
### end

### var_simple
# Simple variable assignment and use
x=hello; echo $x
### expect
hello
### end

### var_default_unset
# Default value when unset
unset x; echo "${x:-default}"
### expect
default
### end

### var_default_set
# Default value when set
x=value; echo "${x:-default}"
### expect
value
### end

### var_length
# String length
x=hello; echo "${#x}"
### expect
5
### end

### array_single
### skip: array access not implemented
# Array single element access
arr=(a b c); echo "${arr[0]}"
### expect
a
### end

### array_all
### skip: array expansion not implemented
# Array all elements
arr=(a b c); echo "${arr[@]}"
### expect
a b c
### end

### quote_single
# Single quoted string
echo 'hello world'
### expect
hello world
### end

### quote_double
# Double quoted string
echo "hello world"
### expect
hello world
### end

### quote_variable
# Variable in double quotes
x=world; echo "hello $x"
### expect
hello world
### end

### quote_escape
# Escaped quote in string
echo "hello \"world\""
### expect
hello "world"
### end

### comsub_simple
# Command substitution
echo $(echo hello)
### expect
hello
### end

### comsub_assign
# Command substitution in assignment
x=$(echo world); echo $x
### expect
world
### end

### cond_true
# If with true
if true; then echo yes; fi
### expect
yes
### end

### cond_false_else
# If-else with false
if false; then echo yes; else echo no; fi
### expect
no
### end

### cond_numeric_test
# Numeric test in if
if [ 5 -gt 3 ]; then echo greater; fi
### expect
greater
### end

### loop_for_words
# For loop with word list
for i in a b c; do echo $i; done
### expect
a
b
c
### end

### loop_for_numbers
# For loop with numbers
for i in 1 2 3; do echo $i; done
### expect
1
2
3
### end

### loop_while
# While loop with counter
x=0; while [ $x -lt 3 ]; do echo $x; x=$((x+1)); done
### expect
0
1
2
### end

### case_exact
# Case with exact match
case foo in foo) echo match;; esac
### expect
match
### end

### case_multiple
# Case with multiple patterns
case bar in foo) echo foo;; bar) echo bar;; esac
### expect
bar
### end

### case_glob
# Case with glob pattern
case test in *est) echo glob;; esac
### expect
glob
### end

### func_simple
# Simple function
greet() { echo "Hello, $1"; }; greet World
### expect
Hello, World
### end

### func_arithmetic
# Function with arithmetic
add() { echo $(($1 + $2)); }; add 3 4
### expect
7
### end

### redir_null
# Redirect to /dev/null
echo test > /dev/null; echo $?
### expect
0
### end

### pipe_simple
# Simple pipe
echo hello | cat
### expect
hello
### end
