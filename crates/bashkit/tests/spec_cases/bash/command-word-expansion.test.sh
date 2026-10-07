# Command word expansion: the command name undergoes the same field
# splitting as arguments, so "$@", $cmd, $(...) may yield several words
# (first is the name) or none (no command runs).

### cmdword_quoted_at_in_function
# "$@" as the command word runs the positional params as a command
g() { "$@"; }
g echo hi
### expect
hi
### end

### cmdword_quoted_at_with_trailing_args
set -- echo a b
"$@" c
### expect
a b c
### end

### cmdword_braced_at
set -- printf '%s|' x y
"${@}"
echo
### expect
x|y|
### end

### cmdword_unquoted_at
set -- echo a b
$@
### expect
a b
### end

### cmdword_quoted_at_keeps_field_with_space
# Each positional param stays one field: name "echo z" is not found
set -- "echo z"
"$@" 2>/dev/null
echo "rc=$?"
### expect
rc=127
### end

### cmdword_array_splat
cmd=(printf '%s-' a b)
"${cmd[@]}"
echo
### expect
a-b-
### end

### cmdword_unquoted_var_splits
c="echo hi there"
$c
### expect
hi there
### end

### cmdword_quoted_var_no_split
c="echo hi"
"$c" 2>/dev/null
echo "rc=$?"
### expect
rc=127
### end

### cmdword_command_substitution_splits
$(echo echo from sub)
### expect
from sub
### end

### cmdword_empty_quoted_at_is_no_command
set --
false
"$@"
echo "rc=$?"
### expect
rc=0
### end

### cmdword_empty_unquoted_var_is_no_command
x=
false
$x
echo "rc=$?"
### expect
rc=0
### end

### cmdword_empty_first_param_not_found
set -- "" a
"$@" 2>/dev/null
echo "rc=$?"
### expect
rc=127
### end

### cmdword_quoted_empty_var_not_found
x=
"$x" 2>/dev/null
echo "rc=$?"
### expect
rc=127
### end

### cmdword_at_calls_function
f() { echo "f:$#:$*"; }
set -- f one two
"$@"
### expect
f:2:one two
### end

### cmdword_at_with_redirect
set -- echo redirected
"$@" > /tmp/cmdword_out
cat /tmp/cmdword_out
### expect
redirected
### end

### word_hash_mid_word_is_literal
# `#` starts a comment only at the start of a word
echo a#b c# #d
x=1#2; echo "$x"
echo ${x}#tail
### expect
a#b c#
1#2
1#2#tail
### end
