### func_keyword
# Function with keyword syntax
function greet { echo hello; }; greet
### expect
hello
### end

### func_posix
# Function with POSIX syntax
greet() { echo hello; }; greet
### expect
hello
### end

### func_args
# Function with arguments
greet() { echo "Hello $1"; }; greet World
### expect
Hello World
### end

### func_multiple_args
# Function with multiple arguments
show() { echo $1 $2 $3; }; show a b c
### expect
a b c
### end

### func_arg_count
# Function argument count
count() { echo $#; }; count a b c d e
### expect
5
### end

### func_all_args
# Function with $@
all() { echo "$@"; }; all one two three
### expect
one two three
### end

### func_return
# Function with return value
check() { return 0; }; check && echo success
### expect
success
### end

### func_return_fail
# Function with non-zero return
check() { return 1; }; check || echo failed
### expect
failed
### end

### func_local
# Function with local variable
outer=global
test_local() { local outer=local; echo $outer; }
test_local; echo $outer
### expect
local
global
### end

### func_nested_call
# Nested function calls
inner() { echo inner; }
outer() { inner; echo outer; }
outer
### expect
inner
outer
### end

### func_recursive
# Recursive function
countdown() {
  if [ $1 -le 0 ]; then return; fi
  echo $1
  countdown $(($1 - 1))
}
countdown 3
### expect
3
2
1
### end

### func_modify_global
# Function modifying global variable
X=old
modify() { X=new; }
modify; echo $X
### expect
new
### end

### func_output_capture
# Capture function output
get_value() { echo 42; }
result=$(get_value)
echo "Result: $result"
### expect
Result: 42
### end

### func_in_pipeline
# Function in pipeline
produce() { echo "a"; echo "b"; echo "c"; }
produce | cat
### expect
a
b
c
### end
