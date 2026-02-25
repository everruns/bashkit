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

### func_local_nested_write
# Writes in nested function update local frame (dynamic scoping)
outer() {
  local x=outer
  inner
  echo "after inner: $x"
}
inner() {
  x=modified
}
outer
### expect
after inner: modified
### end

### func_local_no_value
# local x (no value) initializes to empty (bash behavior)
x=global
wrapper() {
  local x
  echo "in wrapper: [$x]"
}
wrapper
echo "after: $x"
### expect
in wrapper: []
after: global
### end

### func_local_assign_stays_local
# local declaration + assignment stays local
x=global
outer() {
  local x
  x=local_val
  echo "in outer: $x"
}
outer
echo "after outer: $x"
### expect
in outer: local_val
after outer: global
### end

### func_funcname_basic
# FUNCNAME[0] is current function name
myfunc() { echo "${FUNCNAME[0]}"; }
myfunc
### expect
myfunc
### end

### func_funcname_call_stack
# FUNCNAME array reflects call stack
inner() { echo "${FUNCNAME[@]}"; }
outer() { inner; }
outer
### expect
inner outer
### end

### func_funcname_depth
# FUNCNAME array length matches nesting depth
a() { echo "${#FUNCNAME[@]}"; }
b() { a; }
c() { b; }
c
### expect
3
### end

### func_funcname_empty_outside
# FUNCNAME is empty outside functions
echo "${#FUNCNAME[@]}"
### expect
0
### end

### func_funcname_restored
# FUNCNAME is cleared after function returns
f() { echo "in: ${FUNCNAME[0]}"; }
f
echo "out: ${#FUNCNAME[@]}"
### expect
in: f
out: 0
### end

### func_caller_in_function
# caller reports calling context
### bash_diff
f() { caller 0; }
f
### expect
1 main main
### end

### func_caller_nested
# caller 0 reports immediate caller
### bash_diff
inner() { caller 0; }
outer() { inner; }
outer
### expect
1 outer main
### end

### func_caller_outside
# caller outside function returns error
caller 0
echo "exit:$?"
### expect
exit:1
### end

### func_caller_no_args
# caller with no args works same as caller 0
### bash_diff
f() { caller; }
f
### expect
1 main main
### end
