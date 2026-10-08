# BashBox interpreter-core cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_interpreter_core_command_substitution_captures_only_its_own_output
# command substitution captures only its own output
echo a; x=$(echo b); echo "[$x]"
### expect
a
[b]
### end

### bashbox_interpreter_core_exit_inside_command_substitution_ends_only_the_subshell
# exit inside command substitution ends only the subshell
x=$(echo in; exit 3); echo "$? [$x]"
### expect
3 [in]
### end

### bashbox_interpreter_core_assignment_only_command_reports_the_substitution_status
# assignment-only command reports the substitution status
x=$(false); echo $?
### expect
1
### end

### bashbox_interpreter_core_exit_trap_waits_for_the_whole_script
# EXIT trap waits for the whole script
trap "echo bye" EXIT; eval "echo hi"; x=$(echo in); echo "after $x"
### expect
hi
after in
bye
### end

### bashbox_interpreter_core_empty_exit_trap_is_ignored
# empty EXIT trap is ignored
trap "" EXIT; echo x
### expect
x
### end

### bashbox_interpreter_core_skipped_still_reaches
# skipped && still reaches ||
false && echo a || echo b
### expect
b
### end

### bashbox_interpreter_core_skips_on_success
# || skips on success
true || echo no; echo $?
### expect
0
### end

### bashbox_interpreter_core_err_trap_fires_for_a_failing_command
# ERR trap fires for a failing command
trap "echo caught \$?" ERR; false; echo next $?
### expect
caught 1
next 1
### end

### bashbox_interpreter_core_err_trap_skips_non_final_members_of_an_and_or_list
# ERR trap skips non-final members of an and-or list
trap "echo caught" ERR; false && true; false || true; echo done
### expect
done
### end

### bashbox_interpreter_core_set_e_ignores_a_failure_before
# set -e ignores a failure before &&
set -e; false && true; echo yes
### expect
yes
### end

### bashbox_interpreter_core_set_e_ignores_a_negated_pipeline
# set -e ignores a negated pipeline
set -e; ! true; echo yes
### expect
yes
### end

### bashbox_interpreter_core_set_o_errexit_then_set_e
# set -o errexit then set +e
set -o errexit; set +e; false; echo yes
### expect
yes
### end

### bashbox_interpreter_core_pipeline_status_is_the_last_command
# pipeline status is the last command
false | true; echo $?
### expect
0
### end

### bashbox_interpreter_core_negated_pipeline
# negated pipeline
! false; echo $?; ! true; echo $?
### expect
0
1
### end

### bashbox_interpreter_core_pipefail_reports_the_rightmost_failure
# pipefail reports the rightmost failure
set -o pipefail; false | true; echo $?; set +o pipefail; false | true; echo $?
### expect
1
0
### end

### bashbox_interpreter_core_pipes_stderr_too
# |& pipes stderr too
{ echo out; echo err >&2; } |& cat
### expect
out
err
### end

### bashbox_interpreter_core_stderr_of_a_piped_command_is_not_piped
# stderr of a piped command is not piped
{ echo out; echo err >&2; } | cat
### expect
out
### end

### bashbox_interpreter_core_compound_command_output_is_redirected
# compound command output is redirected
{ echo a; echo b >&2; } > out 2>&1; cat out
### expect
a
b
### end

### bashbox_interpreter_core_loop_reads_from_a_redirected_file
# loop reads from a redirected file
printf "1\n2\n" > in; while read l; do echo $l; done < in
### expect
1
2
### end

### bashbox_interpreter_core_failed_compound_redirection_skips_the_command
# failed compound redirection skips the command
while read l; do echo "<$l>"; done < nofile; echo $?
### expect
1
### end

### bashbox_interpreter_core_every_compound_command_kind_runs
# every compound command kind runs
if true; then echo t; fi | cat; case a in a) echo c;; esac; (echo sub); ((1)) && [[ a ]] && echo arith; until true; do :; done; for ((i=0; i<1; i++)); do echo $i; done
### expect
t
c
sub
arith
0
### end

### bashbox_interpreter_core_break_and_continue_with_levels
# break and continue with levels
for i in 1 2 3; do for j in a b; do test $j = b && continue 2; test $i = 3 && break 2; echo $i$j; done; done; until false; do break; done; echo end
### expect
1a
2a
end
### end

### bashbox_interpreter_core_break_and_continue_outside_a_loop
# break and continue outside a loop
break; continue 2; echo $?
### expect
0
### end

### bashbox_interpreter_core_break_inside_a_function_cannot_leave_the_caller_loop
# break inside a function cannot leave the caller loop
f() { break; }; for i in 1; do f; echo no; done
### expect
no
### end

### bashbox_interpreter_core_command_not_found
# command not found
nope; echo $?
### expect
127
### end

### bashbox_interpreter_core_command_name_is_field_split
# command name is field split
CMD="echo a b"; $CMD c
### expect
a b c
### end

### bashbox_interpreter_core_empty_expansion_leaves_only_the_assignment
# empty expansion leaves only the assignment
e=; x=1 $e; echo $x
### expect
1
### end

### bashbox_interpreter_core_prefix_assignment_reaches_a_command_env_only
# prefix assignment reaches a command env only
FOO=bar printenv FOO; echo "[$FOO]"
### expect
bar
[]
### end

### bashbox_interpreter_core_prefix_assignment_is_temporary_for_functions
# prefix assignment is temporary for functions
f() { echo "x=$x"; }; x=1 f; echo "[$x]"
### expect
x=1
[]
### end

### bashbox_interpreter_core_prefix_assignment_is_temporary_for_builtins
# prefix assignment is temporary for builtins
y=0; y=5 eval "echo \$y"; echo $y
### expect
5
0
### end

### bashbox_interpreter_core_repeated_prefix_assignment_restores_the_original
# repeated prefix assignment restores the original
x=0; x=1 x=2 printenv x; echo $x
### expect
2
0
### end

### bashbox_interpreter_core_readonly_prefix_assignment_is_reported_the_command_still_run
# readonly prefix assignment is reported, the command still runs
readonly R=1; R=2 echo "[$R]"; echo "s=$?"
### expect
[1]
s=0
### end

### bashbox_interpreter_core_xtrace_prints_assignments_and_commands
# xtrace prints assignments and commands
set -x; x=1 printenv x; y=2
### expect
1
### end

### bashbox_interpreter_core_bad_substitution_fails_only_its_command
# bad substitution fails only its command
echo ${x!}
echo after $?
### expect
after 1
### end

### bashbox_interpreter_core_expansion_error_abandons_the_rest_of_its_line
# expansion error abandons the rest of its line
f() { echo ${x!}; echo in; }
f; echo same
echo next
### expect
next
### end

### bashbox_interpreter_core_cannot_assign_fails_only_its_command
# cannot assign fails only its command
echo ${1:=x}
echo after $?
### expect
after 1
### end

### bashbox_interpreter_core_return_1_wraps_to_255
# return -1 wraps to 255
f() { return -1; }; f; echo $?
### expect
255
### end

### bashbox_interpreter_core_return_outside_a_function
# return outside a function
return; echo $?
### expect
2
### end

### bashbox_interpreter_core_export_tracks_later_assignments
# export tracks later assignments
export X=1; X=2; printenv X; export Y; Y=3; printenv Y; export -n Z=4; echo $Z
### expect
2
3
4
### end

### bashbox_interpreter_core_export_of_a_readonly_variable
# export of a readonly variable
readonly R=1; export R=2; echo $?
### expect
1
### end

### bashbox_interpreter_core_export_lists_variables
# export lists variables
export B=2 A=1; export | grep "[AB]="
### expect
declare -x A="1"
declare -x B="2"
### end

### bashbox_interpreter_core_unset_f_and_v
# unset -f and -v
f() { echo fn; }; unset -f f; f; a=1; unset -v a; echo "[$a]"
### expect
[]
### end

### bashbox_interpreter_core_unset_an_array_element
# unset an array element
a=(1 2 3); unset "a[1]"; echo ${a[@]}
### expect
1 3
### end

### bashbox_interpreter_core_unset_a_whole_array
# unset a whole array
a=(1 2); unset a; echo "[${a[@]}]"
### expect
[]
### end

### bashbox_interpreter_core_unset_a_readonly_variable
# unset a readonly variable
readonly r=1; unset r; echo $?
### expect
1
### end

### bashbox_interpreter_core_local_outside_a_function
# local outside a function
local x=1; echo $?
### expect
1
### end

### bashbox_interpreter_core_local_variables_vanish_after_the_function
# local variables vanish after the function
f() { local a=1 b; b=2; echo $a$b; }; f; echo "[$a$b]"
### expect
12
[]
### end

### bashbox_interpreter_core_local_of_a_readonly_variable
# local of a readonly variable
readonly r=1; f() { local r=2; }; f; echo $?
### expect
1
### end

### bashbox_interpreter_core_set_lists_variables_quoting_when_needed
# set lists variables, quoting when needed
a=1; b="x y"; c=""; d="it's"; set | grep "^[abcd]="
### expect
a=1
b='x y'
c=
d='it'\''s'
### end

### bashbox_interpreter_core_set_positional_parameters
# set positional parameters
set -- x y; echo $# $1; set a b; echo $2
### expect
2 x
b
### end

### bashbox_interpreter_core_set_c_and_c
# set -C and +C
set -C; echo a > f; echo b > f; echo $?; set +C; echo c > f; cat f
### expect
1
c
### end

### bashbox_interpreter_core_set_f_disables_globbing
# set -f disables globbing
touch a.txt; set -f; echo *.txt; set +f; echo *.txt
### expect
*.txt
a.txt
### end

### bashbox_interpreter_core_set_v_has_no_effect_on_a_one_line_script
# set -v has no effect on a one-line script
set -v; echo hi
### expect
hi
### end

### bashbox_interpreter_core_set_u_and_u
# set -u and +u
set -u; set +u; echo "[$nope]"
### expect
[]
### end

### bashbox_interpreter_core_set_x_stops_tracing
# set +x stops tracing
set -x; set +x; echo hi
### expect
hi
### end

### bashbox_interpreter_core_shopt_succeeds
# shopt succeeds
shopt -s nullglob; echo $?
### expect
0
### end

### bashbox_interpreter_core_source_without_a_file
# source without a file
source; echo $?
### expect
2
### end

### bashbox_interpreter_core_source_a_missing_file
# source a missing file
source nope.sh; echo $?
### expect
1
### end

### bashbox_interpreter_core_source_arguments_become_positional_parameters
# source arguments become positional parameters
echo "echo sourced \$1 \$#" > s.sh; set -- p q; source s.sh arg; . ./s.sh; echo $1
### expect
sourced arg 1
sourced p 2
p
### end

### bashbox_interpreter_core_source_without_arguments_can_set_positional_parameters
# source without arguments can set positional parameters
echo "set -- z" > s.sh; source s.sh; echo $1
### expect
z
### end

### bashbox_interpreter_core_return_ends_a_sourced_file
# return ends a sourced file
echo "return 4; echo no" > s.sh; source s.sh; echo $?
### expect
4
### end

### bashbox_interpreter_core_return_in_a_file_sourced_by_a_function
# return in a file sourced by a function
echo "x=1; return 5; x=2" > s; f() { source ./s; echo "s=$? x=$x"; }; f
### expect
s=5 x=1
### end

### bashbox_interpreter_core_eval_runs_in_the_current_shell
# eval runs in the current shell
eval "a=1;" "echo \$a"; eval; echo $?
### expect
1
0
### end

### bashbox_interpreter_core_eval_output_follows_its_redirection
# eval output follows its redirection
echo a; eval "echo b" > f; cat f
### expect
a
b
### end

### bashbox_interpreter_core_declare_without_a_value_keeps_the_variable
# declare without a value keeps the variable
x=1; declare x; echo $x; declare -p x nope; echo $?
### expect
1
declare -- x="1"
1
### end

### bashbox_interpreter_core_declare_p_readonly_and_arrays
# declare -p readonly and arrays
declare -r y=2; declare -p y; a=(1 "b c"); declare -p a
### expect
declare -r y="2"
declare -a a=([0]="1" [1]="b c")
### end

### bashbox_interpreter_core_declare_in_a_function_is_local_unless_g
# declare in a function is local unless -g
f() { declare x=1; declare -g g=2; }; f; echo "[$x][$g]"
### expect
[][2]
### end

### bashbox_interpreter_core_declare_a
# declare -a
declare -a arr; arr[0]=x; echo ${arr[0]}; declare -a b=v; echo ${b[0]}
### expect
x
v
### end

### bashbox_interpreter_core_declare_of_a_readonly_variable
# declare of a readonly variable
readonly RO=1; declare RO=2; echo $?
### expect
1
### end

### bashbox_interpreter_core_declare_x
# declare -x
declare -x E=1; printenv E
### expect
1
### end

### bashbox_interpreter_core_declare_alone_lists_variables
# declare alone lists variables
q=1; declare | grep "^q="
### expect
q=1
### end

### bashbox_interpreter_core_let_without_arguments
# let without arguments
let; echo $?
### expect
1
### end

### bashbox_interpreter_core_let_status_follows_the_last_value
# let status follows the last value
let x=0; echo $?; let y=2 z=3; echo $y$z $?
### expect
1
23 0
### end

### bashbox_interpreter_core_shift
# shift
set -- a b c; shift; echo $*; shift 2; echo $# $?; shift; echo $?
### expect
b c
0 0
1
### end

### bashbox_interpreter_core_getopts_walks_grouped_options_and_arguments
# getopts walks grouped options and arguments
set -- -ac -bval -- -z rest1; while getopts "ab:c" o; do echo "$o ${OPTARG-unset} $OPTIND"; done; shift $((OPTIND-1)); echo "rest: $*"
### expect
a unset 1
c unset 2
b val 3
rest: -z rest1
### end

### bashbox_interpreter_core_getopts_silent_mode
# getopts silent mode
set -- -z -b; while getopts ":ab:" o; do echo "$o ${OPTARG-unset}"; done
### expect
? z
: b
### end

### bashbox_interpreter_core_getopts_reports_errors
# getopts reports errors
set -- -z -b; while getopts "ab:" o; do echo "$o ${OPTARG-unset}"; done
### expect
? unset
? unset
### end

### bashbox_interpreter_core_getopts_with_explicit_arguments
# getopts with explicit arguments
getopts a o -a -b; echo "$o $?"; getopts a o -a -b; echo "$o $? ${OPTARG-u}"; getopts a o -a -b; echo "$o $?"
### expect
a 0
? 0 u
? 1
### end

### bashbox_interpreter_core_getopts_restarts_when_optind_is_reset
# getopts restarts when OPTIND is reset
getopts ab o -ab; echo $o; OPTIND=1; getopts xy o -x; echo $o
### expect
a
x
### end

### bashbox_interpreter_core_getopts_stops_at_a_non_option
# getopts stops at a non-option
set -- a -b; getopts b o; echo "$? $o $OPTIND"
### expect
1 ? 1
### end

### bashbox_interpreter_core_getopts_option_argument_in_the_next_word
# getopts option argument in the next word
getopts "a:" o -a val; echo "$o $OPTARG $OPTIND"
### expect
a val 3
### end

### bashbox_interpreter_core_getopts_usage
# getopts usage
getopts; echo $?
### expect
2
### end

### bashbox_interpreter_core_type_t
# type -t
f(){ :; }; type -t f cd ls echo nope; echo $?
### expect
function
builtin
file
builtin
1
### end

### bashbox_interpreter_core_type_describes_each_name
# type describes each name
type nope cd ls; echo $?
### expect
cd is a shell builtin
ls is /usr/bin/ls
1
### end

### bashbox_interpreter_core_command_v
# command -v
f(){ :; }; command -v cd ls f nope; echo $?; command -v nope; echo $?
### expect
cd
/usr/bin/ls
f
0
1
### end

### bashbox_interpreter_core_command_skips_functions
# command skips functions
f(){ :; }; command f; echo $?; command; echo $?; echo() { :; }; command echo hi
### expect
127
0
hi
### end

### bashbox_interpreter_core_command_runs_builtins
# command runs builtins
command cd /tmp; pwd
### expect
/tmp
### end

### bashbox_interpreter_core_builtin
# builtin
builtin cd /tmp && pwd; builtin echo hi; builtin cat; echo $?; builtin; echo $?
### expect
/tmp
hi
1
0
### end

### bashbox_interpreter_core_exec_runs_a_command_and_ends_the_script
# exec runs a command and ends the script
exec echo done; echo no
### expect
done
### end

### bashbox_interpreter_core_exec_without_a_command
# exec without a command
exec; echo still
### expect
still
### end

### bashbox_interpreter_core_alias_define_list_and_query
# alias define, list and query
alias a=b c="it's"; alias; alias a; alias zz; echo $?
### expect
alias a='b'
alias c='it'\''s'
alias a='b'
1
### end

### bashbox_interpreter_core_unalias
# unalias
alias a=b; unalias a zz; echo $?; alias x=y; unalias -a; alias
### expect
1
### end

### bashbox_interpreter_core_trap_listing_order_and_quoting
# trap listing order and quoting
trap "echo b" ERR; trap "echo a" EXIT; trap "" INT; trap "it's" SIGTERM; trap
### expect
trap -- 'echo a' EXIT
trap -- '' SIGINT
trap -- 'it'\''s' SIGTERM
trap -- 'echo b' ERR
a
### end

### bashbox_interpreter_core_trap_with_a_lone_signal_resets_it
# trap with a lone signal resets it
trap "echo x" 0; trap; trap EXIT; trap; echo done
### expect
trap -- 'echo x' EXIT
done
### end

### bashbox_interpreter_core_err_trap_keeps
# ERR trap keeps $?
trap "true" ERR; false; echo $?
### expect
1
### end

### bashbox_interpreter_core_pushd_and_popd
# pushd and popd
cd /tmp; mkdir -p /tmp/a /tmp/b; pushd /tmp/a; pushd /tmp/b; pushd; popd; popd; popd; echo $?
### expect
/tmp/a /tmp
/tmp/b /tmp/a /tmp
/tmp/a /tmp/b /tmp
/tmp/b /tmp
/tmp
1
### end

### bashbox_interpreter_core_pushd_without_a_stack
# pushd without a stack
pushd; echo $?
### expect
1
### end

### bashbox_interpreter_core_pushd_to_a_missing_directory
# pushd to a missing directory
pushd /nope; echo $?
### expect
1
### end

### bashbox_interpreter_core_pushd_abbreviates_home
# pushd abbreviates HOME
cd /tmp; pushd ~
### expect
~ /tmp
### end

### bashbox_interpreter_core_hash
# hash
hash; hash -r; : a b; echo $?
### expect
hash: hash table empty
0
### end

### bashbox_interpreter_core_reply_is_not_trimmed
# REPLY is not trimmed
echo "  a  b  " | { read; echo "[$REPLY]"; }
### expect
[  a  b  ]
### end

### bashbox_interpreter_core_non_whitespace_ifs_keeps_the_rest_intact
# non-whitespace IFS keeps the rest intact
echo "a:b:c:d" | { IFS=: read x y; echo "[$x][$y]"; }
### expect
[a][b:c:d]
### end

### bashbox_interpreter_core_adjacent_separators_make_empty_fields
# adjacent separators make empty fields
echo "a::b" | { IFS=: read -a x; echo "${#x[@]} [${x[1]}]"; }
### expect
3 []
### end

### bashbox_interpreter_core_unterminated_line_assigns_but_fails
# unterminated line assigns but fails
printf "a b" | { read x; echo "$? [$x]"; }
### expect
1 [a b]
### end

### bashbox_interpreter_core_escaped_separator_stays_in_the_field
# escaped separator stays in the field
echo "a\\ b c" | { read x y; echo "[$x][$y]"; }
### expect
[a b][c]
### end

### bashbox_interpreter_core_empty_ifs_disables_splitting
# empty IFS disables splitting
printf "x y" | { IFS= read -r v; echo "[$v]"; }
### expect
[x y]
### end

### bashbox_interpreter_core_nul_delimited_records
# NUL delimited records
printf "a\0b\0" | while read -r -d "" v; do echo "[$v]"; done
### expect
[a]
[b]
### end

### bashbox_interpreter_core_mapfile_on_nul_delimiters
# mapfile on NUL delimiters
printf "a\0b\0" | { mapfile -d "" arr; echo ${#arr[@]}; }
### expect
2
### end

### bashbox_interpreter_core_readarray_t_into_mapfile
# readarray -t into MAPFILE
printf "a\nb" | { readarray -t; echo "${MAPFILE[1]}"; }
### expect
b
### end

### bashbox_interpreter_core_read_from_a_pipe_into_eval
# read from a pipe into eval
echo hi | eval "read x; echo \$x"
### expect
hi
### end

### bashbox_interpreter_core_default
# default
time echo hi
### expect
hi
### end

### bashbox_interpreter_core_posix
# posix
time -p true
### expect
### end

### bashbox_interpreter_core_an_invalid_regex_fails_with_status_2
# an invalid regex fails [[ with status 2
re='('; [[ a =~ $re ]]; echo $?; re='[[:foo:]]'; [[ a =~ $re ]]; echo $?; [[ abc =~ b(c) ]]; echo $? ${BASH_REMATCH[@]}
### expect
2
2
0 bc c
### end

### bashbox_interpreter_core_evaluates_integer_operands_without_expanding_them_again
# [[ ]] evaluates integer operands without expanding them again
x='$(echo hi >&2; echo 1)'; [[ $x -eq 1 ]]; echo $?; x='1+1'; [[ $x -eq 2 ]]; echo $?; [[ 08 -eq 1 ]]; echo $?
### expect
1
0
1
### end

### bashbox_interpreter_core_assignment_subscripts_are_expanded_and_arithmetic_for_indexe
# assignment subscripts are expanded, and arithmetic for indexed arrays
i=3; a[$i]=x; a[i+1]=y; a[-1]=z; b=([i+1]=p [6]=q); read 'c[1+1]' <<< r; declare -p a b c
### expect
declare -a a=([3]="x" [4]="z")
declare -a b=([4]="p" [6]="q")
declare -a c=([2]="r")
### end

### bashbox_interpreter_core_an_associative_array_s_subscript_is_its_key
# an associative array's subscript is its key
declare -A m; k='a b'; m[$k]=1; m[1+1]=2; echo "${m[a b]} ${m[1+1]}"
### expect
1 2
### end

### bashbox_interpreter_core_a_negative_subscript_counts_back_from_the_end
# a negative subscript counts back from the end
a=(1 2 3); a[-1]+=x; declare -p a; a[-5]=q; echo same
echo next $?
### expect
declare -a a=([0]="1" [1]="2" [2]="3x")
next 1
### end

### bashbox_interpreter_core_unset_takes_options_first
# unset takes options first
unset -v x -n y; echo $?; unset -z; echo $?; unset -fn; echo $?; unset -n; echo $?; unset 'a b'; echo $?
### expect
1
2
0
0
0
### end

### bashbox_interpreter_core_unset_of_an_element_and_of_a_readonly_variable
# unset of an element and of a readonly variable
a=(1 2 3); unset 'a[1]'; declare -p a; readonly r; unset -n r; echo $?; readonly -a ra=(1); unset 'ra[0]'; echo $?
### expect
declare -a a=([0]="1" [2]="3")
1
1
### end

### bashbox_interpreter_core_set_n_reads_the_rest_without_running_it
# set -n reads the rest without running it
echo a; set -n; echo b
### expect
a
### end

### bashbox_interpreter_core_set_o_noexec_too
# set -o noexec too
set -o noexec; echo hi
### expect
### end

### bashbox_interpreter_core_set_k_takes_assignments_from_anywhere_in_a_command
# set -k takes assignments from anywhere in a command
f() { echo "$x"; }; set -k; echo a=b c; f x=5; echo $-; set +k; echo a=b
### expect
c
5
hkBc
a=b
### end

### bashbox_interpreter_core_a_subscript_may_quote_or_escape_a
# a subscript may quote or escape a ]
declare -A a b c; a["x]"]=1; b[\]]=2; c['y z']=3; declare -p a b c
### expect
declare -A a=(["x]"]="1" )
declare -A b=(["]"]="2" )
declare -A c=(["y z"]="3" )
### end

### bashbox_interpreter_core_unset_stops_taking_options_at
# unset stops taking options at --
x=1; unset -- x; echo $? ${x-unset}
### expect
0 unset
### end

### bashbox_interpreter_core_redirections_report_why_a_file_can_t_be_opened
# redirections report why a file can't be opened
### skip: TODO `<>` (read-write open) redirect is not parsed yet, and InMemoryFs lets `f/x` be created under a regular file `f` instead of failing with ENOTDIR
true <> /tmp; echo $?; touch f; echo hi > f/x; echo $?; cat < f/x; echo $?; ln -s loop loop; echo hi > loop; echo $?
### expect
1
1
1
1
### end

### bashbox_interpreter_core_reading_a_directory_fails
# reading a directory fails
cat < /tmp; echo $?
### expect
1
### end

### bashbox_interpreter_core_source_names_a_directory_it_can_t_read
# source names a directory it can't read
source /tmp; echo $?
### expect
1
### end

### bashbox_interpreter_core_set_o_keyword_shows_as_k
# set -o keyword shows as k
set -o keyword; echo $-; set -o | grep -E '^(keyword|noexec) '
### expect
hkBc
keyword        	on
noexec         	off
### end
