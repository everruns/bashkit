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

### var_substring
# Substring extraction with offset and length
X=hello_world; echo ${X:0:5}
### expect
hello
### end

### var_substring_offset
# Substring with offset only
X=hello_world; echo ${X:6}
### expect
world
### end

### var_substring_middle
# Substring from middle
X=hello_world; echo ${X:2:5}
### expect
llo_w
### end

### var_replace_first
# Pattern replacement first occurrence
X=hello_world; echo ${X/o/0}
### expect
hell0_world
### end

### var_replace_all
# Pattern replacement all occurrences
X=hello_world; echo ${X//o/0}
### expect
hell0_w0rld
### end

### var_replace_word
# Pattern replacement with word
X=hello_world; echo ${X/world/bash}
### expect
hello_bash
### end

### var_uppercase_all
# Uppercase all characters
X=hello; echo ${X^^}
### expect
HELLO
### end

### var_uppercase_first
# Uppercase first character
X=hello; echo ${X^}
### expect
Hello
### end

### var_lowercase_all
# Lowercase all characters
X=HELLO; echo ${X,,}
### expect
hello
### end

### var_lowercase_first
# Lowercase first character
X=HELLO; echo ${X,}
### expect
hELLO
### end

### var_indirect
# Indirect variable expansion
A=hello; B=A; echo ${!B}
### expect
hello
### end

### var_indirect_nested
# Indirect with nested variable
X=value; ref=X; echo ${!ref}
### expect
value
### end

### prefix_assign_visible_in_env
# Prefix assignment visible to command via printenv
MYVAR=hello printenv MYVAR
### expect
hello
### end

### prefix_assign_multiple
# Multiple prefix assignments visible to command
A=one B=two printenv A
### expect
one
### end

### prefix_assign_temporary
# Prefix assignment does not persist after command
TMPVAR=gone printenv TMPVAR; echo ${TMPVAR:-unset}
### expect
gone
unset
### end

### prefix_assign_no_clobber
# Prefix assignment does not overwrite pre-existing var permanently
X=original; X=temp echo done; echo $X
### expect
done
original
### end

### prefix_assign_empty_value
# Prefix assignment with empty value is still set (exit code 0)
MYVAR= printenv MYVAR > /dev/null; echo $?
### expect
0
### end

### prefix_assign_only_no_command
# Assignment without command persists (not a prefix assignment)
PERSIST=yes; echo $PERSIST
### expect
yes
### end
