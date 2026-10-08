# Oils spec gaps closed together: `${##}` forms, NUL truncation, debug
# stack arrays, temp-env dynamic unset, `$[ ]`, posix special builtins,
# xtrace of assignment builtins, regex bracket quoting and more.
# Each expectation was checked against bash 5.2.

### hash_param_with_removal_ops
set -- $(seq 25)
echo ${###} ${####} ${##2} ${###2} ${##}
### expect
25 25 5 5 2
### end

### length_with_operator_is_bad_substitution
set -- '####'
echo ${#1#'###'}; echo not reached
echo next $?
### expect
next 1
### end

### array_name_tests_element_zero
a=("")
echo "[${a-unset}] [${a:-empty}]"
a=()
echo "[${a-unset}]"
### expect
[] [empty]
[unset]
### end

### unquoted_star_null_test_ignores_ifs
set -- "" ""
IFS=
echo argv=${*:-minus} x${*:+plus}
### expect
argv= xplus
### end

### dollar_single_quote_nul_ends_string
nul=$'\0'
echo ${#nul} $'ab\0cd'
test -n $'\0'; echo status=$?
printf $'x\0y' | od -An -c
### expect
0 ab
status=1
   x
### end

### read_drops_nul_and_mapfile_truncates
printf '.\000.\n' | { read s; echo len=${#s}; }
printf '.\000.\n' | { mapfile L; echo -n "${L[0]}" | od -An -tx1; }
### expect
len=2
 2e
### end

### debug_stack_arrays
g() { echo "${FUNCNAME[*]}|${BASH_LINENO[*]}"; }
f() {
  g
}
f
echo "${FUNCNAME[*]-none}|${#BASH_LINENO[@]}"
### expect
g f|3 5
none|0
### end

### unset_without_f_unsets_function
f() { echo foo; }
unset f
f 2>/dev/null
echo status=$?
### expect
status=127
### end

### unset_assoc_key_and_arith_index
declare -A d=()
key='1],a[1'
d["$key"]=foo
unset -v 'd["$key"]'
echo ${#d[@]}
a=(w x y z)
i=1
unset 'a[ i - 1 ]' a[-1]
echo "${a[@]}"
### expect
0
x y
### end

### readonly_assignment_in_eval_and_errexit
f() {
  local x=1
  readonly x
  eval 'x=2' 2>/dev/null
  echo status=$?
}
f
set -e
readonly r=1
r=2 2>/dev/null
echo not reached
### exit_code: 1
### expect
status=1
### end

### eval_and_source_end_aborted_body
eval 'echo $((1/0)); echo no' 2>/dev/null
echo after $?
### expect
after 1
### end

### unquoted_assignment_like_argument_splits
x='a b'
printf '<%s>' v=$x; echo
export y=$x
echo "$y"
e=export
$e z=$x
echo "$z"
### expect
<v=a><b>
a b
a
### end

### dollar_bracket_arithmetic
i=2
echo $[i + 1] "$[i * 3]"
### expect
3 6
### end

### tempenv_dynamic_unset
f() { unset v; echo "v=${v-(unset)}"; }
v=global
v=tempenv f
echo "v=$v"
g() { local v; echo "v=${v-(unset)}"; }
v=tempenv g
### expect
v=global
v=global
v=tempenv
### end

### local_hides_exported_value
f() { local v; echo "v=${v-(unset)}"; }
(export v=global; f)
### expect
v=(unset)
### end

### kill_l_lists_several
kill -l 0 10 USR2 SIGSEGV
### expect
EXIT
USR1
12
11
### end

### pushd_popd_dirs_usage
pushd -z 2>/dev/null; echo $?
pushd . . 2>/dev/null; echo $?
popd zzz 2>/dev/null; echo $?
dirs a 2>/dev/null; echo $?
### expect
2
1
2
2
### end

### xtrace_assignment_builtins
{
set -x
readonly x=3
declare -a a+=(2)
(( a = 4 ))
set +x
} 2>&1
### expect
+ readonly x=3
+ x=3
+ a+=('2')
+ declare -a a
+ ((  a = 4  ))
+ set +x
### end

### regex_brace_and_quoted_bracket
[[ { =~ "{" ]] && echo brace
[[ b =~ ["a-z"] ]] && echo range
[[ '|' =~ $'|' ]] && echo ansi
[[ "(" =~ ^([][{}\(\)^@]) ]] && echo lisp
### expect
brace
range
ansi
lisp
### end

### posix_special_builtins
set -o posix
foo=bar :
echo foo=$foo
eval 'echo hi'
### expect
foo=bar
hi
### end

### pwd_variable_is_ordinary
cd /
PWD=foo
echo $PWD
cd /
echo $PWD
### expect
foo
/
### end

### exec_options
exec -- echo hi
### expect
hi
### end

### trap_exit_handler_exit_status
trap 'exit 42' EXIT
### exit_code: 42
### expect
### end

### indirect_attribute_transform
array=(1 2)
r=array
echo "[${!r@a}]"
### expect
[a]
### end

### test_mode_bits
touch su; chmod u+s su
test -u su; echo $?
test -g su; echo $?
test -c /dev/null; echo $?
test -O su; echo $?
### expect
0
1
0
0
### end
