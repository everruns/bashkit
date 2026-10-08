# BashBox shell-variables cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_shell_variables_bash_subshell_counts_subshells_and
# BASH_SUBSHELL counts subshells, $(...) and <(...)
echo $BASH_SUBSHELL; (echo $BASH_SUBSHELL; (echo $BASH_SUBSHELL)); echo $(echo $BASH_SUBSHELL); x=$( (echo $BASH_SUBSHELL) ); echo $x; cat <(echo $BASH_SUBSHELL)
### expect
0
1
2
1
2
1
### end

### bashbox_shell_variables_unsetting_them_works_like_bash
# unsetting them works like bash
unset HOME USER PATH IFS; echo "[$HOME|$USER|$PATH|$IFS]" "${IFS-unset}"; cd; echo $?
x="a b	c"; for w in $x; do echo "<$w>"; done; set -- p q; echo "$*"
IFS=; for w in $x; do echo "<$w>"; done; echo "$*"
### expect
[|||] unset
1
<a>
<b>
<c>
p q
<a b	c>
pq
### end

### bashbox_shell_variables_a_pipeline_sets_one_status_per_command
# a pipeline sets one status per command
true | false | true; echo "${PIPESTATUS[*]}" ${#PIPESTATUS[@]}; declare -p PIPESTATUS
### expect
0 1 0 3
declare -a PIPESTATUS=([0]="0")
### end

### bashbox_shell_variables_a_single_command_and_a_negated_one_set_it_too
# a single command and a negated one set it too
false; echo ${PIPESTATUS[@]}; ! true; echo ${PIPESTATUS[@]}; f() { return 3; }; f; echo ${PIPESTATUS[@]}
### expect
1
0
3
### end

### bashbox_shell_variables_an_assignment_and_a_subshell_each_set_it
# an assignment, ((...)), [[...]] and a subshell each set it
false | true; x=$(false); echo ${PIPESTATUS[@]}; (( 0 )); echo ${PIPESTATUS[@]}; [[ a == a ]]; echo ${PIPESTATUS[@]}; (exit 3); echo ${PIPESTATUS[@]}
### expect
1
1
0
3
### end

### bashbox_shell_variables_other_compound_commands_and_definitions_leave_the_last_one_i
# other compound commands and definitions leave the last one inside them
false | true; { false | true; }; echo ${PIPESTATUS[@]}; false | true; case x in y) ;; esac; echo ${PIPESTATUS[@]}; false | true; f() { :; }; echo ${PIPESTATUS[@]}
### expect
1 0
1 0
1 0
### end

### bashbox_shell_variables_a_substitution_sees_the_status_from_before_it
# a substitution sees the status from before it
false | true; echo $(echo ${PIPESTATUS[@]})
### expect
1 0
### end

### bashbox_shell_variables_pipefail_changes_but_not_pipestatus_and_the_shell_overwrites
# pipefail changes $? but not PIPESTATUS, and the shell overwrites it even when readonly
set -o pipefail; false | true; echo $? ${PIPESTATUS[@]}; PIPESTATUS=(9 9); echo ${PIPESTATUS[@]}; readonly PIPESTATUS; true | false; echo ${PIPESTATUS[@]}
### expect
1 1 0
0
0 1
### end
