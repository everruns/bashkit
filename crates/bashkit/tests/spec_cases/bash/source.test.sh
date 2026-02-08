### source_loads_function
# Source a file and call its function
echo 'greet() { echo "hello world"; }' > /tmp/lib.sh
source /tmp/lib.sh
greet
### expect
hello world
### end

### dot_loads_function
# Dot command loads function into scope
echo 'greet() { echo "dotted"; }' > /tmp/lib.sh
. /tmp/lib.sh
greet
### expect
dotted
### end

### source_loads_variable
# Source a file that sets a variable
echo 'MY_VAR=loaded' > /tmp/vars.sh
source /tmp/vars.sh
echo $MY_VAR
### expect
loaded
### end

### source_keyword_function
# Source file with keyword function syntax
echo 'function myfunc { echo "keyword"; }' > /tmp/lib.sh
source /tmp/lib.sh
myfunc
### expect
keyword
### end

### source_multiple_functions
# Source file with multiple function definitions
echo 'add() { echo $(( $1 + $2 )); }' > /tmp/lib.sh
echo 'sub() { echo $(( $1 - $2 )); }' >> /tmp/lib.sh
source /tmp/lib.sh
add 3 2
sub 10 4
### expect
5
6
### end

### source_function_with_args
# Sourced function receives arguments
echo 'greet() { echo "Hello $1"; }' > /tmp/lib.sh
source /tmp/lib.sh
greet World
### expect
Hello World
### end

### source_function_return
# Sourced function with return value
echo 'check() { return 0; }' > /tmp/lib.sh
source /tmp/lib.sh
check && echo success
### expect
success
### end

### source_function_calls_function
# Sourced function calls another sourced function
echo 'inner() { echo "inner"; }' > /tmp/lib.sh
echo 'outer() { inner; echo "outer"; }' >> /tmp/lib.sh
source /tmp/lib.sh
outer
### expect
inner
outer
### end

### source_overwrites_function
# Source overwrites previously defined function
myfunc() { echo "v1"; }
echo 'myfunc() { echo "v2"; }' > /tmp/lib.sh
source /tmp/lib.sh
myfunc
### expect
v2
### end

### source_chained
# Source file that sources another file
echo 'deep() { echo "deep"; }' > /tmp/inner.sh
echo 'source /tmp/inner.sh' > /tmp/outer.sh
source /tmp/outer.sh
deep
### expect
deep
### end

### source_from_function
# Source from within a function
echo 'helper() { echo "from helper"; }' > /tmp/lib.sh
load_lib() { source /tmp/lib.sh; }
load_lib
helper
### expect
from helper
### end

### source_positional_params
# Source with positional parameters
echo 'echo "arg1=$1 arg2=$2"' > /tmp/lib.sh
source /tmp/lib.sh hello world
### expect
arg1=hello arg2=world
### end

### source_param_count
# Source with $# parameter
echo 'echo "count=$#"' > /tmp/lib.sh
source /tmp/lib.sh a b c
### expect
count=3
### end

### source_missing_filename
# Source without filename argument returns non-zero
### bash_diff: error message wording differs
source
echo $?
### expect
1
### end

### source_nonexistent_file
# Source nonexistent file returns non-zero
### bash_diff: error message wording differs
source /nonexistent.sh
echo $?
### expect
1
### end

### source_empty_file
# Source empty file is a no-op
echo -n '' > /tmp/empty.sh
source /tmp/empty.sh
echo ok
### expect
ok
### end

### source_comments_only
# Source file with only comments
echo '# just a comment' > /tmp/comments.sh
source /tmp/comments.sh
echo ok
### expect
ok
### end

### source_vars_visible
# Variables set by source are visible in caller
echo 'COLOR=blue' > /tmp/config.sh
source /tmp/config.sh
echo $COLOR
### expect
blue
### end

### source_function_sees_vars
# Sourced function accesses caller's variables
NAME=world
echo 'greet() { echo "hello $NAME"; }' > /tmp/lib.sh
source /tmp/lib.sh
greet
### expect
hello world
### end
