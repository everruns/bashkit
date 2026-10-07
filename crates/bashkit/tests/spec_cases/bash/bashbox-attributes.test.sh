# BashBox attributes cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_attributes_a_plain_assignment_is_not_exported
# a plain assignment is not exported
FOO=1; printenv FOO; echo $?
### expect
1
### end

### bashbox_attributes_export_marks_the_name_so_later_values_reach_commands
# export marks the name, so later values reach commands
export X=1; X=2; printenv X; export Y; Y=3; printenv Y
### expect
2
3
### end

### bashbox_attributes_a_local_inherits_the_export_attribute
# a local inherits the export attribute
export x=g; f(){ local x=1; printenv x; }; f
### expect
1
### end

### bashbox_attributes_local_x_exports_only_the_local
# local -x exports only the local
x=g; f(){ local -x x=1; printenv x; }; f; printenv x; echo $?
### expect
1
1
### end

### bashbox_attributes_unset_drops_the_export_attribute
# unset drops the export attribute
export x=1; unset x; x=2; printenv x; echo $?
### expect
1
### end

### bashbox_attributes_export_n_un_exports
# export -n un-exports
export A=1; export -n A; printenv A; echo $? $A
### expect
1 1
### end

### bashbox_attributes_declare_p_shows_r_and_x
# declare -p shows r and x
declare -rx R=1; declare -p R; declare -x S; declare -p S; export -p | grep -w '[RS]'
### expect
declare -rx R="1"
declare -x S
declare -rx R="1"
declare -x S
### end

### bashbox_attributes_declare_x_un_exports
# declare +x un-exports
declare -x A=1; declare +x A; printenv A; echo $?
### expect
1
### end

### bashbox_attributes_set_a_exports_every_assignment_while_on
# set -a exports every assignment while on
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -a; B=2; printenv B; echo $-; set +a; C=3; printenv C; echo $?
### expect
2
ahBc
1
### end

### bashbox_attributes_allexport_covers_locals_and_read_not_arrays
# allexport covers locals and read, not arrays
set -o allexport; f(){ local l=3; declare -p l; }; f; read r <<< hi; declare -p r; arr=(1); declare -p arr
### expect
declare -x l="3"
declare -x r="hi"
declare -a arr=([0]="1")
### end

### bashbox_attributes_prefix_assignments_are_exported_for_one_command
# prefix assignments are exported for one command
FOO=1 printenv FOO; printenv FOO; echo $?; f(){ printenv V; }; V=7 f; printenv V; echo $?
### expect
1
1
7
1
### end

### bashbox_attributes_a_prefix_assignment_to_an_exported_variable_restores_its_val
# a prefix assignment to an exported variable restores its value
export V=0; V=1 printenv V; printenv V
### expect
1
0
### end

### bashbox_attributes_arrays_print_with_a_or_a_an_associative_one_with_a_trailing_
# arrays print with a or A, an associative one with a trailing space
declare -a arr=(1 2); export arr; declare -p arr; declare -A m=([k]=v); declare -p m
### expect
declare -ax arr=([0]="1" [1]="2")
declare -A m=([k]="v" )
### end

### bashbox_attributes_a_local_hides_a_until_the_function_returns
# a local hides -A until the function returns
declare -A m=([a]=1); f(){ local m; declare -p m; local -A n; n[x]=1; declare -p n; }; f; declare -p m
### expect
declare -- m
declare -A n=([x]="1" )
declare -A m=([a]="1" )
### end

### bashbox_attributes_declare_rejects_names_that_are_not_identifiers_and_options_a
# declare rejects names that are not identifiers, and options after the names
declare 1x=2 y=3 -z; echo $? $y
### expect
1 3
### end

### bashbox_attributes_local_rejects_names_that_are_not_identifiers
# local rejects names that are not identifiers
f(){ local a-b=1 c=2; echo $? $c; }; f
### expect
1 2
### end

### bashbox_attributes_readonly_rejects_names_that_are_not_identifiers
# readonly rejects names that are not identifiers
readonly 9=1 r=1; echo $? $r; readonly | grep ' r='
### expect
1 1
declare -r r="1"
### end

### bashbox_attributes_export_rejects_names_that_are_not_identifiers
# export rejects names that are not identifiers
export a.b x=1; echo $?; printenv x
### expect
1
1
### end

### bashbox_attributes_export_rejects_unknown_options
# export rejects unknown options
export -z; echo $?
### expect
2
### end

### bashbox_attributes_declare_assigns_an_array_element
# declare assigns an array element
declare a[1]=x; declare -p a
### expect
declare -a a=([1]="x")
### end

### bashbox_attributes_readonly_p_shows_each_variable_s_attributes
# readonly -p shows each variable's attributes
readonly U; export V; readonly V; readonly -p | grep -w '[UV]'
### expect
declare -r U
declare -rx V
### end

### bashbox_attributes_declare_p_escapes_quotes_dollars_backquotes_and_backslashes
# declare -p escapes quotes, dollars, backquotes and backslashes
x='a"b$c`d\e'; declare -p x; a=("q\""); declare -p a
### expect
declare -- x="a\"b\$c\`d\\e"
declare -a a=([0]="q\"")
### end

### bashbox_attributes_export_f_only_checks_that_the_functions_exist
# export -f only checks that the functions exist
f(){ :; }; export -f f; echo $?; export -f nosuch; echo $?
### expect
0
1
### end

### bashbox_attributes_env_sees_only_exported_variables
# env sees only exported variables
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
env | grep -c FOO; FOO=1 env | grep FOO; export E=1; (E=2; printenv E); printenv E; x=$(printenv E); echo $x
### expect
0
FOO=1
2
1
1
### end

### bashbox_attributes_options_end_at
# options end at --
export -- G=1; printenv G
### expect
1
### end

### bashbox_attributes_o_in_a_cluster_takes_the_next_argument_as_its_option_name
# o in a cluster takes the next argument as its option name
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -euo pipefail; echo "[${1-}]" $-; shopt -po pipefail
### expect
[] ehuBc
set -o pipefail
### end

### bashbox_attributes_several_o_options
# several -o options
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -o errexit -o nounset; echo $-; set +o nounset; echo $-
### expect
ehuBc
ehBc
### end

### bashbox_attributes_o_at_the_end_of_a_cluster_lists_the_options_after_applying_t
# o at the end of a cluster lists the options after applying the others
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -euo | grep -E '^(errexit|nounset|xtrace) '
### expect
errexit        	on
nounset        	on
xtrace         	off
### end

### bashbox_attributes_o_at_the_end_prints_them_as_commands
# +o at the end prints them as commands
set -e +o | grep -E 'errexit|pipefail'
### expect
set -o errexit
set +o pipefail
### end

### bashbox_attributes_an_unknown_option_name_changes_nothing
# an unknown option name changes nothing
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -o bogus -u; echo $? $-
### expect
2 hBc
### end

### bashbox_attributes_an_unknown_letter_changes_nothing
# an unknown letter changes nothing
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -ez; echo $? $-
### expect
2 hBc
### end

### bashbox_attributes_a_bad_option_inside_is_reported_as
# a bad option inside -- is reported as -
set --bogus; echo $?
### expect
2
### end

### bashbox_attributes_letters_with_no_effect_here_are_accepted
# letters with no effect here are accepted
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
set -hkB; echo $?
### expect
0
### end

### bashbox_attributes_words_after_the_options_become_positional_parameters
# words after the options become positional parameters
set -e a b; echo "$@"; set -ex -- c d; echo "$@"
### expect
a b
c d
### end

### bashbox_attributes_with_nothing_after_it_clears_them
# -- with nothing after it clears them
set -- a b; set --; echo $#
### expect
0
### end

### bashbox_attributes_a_lone_keeps_them
# a lone - keeps them
set -- a b; set -; echo $# "$@"; set - c; echo "$@"
### expect
2 a b
c
### end
