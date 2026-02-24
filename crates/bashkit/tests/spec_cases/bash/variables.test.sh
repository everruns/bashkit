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

### var_prefix_match
# ${!prefix*} - names of variables with given prefix
MYVAR1=a; MYVAR2=b; MYVAR3=c; echo ${!MYVAR*}
### expect
MYVAR1 MYVAR2 MYVAR3
### end

### var_subshell_isolation
# Subshell variable changes don't leak to parent
X=orig; (X=changed; echo $X); echo $X
### expect
changed
orig
### end

### var_pipestatus
# PIPESTATUS array tracks per-command exit codes
false | true | false; echo ${PIPESTATUS[0]}-${PIPESTATUS[1]}-${PIPESTATUS[2]}
### expect
1-0-1
### end

### var_trap_exit
# trap EXIT handler runs on script completion
trap 'echo goodbye' EXIT; echo hello
### expect
hello
goodbye
### end

### var_pipefail
# set -o pipefail returns rightmost non-zero exit code
set -o pipefail; false | true; echo $?
### expect
1
### end

### var_pipefail_all_success
# pipefail with all-success pipeline returns 0
set -o pipefail; true | true; echo $?
### expect
0
### end

### var_pipefail_last_fails
# pipefail with last command failing
set -o pipefail; true | false; echo $?
### expect
1
### end

### var_pipefail_disable
# set +o pipefail disables pipefail
set -o pipefail; set +o pipefail; false | true; echo $?
### expect
0
### end

### var_error_if_unset_set
# ${var:?message} succeeds when set
x=hello; echo ${x:?should not error}
### expect
hello
### end

### var_transform_quote
# ${var@Q} quotes for reuse
x='hello world'; echo ${x@Q}
### expect
'hello world'
### end

### var_transform_uppercase_all
# ${var@U} uppercases all
x=hello; echo ${x@U}
### expect
HELLO
### end

### var_transform_uppercase_first
# ${var@u} uppercases first char
x=hello; echo ${x@u}
### expect
Hello
### end

### var_transform_lowercase
# ${var@L} lowercases all
x=HELLO; echo ${x@L}
### expect
hello
### end

### var_transform_assign
# ${var@A} shows assignment form
x=hello; echo ${x@A}
### expect
x='hello'
### end

### var_line_continuation_dquote
# Backslash-newline inside double quotes is line continuation
echo "hel\
lo"
### expect
hello
### end

### var_line_continuation_dquote_var
# Line continuation in double-quoted variable assignment
x="abc\
def"; echo "$x"
### expect
abcdef
### end

### var_line_continuation_dquote_multi
# Multiple line continuations
echo "one\
two\
three"
### expect
onetwothree
### end

### var_line_continuation_unquoted
# Backslash-newline in unquoted context
echo hel\
lo
### expect
hello
### end

### var_line_continuation_preserves_other_escapes
# Backslash before non-newline in double quotes is preserved
echo "hello\tworld"
### expect
hello\tworld
### end

### var_pwd_set
# $PWD is set to current directory
### bash_diff
echo "$PWD" | grep -q "/" && echo "has_slash"
### expect
has_slash
### end

### var_home_set
# $HOME is set
### bash_diff
test -n "$HOME" && echo "home_set"
### expect
home_set
### end

### var_user_set
# $USER is set
### bash_diff
test -n "$USER" && echo "user_set"
### expect
user_set
### end

### var_hostname_set
# $HOSTNAME is set
### bash_diff
test -n "$HOSTNAME" && echo "hostname_set"
### expect
hostname_set
### end

### var_bash_version
# BASH_VERSION is set
### bash_diff
test -n "$BASH_VERSION" && echo "version_set"
### expect
version_set
### end

### var_bash_versinfo_array
# BASH_VERSINFO is an array with version parts
### bash_diff
echo "${#BASH_VERSINFO[@]}"
test -n "${BASH_VERSINFO[0]}" && echo "major_set"
### expect
6
major_set
### end

### var_uid_set
# $UID is set
### bash_diff
test -n "$UID" && echo "uid_set"
### expect
uid_set
### end

### var_seconds
# $SECONDS is set (always 0 in bashkit)
### bash_diff
test -n "$SECONDS" && echo "seconds_set"
### expect
seconds_set
### end

### var_pwd_updates_with_cd
# $PWD updates after cd
### bash_diff
mkdir -p /tmp/test_pwd_cd
cd /tmp/test_pwd_cd
echo "$PWD"
### expect
/tmp/test_pwd_cd
### end

### var_oldpwd_set_after_cd
# $OLDPWD is set after cd
### bash_diff
mkdir -p /tmp/test_oldpwd_cd
old="$PWD"
cd /tmp/test_oldpwd_cd
echo "$OLDPWD" | grep -q "/" && echo "oldpwd_set"
### expect
oldpwd_set
### end

### xtrace_stdout_unaffected
# set -x does not alter stdout
set -x
echo hello
### expect
hello
### end

### xtrace_multiple_stdout
# set -x does not alter stdout for multiple commands
set -x
echo one
echo two
### expect
one
two
### end

### xtrace_disable_stdout
# set +x properly disables tracing, stdout unaffected
set -x
echo traced
set +x
echo not_traced
### expect
traced
not_traced
### end

### xtrace_expanded_vars_stdout
# set -x with variables does not alter stdout
x=hello
set -x
echo $x
### expect
hello
### end

### xtrace_no_output_without_flag
# Without set -x, no trace output
echo hello
### expect
hello
### end
