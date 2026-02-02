### var_simple
# Simple variable assignment and expansion
FOO=bar; echo $FOO
### expect
bar
### end

### var_braces
# Variable with braces
FOO=hello; echo ${FOO}
### expect
hello
### end

### var_undefined
# Undefined variable expands to empty
echo "x${UNDEFINED}y"
### expect
xy
### end

### var_multiple
# Multiple variables
A=1; B=2; C=3; echo $A $B $C
### expect
1 2 3
### end

### var_in_string
# Variable in double-quoted string
NAME=world; echo "hello $NAME"
### expect
hello world
### end

### var_no_expand_single
# Single quotes prevent expansion
NAME=world; echo 'hello $NAME'
### expect
hello $NAME
### end

### var_adjacent
# Adjacent variable and text
FOO=bar; echo ${FOO}baz
### expect
barbaz
### end

### var_default
# Default value when unset
echo ${UNSET:-default}
### expect
default
### end

### var_default_set
# Default not used when set
X=value; echo ${X:-default}
### expect
value
### end

### var_assign_default
# Assign default when unset
echo ${NEW:=assigned}; echo $NEW
### expect
assigned
assigned
### end

### var_length
# String length
X=hello; echo ${#X}
### expect
5
### end

### var_remove_prefix
# Remove shortest prefix
X=hello.world.txt; echo ${X#*.}
### expect
world.txt
### end

### var_remove_prefix_longest
# Remove longest prefix
X=hello.world.txt; echo ${X##*.}
### expect
txt
### end

### var_remove_suffix
# Remove shortest suffix
X=file.tar.gz; echo ${X%.*}
### expect
file.tar
### end

### var_remove_suffix_longest
# Remove longest suffix
X=file.tar.gz; echo ${X%%.*}
### expect
file
### end

### var_positional_1
# Positional parameter $1 in function
greet() { echo "Hello $1"; }; greet World
### expect
Hello World
### end

### var_positional_count
# Argument count $#
count() { echo $#; }; count a b c
### expect
3
### end

### var_positional_all
# All arguments $@
show() { echo "$@"; }; show one two three
### expect
one two three
### end

### var_special_question
# Exit code $?
true; echo $?
### expect
0
### end

### var_special_question_fail
# Exit code after failure
false; echo $?
### expect
1
### end

### var_special_pid
# Process ID $$ is a number
test -n "$$" && echo "ok"
### expect
ok
### end

### var_special_lineno
# Line number $LINENO
echo $LINENO
### expect
1
### end

### var_special_random
# Random number is set
test -n "$RANDOM" && echo "ok"
### expect
ok
### end

### tilde_home
# Tilde expands to HOME
HOME=/home/user; echo ~
### expect
/home/user
### end

### tilde_in_path
# Tilde at start of path
HOME=/home/user; echo ~/subdir
### expect
/home/user/subdir
### end

### tilde_not_in_middle
# Tilde not expanded in middle of word
HOME=/home/user; echo a~b
### expect
a~b
### end
