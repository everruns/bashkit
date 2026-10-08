# Oils spec long tail: bash behaviors found by the upstream Oils suite
# (scripts/oils-spec). Each case was checked against real bash 5.2.

### empty_command_word_runs_first_argument
# `$empty cmd`: the name word vanishes and the next word is the command
p=
$p echo hi
seq 2 3 | $p cat
### expect
hi
2
3
### end

### pwd_and_oldpwd_are_exported
env | grep -c '^PWD='
cd /
cd /tmp
env | grep '^OLDPWD='
env | grep '^PWD='
### expect
1
OLDPWD=/
PWD=/tmp
### end

### rm_reports_each_operand_and_goes_on
touch foo bar
mkdir -p d
rm -- foo OOPS d bar 2>/dev/null
echo status=$?
test -f foo || echo foo-gone
test -f bar || echo bar-gone
rm -f
echo status=$?
### expect
status=1
foo-gone
bar-gone
status=0
### end

### cat_reads_stdin_once
seq 5 6 | cat - -
### expect
5
6
### end

### test_argument_count_rules
[ -a -a -a -a ]; echo $?
[ -a -a -a -a -a ]; echo $?
[ -a -a -a -a -a -a ] 2>/dev/null; echo $?
test '(' = ')'; echo $?
test 0 -eq 0 -a '(' = ')'; echo $?
set -- -o; test $# -ne 0 -a "$1" != "--"; echo $?
test x -a 2>/dev/null; echo $?
### expect
1
1
2
1
0
0
2
### end

### getopts_invalid_name_and_end_of_options
set -- -c foo -h
getopts 'hc:' opt- 2>/dev/null
echo status=$? OPTARG=$OPTARG OPTIND=$OPTIND
OPTIND=1
getopts "c:" opt -c10
getopts "c:" opt -c10
echo opt=$opt OPTARG=${OPTARG-unset}
### expect
status=1 OPTARG=foo OPTIND=3
opt=? OPTARG=unset
### end

### set_lone_plus_is_ignored
set + a b
echo "$@"
set - +
echo "$@"
set + -
echo "$@"
### expect
a b
+
+
### end

### nounset_in_arithmetic_is_fatal
(
set -u
(( x = 1 ))
echo $(( x + 1 ))
(( 0 && unset_name ))
echo ok
(( undef++ ))
echo not reached
) 2>/dev/null
echo status=$?
( set -u; y=$(( undef2 + 5 )); echo not reached ) 2>/dev/null
echo status=$?
### expect
2
ok
status=1
status=1
### end

### nounset_length_and_slice_are_fatal
( set -u; echo ${#undef}; echo no ) 2>/dev/null
echo status=$?
( set -u; echo ${undef:1:2}; echo no ) 2>/dev/null
echo status=$?
### expect
status=1
status=1
### end

### arithmetic_backticks_continuation_and_bases
echo $((`echo 1` + 2))
echo $((
1 +
2 + \
3
))
( echo $(( 02#0110 )) ) 2>/dev/null || echo invalid
### expect
3
6
invalid
### end

### empty_slice_offset_is_bad_substitution
( s=123; echo ${s:}; echo no ) 2>/dev/null
echo status=$?
### expect
status=1
### end

### brace_items_with_empty_quoted_suffix
printf '[%s]' {X,,Y,}''
echo
### expect
[X][][Y][]
### end

### printf_q_and_at_q_use_ansi_c_quoting
foo=$'a\nb\001c\'d'
printf '%q\n' "$foo"
echo ${foo@Q}
x=$'a\tb'
echo ${x@A}
printf '%q\n' $'\e\a'
### expect
$'a\nb\001c\'d'
$'a\nb\001c\'d'
x=$'a\tb'
$'\E\a'
### end

### ansi_c_control_and_invalid_unicode_escapes
echo -n $'\cA\cz\c-' | od -A n -t x1
echo $'\uZ' $'\u{03bc' $'\z'
### expect
 01 1a 0d
\uZ \u{03bc \z
### end

### braces_and_quotes_in_double_quoted_operands
echo "${var-}}"
echo "${var-\}}"
echo "${var-"}"}"
echo "${var-a \
b}"
x='}'
echo "[${x#\}}]" "[${x#"}"}]"
### expect
}
}
}
a b
[] []
### end

### function_names_bash_refuses_at_definition
func-name=ext ( ) { echo func-name=ext; }
func-name=ext
$foo-bar() { :; }
echo status=$?
### expect
func-name=ext
status=1
### end

### paren_after_command_word_is_syntax_error
bash -c 'echo a(b)
echo not reached' 2>/dev/null
echo "status $?"
### expect
status 2
### end

### date_operand_without_plus_is_an_error
date foo 2>/dev/null; echo "status $?"
### expect
status 1
### end

### history_negative_count_is_invalid_option
history -5 2>/dev/null; echo "status $?"
### expect
status 2
### end

### empty_array_subscript_fails
# bash abandons the rest of the line
a[]=x 2>/dev/null; echo unreached
echo "len ${#a[@]}"
### expect
len 0
### end

### unset_ps4_traces_without_prefix
unset PS4
set -x
echo hi 2>/dev/null
set +x 2>/dev/null
### expect
hi
### end

### time_keeps_pipeline_output
{ time echo hi | wc -c; } 2>/dev/null
### expect
3
### end

### array_prefix_binding_reaches_env_as_text
# bash binds the list text as a temporary scalar
f() { env | grep '^B='; echo "in: $B"; }
B=(b b) f
echo "after: ${B-unset}"
### expect
B=(b b)
in: (b b)
after: unset
### end

### nameref_export_flag
x=1
declare -nx r=x
env | grep '^r='
### expect
r=x
### end

### assoc_quoted_and_variable_keys
declare -A A=(['K']=val)
declare -n ref='A["K"]'
echo "$ref"
key=K
declare -n ref2='A[$key]'
ref2=new
echo "${A[K]}"
### expect
val
new
### end

### array_literal_subscripts_in_order
i=0
a=([i++]=x [i++]=y z)
echo "${a[@]} ${!a[@]}"
b=(~ [3]=~)
[ "${b[0]}" = "$HOME" ] && echo tilde-ok
declare -A h=(x y)
echo "assoc status $?"
### expect
x y z 0 1 2
tilde-ok
assoc status 0
### end

### command_sub_in_array_subscript
a=(1 '2 3')
echo "${a[$(echo 1)]}"
a[$(echo 0)]=Z
echo "${a[0]}"
### expect
2 3
Z
### end

### umask_uses_first_operand
umask 0111
umask 1 2; echo "status $?"
umask
umask 0111
umask u=, g+ 2>/dev/null; echo "status $?"
### expect
status 0
0001
status 1
### end

### nested_operand_keeps_inner_quotes
unset u
echo "${u:-"a b"}" "${u:-${u:-"c d"}}"
### expect
a b c d
### end

### shift_extra_operand_discards_line
# a script file resumes at the next line
cat > s.sh <<'EOF'
set -- a b c
shift 1 extra 2>/dev/null; echo unreached
echo "next $# $?"
EOF
bash s.sh
### expect
next 3 1
### end

### discarded_line_ends_child_command_string
# a builtin's discard ends a `bash -c` string; an expansion error does not
bash -c 'shift 1 2
echo unreached' 2>/dev/null
echo "rc $?"
bash -c 'echo $((1/0))
echo "next $?"' 2>/dev/null
### expect
rc 1
next 1
### end

### help_knows_itself_and_end_of_options
help help >/dev/null; echo "status $?"
help -- help >/dev/null; echo "status $?"
help ZZZ 2>&1 | grep -c 'no help topics match'
### expect
status 0
status 0
1
### end
