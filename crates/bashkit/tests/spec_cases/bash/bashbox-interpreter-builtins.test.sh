# BashBox interpreter-builtins cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_interpreter_builtins_for_without_in_walks_the_positional_parameters
# for without in walks the positional parameters
set -- a b c; for x; do echo $x; done
### expect
a
b
c
### end

### bashbox_interpreter_builtins_c_style_for_with_continue_and_break
# c-style for with continue and break
for ((i=0; i<10; i++)); do test $i = 1 && continue; test $i = 3 && break; echo $i; done
### expect
0
2
### end

### bashbox_interpreter_builtins_c_style_for_with_no_clauses_loops_until_break
# c-style for with no clauses loops until break
i=0; for ((;;)); do i=$((i+1)); test $i -gt 2 && break; echo $i; done
### expect
1
2
### end

### bashbox_interpreter_builtins_loop_status_is_the_last_body_status
# loop status is the last body status
for ((i=0; i<2; i++)); do false; done; echo $?
### expect
1
### end

### bashbox_interpreter_builtins_while_with_continue_and_break
# while with continue and break
i=0; while test $i -lt 5; do i=$((i+1)); test $i = 2 && continue; test $i = 4 && break; echo $i; done
### expect
1
3
### end

### bashbox_interpreter_builtins_while_break_2_from_a_nested_while
# while break 2 from a nested while
for a in x y; do while true; do while true; do break 2; done; done; echo $a; done
### expect
x
y
### end

### bashbox_interpreter_builtins_while_continue_3_resumes_the_outer_for
# while continue 3 resumes the outer for
for a in x y; do while true; do while true; do continue 3; done; done; echo no; done; echo done
### expect
done
### end

### bashbox_interpreter_builtins_while_that_never_runs_exits_0
# while that never runs exits 0
while false; do :; done; echo $?
### expect
0
### end

### bashbox_interpreter_builtins_until_counts_up
# until counts up
i=0; until test $i -ge 3; do echo $i; i=$((i+1)); done
### expect
0
1
2
### end

### bashbox_interpreter_builtins_until_with_continue_and_break
# until with continue and break
i=0; until false; do i=$((i+1)); test $i = 2 && continue; test $i = 4 && break; echo $i; done
### expect
1
3
### end

### bashbox_interpreter_builtins_until_status_is_the_last_body_status
# until status is the last body status
i=0; until test $i = 2; do i=$((i+1)); false; done; echo $?
### expect
1
### end

### bashbox_interpreter_builtins_runs_the_next_body_without_testing_it
# ;& runs the next body without testing it
case a in a) echo 1;& b) echo 2;; c) echo 3;; esac
### expect
1
2
### end

### bashbox_interpreter_builtins_goes_on_testing_the_next_patterns
# ;;& goes on testing the next patterns
case ab in a*) echo 1;;& *b) echo 2;;& c) echo 3;; esac
### expect
1
2
### end

### bashbox_interpreter_builtins_no_match_exits_0
# no match exits 0
case x in y) echo no;; esac; echo $?
### expect
0
### end

### bashbox_interpreter_builtins_status_of_the_matched_body
# status of the matched body
case x in x) false;; esac; echo $?
### expect
1
### end

### bashbox_interpreter_builtins_quoted_backslash
# quoted backslash
case 'a\' in 'a\') echo bs;; esac
### expect
bs
### end

### bashbox_interpreter_builtins_unclosed_bracket_is_literal
# unclosed bracket is literal
case ab in a[b) echo open;; *) echo other;; esac
### expect
other
### end

### bashbox_interpreter_builtins_exit_ends_only_the_subshell
# exit ends only the subshell
(exit 3); echo $?
### expect
3
### end

### bashbox_interpreter_builtins_return_ends_only_the_subshell
# return ends only the subshell
f(){ (return 4); echo $?; }; f
### expect
4
### end

### bashbox_interpreter_builtins_errexit_ends_only_the_subshell
# errexit ends only the subshell
(set -e; false; echo no); echo $?
### expect
1
### end

### bashbox_interpreter_builtins_keeps_earlier_output_in_place
# keeps earlier output in place
echo pre; x=$(echo b); echo "[$x]"
### expect
pre
[b]
### end

### bashbox_interpreter_builtins_exit_status_reaches
# exit status reaches $?
x=$(exit 4); echo $?
### expect
4
### end

### bashbox_interpreter_builtins_state_changes_stay_inside
# state changes stay inside
x=1; y=$(x=2; f(){ :; }; echo $x); echo $x $y; type f >/dev/null 2>&1; echo $?
### expect
1 2
1
### end

### bashbox_interpreter_builtins_does_not_fire_the_exit_trap
# does not fire the EXIT trap
trap 'echo bye' EXIT; x=$(echo a); echo $x
### expect
a
bye
### end

### bashbox_interpreter_builtins_an_expansion_error_ends_only_the_substitution
# an expansion error ends only the substitution
x=$(echo ${zz:?boom}); echo "after $?"
### expect
after 1
### end

### bashbox_interpreter_builtins_fires_after_the_body_of_the_function_that_set_it
# fires after the body of the function that set it
f(){ trap 'echo ret' RETURN; echo in; }; f; echo after
### expect
in
ret
after
### end

### bashbox_interpreter_builtins_is_not_inherited_by_functions
# is not inherited by functions
trap 'echo ret' RETURN; f(){ echo f; }; f; echo end
### expect
f
end
### end

### bashbox_interpreter_builtins_stays_set_but_other_functions_do_not_see_it
# stays set but other functions do not see it
f(){ trap 'echo ret' RETURN; }; g(){ echo g; }; f; echo top; g; h(){ g; }; h
### expect
ret
top
g
g
### end

### bashbox_interpreter_builtins_sees_the_function_locals
# sees the function locals
f(){ local v=loc; trap 'echo $v' RETURN; }; f
### expect
loc
### end

### bashbox_interpreter_builtins_input_from_a_missing_file
# input from a missing file
cat < missing.txt; echo $?
### expect
1
### end

### bashbox_interpreter_builtins_creates_the_file
# <> creates the file
cat <> rw.txt; echo $?; test -f rw.txt && echo created
### expect
0
created
### end

### bashbox_interpreter_builtins_2_1_before_dev_null_keeps_stderr_on_the_pipe
# 2>&1 before >/dev/null keeps stderr on the pipe
{ echo out; echo err >&2; } 2>&1 >/dev/null | cat
### expect
err
### end

### bashbox_interpreter_builtins_closing_stderr
# closing stderr
ls /nope 2>&-; echo y
### expect
y
### end

### bashbox_interpreter_builtins_file_sends_both_streams_to_the_file
# >&file sends both streams to the file
{ echo z; echo e >&2; } >& both.txt; cat both.txt
### expect
z
e
### end

### bashbox_interpreter_builtins_and_write_and_append_both_streams
# &> and &>> write and append both streams
{ echo a; echo b >&2; } &> all.txt; { echo c; } &>> all.txt; cat all.txt
### expect
a
b
c
### end

### bashbox_interpreter_builtins_dev_stderr_is_fd_2_as_of_that_redirection
# /dev/stderr is fd 2 as of that redirection
echo x > /dev/stderr 2>/dev/null; echo y
### expect
y
### end

### bashbox_interpreter_builtins_a_missing_directory_fails
# a missing directory fails >
echo a > /no/such/dir/f; echo $?
### expect
1
### end

### bashbox_interpreter_builtins_a_missing_directory_fails_2
# a missing directory fails >&
echo z >& /no/such/dir/f; echo $?
### expect
1
### end

### bashbox_interpreter_builtins_quoted_heredoc_stays_literal
# quoted heredoc stays literal
cat <<'EOF'
$HOME	x
EOF
### expect
$HOME	x
### end

### bashbox_interpreter_builtins_strips_leading_tabs_and_expands
# <<- strips leading tabs and expands
cat <<-EOF
	tab $((1+1))
	EOF
### expect
tab 2
### end

### bashbox_interpreter_builtins_here_string
# here-string
cat <<< "here $((2+3))"
### expect
here 5
### end

### bashbox_interpreter_builtins_an_empty_array
# an empty array
a=(); echo ${#a[@]}; a=(1 2 3); a=(); echo ${#a[@]}
### expect
0
0
### end

### bashbox_interpreter_builtins_to_a_readonly_variable_fails
# to a readonly variable fails
readonly r=1
r=2
echo $?
### expect
1
### end

### bashbox_interpreter_builtins_xtrace_of_a_prefix_assignment
# xtrace of a prefix assignment
set -x; X=1 echo hi
### expect
hi
### end

### bashbox_interpreter_builtins_and_short_circuit
# && and || short-circuit
x=0; echo $((0 && (x=5))) $x $((1 || (x=6))) $x $((1 && 2)) $((0 || 0)) $((0 || 3))
### expect
0 0 1 0 1 0 1
### end

### bashbox_interpreter_builtins_unary_operators
# unary operators
echo $((-(2+3))) $((+4)) $((!0)) $((!5)) $((~0)) $((++5)) $((--5))
### expect
-5 4 1 0 -1 5 5
### end

### bashbox_interpreter_builtins_increment_and_decrement
# increment and decrement
x=5; echo $((x++)) $x $((x--)) $x $((++x)) $((--x)); echo $((y++)) $y
### expect
5 6 6 5 6 5
0 1
### end

### bashbox_interpreter_builtins_ternary
# ternary
echo $((1?2:3)) $((0?2:3))
### expect
2 3
### end

### bashbox_interpreter_builtins_variables_holding_expressions
# variables holding expressions
x=1+2; echo $((x*3)) $(($x*3))
### expect
9 7
### end

### bashbox_interpreter_builtins_positional_parameters
# positional parameters
set -- 5; echo $(($1+1)); (( $1 == 5 )) && echo five
### expect
6
five
### end

### bashbox_interpreter_builtins_expansions_and_quotes_inside
# expansions and quotes inside
a=(1 2 3); echo $(("1"+1)) $(( $(echo 2) * 3 )) $(( ${#a[@]} + 1 ))
### expect
2 6 4
### end

### bashbox_interpreter_builtins_associative_array_elements
# associative array elements
declare -A m; m[foo]=5; echo $((m[foo]*2)); (( m[bar]=3 )); echo ${m[bar]}
### expect
10
3
### end

### bashbox_interpreter_builtins_command_status
# command status
((0)); echo $?; ((2)); echo $?
### expect
1
0
### end

### bashbox_interpreter_builtins_c_style_for_header_with_a_substitution
# c-style for header with a substitution
for ((i=$(echo 1); i<3; i++)); do echo $i; done
### expect
1
2
### end

### bashbox_interpreter_builtins_case
# /=
x=5; ((x/=0)); echo $? $x
### expect
1 5
### end

### bashbox_interpreter_builtins_case_2
# %=
x=5; ((x%=0)); echo $? $x
### expect
1 5
### end

### bashbox_interpreter_builtins_case_3
# /
((5/0)); echo $?
### expect
1
### end

### bashbox_interpreter_builtins_case_4
# %
((5%0)); echo $?
### expect
1
### end

### bashbox_interpreter_builtins_string_comparisons
# string comparisons
[[ abc != a* ]]; echo $?; [[ abc != x* ]]; echo $?; [[ a < b ]]; echo $?; [[ b > a ]]; echo $?; [[ b < a ]]; echo $?
### expect
1
0
0
0
1
### end

### bashbox_interpreter_builtins_integer_comparisons
# integer comparisons
[[ 3 -eq 3 && 3 -ne 4 && 2 -lt 3 && 3 -le 3 && 4 -gt 3 && 4 -ge 4 ]]; echo $?; [[ 3 -eq 4 ]]; echo $?
### expect
0
1
### end

### bashbox_interpreter_builtins_regex_match
# regex match
[[ abc123 =~ [0-9]+ ]]; echo $?; [[ abc =~ ^b ]]; echo $?
### expect
0
1
### end

### bashbox_interpreter_builtins_regex_fills_bash_rematch
# regex fills BASH_REMATCH
re="(b)(c)"; [[ abc =~ $re ]]; echo $? ${BASH_REMATCH[0]} ${BASH_REMATCH[2]}; [[ a/b =~ a/b ]]; echo $?
### expect
0 bc c
0
### end

### bashbox_interpreter_builtins_empty_and_non_empty_strings
# empty and non-empty strings
[[ -z "" && -n x ]]; echo $?; [[ -z x ]]; echo $?; [[ x ]]; echo $?; [[ "" ]]; echo $?
### expect
0
1
0
1
### end

### bashbox_interpreter_builtins_variable_is_set
# variable is set
v=1; a=(1 2); [[ -v v ]]; echo $?; [[ -v a ]]; echo $?; [[ -v nope ]]; echo $?
### expect
0
0
1
### end

### bashbox_interpreter_builtins_and_match_newlines
# * and ? match newlines
x=$(printf "a\nb"); [[ $x == a* ]]; echo $?; case $x in a?b) echo nl;; esac
### expect
0
nl
### end

### bashbox_interpreter_builtins_unset_in_its_own_function_keeps_a_local_hiding_the_global
# unset in its own function keeps a local hiding the global
f(){ local x=1; unset x; echo "[${x-unset}]"; x=2; echo $x; }; x=g; f; echo $x
### expect
[unset]
2
g
### end
