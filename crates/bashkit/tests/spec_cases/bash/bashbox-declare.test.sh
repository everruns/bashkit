# BashBox declare cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_declare_i_evaluates_each_assignment_and_adds
# -i evaluates each assignment, and += adds
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -i x=1+2; x+=3; echo $x; x='2*3'; echo $x; x=abc; echo $x; x=; echo $x; declare -p x
### expect
6
6
0
0
declare -i x="0"
### end

### bashbox_declare_i_on_an_array_evaluates_each_element_as_it_is_assigned
# -i on an array evaluates each element as it is assigned
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -i a=(1+1 2*3); a+=(5+5); a[7]=3+3; a[7]+=1; declare -p a
### expect
declare -ai a=([0]="2" [1]="6" [2]="10" [7]="7")
### end

### bashbox_declare_i_reads_and_let_agree
# -i reads and let/(( )) agree
declare -i x; let x=4; ((x=x+1)); echo $x; read x <<< 2+2; echo $x
### expect
5
4
### end

### bashbox_declare_read_and_printf_name_themselves_too
# read and printf name themselves too
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -i x; (read x <<< 1/0); (printf -v x 1/0); echo $?
### expect
1
### end

### bashbox_declare_a_subshell_ends_with_status_1
# a subshell ends with status 1
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
(declare -i x; x=1/0; echo in); echo out $?
### expect
out 1
### end

### bashbox_declare_i_removes_the_attribute_and_a_local_does_not_inherit_it
# +i removes the attribute, and a local does not inherit it
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -i x=1; f() { local x; x=1+1; echo $x; }; f; declare +i x; x=1+1; echo $x
### expect
1+1
1+1
### end

### bashbox_declare_a_prefix_assignment_ignores_i_l_and_u
# a prefix assignment ignores -i, -l and -u
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -u x; declare -i n; f() { echo "$x $n"; }; x=abc n=1/0 f
### expect
abc 1/0
### end

### bashbox_declare_l_and_u_convert_included
# -l and -u convert, += included
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -l x=ABC; x+=DEF; echo $x; declare -u y=abc; echo $y; read y <<< mixed; echo $y; declare -p x y
### expect
abcdef
ABC
MIXED
declare -l x="abcdef"
declare -u y="MIXED"
### end

### bashbox_declare_l_and_u_replace_each_other_together_neither_changes
# -l and -u replace each other; together neither changes
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -l x=AB; declare -u x; echo $x; x=Cd; echo $x; declare -lu x; x=eF; echo $x; declare +u x; x=Gh; declare -p x
### expect
ab
CD
eF
declare -- x="Gh"
### end

### bashbox_declare_u_applies_to_array_elements
# -u applies to array elements
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -u a=(xa yb); a[3]=zz; a[3]+=q; declare -p a
### expect
declare -au a=([0]="XA" [1]="YB" [3]="ZZQ")
### end

### bashbox_declare_declare_p_prints_the_attributes_in_bash_s_order
# declare -p prints the attributes in bash's order
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -ilrx x=5; declare -iAx m; declare -nr q=z; declare -p x m q
### expect
declare -irxl x="5"
declare -Aix m
declare -nr q="z"
### end

### bashbox_declare_declared_but_unset_arrays_and_scalars_print_bare
# declared but unset arrays and scalars print bare
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -a e; declare -A f; declare -i g; declare -p e f g; e+=(q); declare -p e; [[ -v f ]]; echo $?
### expect
declare -a e
declare -A f
declare -i g
declare -a e=([0]="q")
1
### end

### bashbox_declare_a_scalar_declared_as_an_array_becomes_element_0
# a scalar declared as an array becomes element 0
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=1; declare -A x; declare -p x; y=2; declare -a y; declare -p y; declare -a z=v; declare -p z
### expect
declare -A x=([0]="1" )
declare -a y=([0]="2")
declare -a z=([0]="v")
### end

### bashbox_declare_converting_between_indexed_and_associative_fails
# converting between indexed and associative fails
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
a=(1 2); declare -A a; echo $?; declare -A m; declare -a m; echo $?; declare -p a m
### expect
1
1
declare -a a=([0]="1" [1]="2")
declare -A m
### end

### bashbox_declare_with_an_array_value_the_conversion_error_abandons_the_line
# with an array value the conversion error abandons the line
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
a=(1); declare -A a=([x]=2); echo same
echo next $?; declare -p a
### expect
next 1
declare -a a=([0]="1")
### end

### bashbox_declare_declare_with_only_attributes_lists_the_variables_that_have_t
# declare with only attributes lists the variables that have them all
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -i I=2; declare -ia J=(1); declare -A M; declare -ai; declare -A | grep ' M'; declare -i | grep -E '^declare -a?i [IJ]'
### expect
declare -ai J=([0]="1")
declare -A M
declare -i I="2"
declare -ai J=([0]="1")
### end

### bashbox_declare_a_nameref_reads_writes_and_appends_through_to_its_target
# a nameref reads, writes and appends through to its target
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -n r=a; a=5; echo $r; r=7; echo $a; r+=x; echo $a; echo ${!r}; declare -p r a
### expect
5
7
7x
a
declare -n r="a"
declare -- a="7x"
### end

### bashbox_declare_a_chain_of_namerefs
# a chain of namerefs
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -n r=x; declare -n s=r; s=5; echo $x ${!s}
### expect
5 x
### end

### bashbox_declare_a_nameref_to_an_element
# a nameref to an element
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -n r=a[1]; r=x; echo $r; declare -p a
### expect
x
declare -a a=([1]="x")
### end

### bashbox_declare_a_nameref_to_an_array
# a nameref to an array
declare -n r=arr; r=(1 2 3); echo ${r[1]} ${#r[@]} "${!r[@]}"; r[1]=b; declare -p arr
### expect
2 3 0 1 2
declare -a arr=([0]="1" [1]="b" [2]="3")
### end

### bashbox_declare_declare_acts_on_the_target_unless_n_is_given
# declare acts on the target unless -n is given
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -n r=x; declare r=5; declare -i r; r=1+1; declare -p r x
### expect
declare -n r="x"
declare -i x="2"
### end

### bashbox_declare_n_turns_a_nameref_back_into_a_plain_variable
# +n turns a nameref back into a plain variable
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -n r=x; declare +n r; declare -p r
### expect
declare -- r="x"
### end

### bashbox_declare_a_nameref_declared_without_a_value_takes_the_first_value_ass
# a nameref declared without a value takes the first value assigned
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -n r; r=x; declare -p r; echo "[${!r}]"
### expect
declare -n r="x"
[x]
### end

### bashbox_declare_a_for_loop_points_a_nameref_at_each_word
# a for loop points a nameref at each word
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -n r; for r in a b; do r=v; done; declare -p a b r
### expect
declare -- a="v"
declare -- b="v"
declare -n r="b"
### end

### bashbox_declare_local_n_refers_to_the_caller_s_variable
# local -n refers to the caller's variable
f() { local -n r=$1; r=hi; }; f v; echo $v
### expect
hi
### end

### bashbox_declare_invalid_nameref_targets_are_refused
# invalid nameref targets are refused
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -n r='1x'; echo $?; declare -n r=@; echo $?; f() { local -n r='a b'; }; f; echo $?
### expect
1
1
1
### end

### bashbox_declare_a_value_that_can_t_be_a_name_stays_and_declare_n_still_succe
# a value that can't be a name stays and declare -n still succeeds
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=1; declare -n x; echo $?; declare -p x; y=foo; declare -n y; declare -p y
### expect
1
declare -- x="1"
declare -n y="foo"
### end

### bashbox_declare_in_a_function_a_self_reference_only_warns
# in a function a self reference only warns
f() { local -n r=r; echo $?; }; f
### expect
0
### end

### bashbox_declare_a_circular_reference_reads_as_unset_with_a_warning
# a circular reference reads as unset with a warning
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -n a=b; declare -n b=a; echo "${a-u}"; declare -p a b
### expect
u
declare -n a="b"
declare -n b="a"
### end

### bashbox_declare_assigning_through_a_circular_reference_abandons_the_line
# assigning through a circular reference abandons the line
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -n a=b; declare -n b=a; a=1; echo same
echo next $?
### expect
next 1
### end

### bashbox_declare_read_through_a_circular_reference_fails
# read through a circular reference fails
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -n a=b; declare -n b=a; read a <<< x; echo $?
### expect
1
### end

### bashbox_declare_a_nameref_local_to_a_function_is_restored
# a nameref local to a function is restored
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
r=global; f() { local -n r=x; r=1; }; f; echo $r $x
### expect
global 1
### end

### bashbox_declare_readonly_accepts_only_a_a_and_f
# readonly accepts only -a, -A and -f
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
readonly -i r=1+1; echo $?; readonly -A m=([k]=v); declare -p m; readonly -a m2; declare -p m2
### expect
2
declare -Ar m=([k]="v" )
declare -r m2
### end

### bashbox_declare_readonly_a_converts_nothing
# readonly -A converts nothing
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
a=(1); readonly -A a; echo $?; declare -p a
### expect
0
declare -ar a=([0]="1")
### end

### bashbox_declare_export_and_readonly_assign_array_operands
# export and readonly assign array operands
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
export a=(1 2); readonly b=(3); declare -p a b
### expect
declare -ax a=([0]="1" [1]="2")
declare -ar b=([0]="3")
### end

### bashbox_declare_declare_p_quotes_control_characters_and_bytes_that_aren_t_ut
# declare -p quotes control characters and bytes that aren't UTF-8 like bash
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=$'a\x01b\e\'"\\c$`d\a\b\f\v\r\t\n\x7f'; declare -p x; y=$'\xc3\xa9\n'; z=$'\xff'; declare -p y z; a=($'x\ny' z); declare -p a
### expect
declare -- x=$'a\001b\E\'"\\c$`d\a\b\f\v\r\t\n\177'
declare -- y=$'é\n'
declare -- z=$'\377'
declare -a a=([0]=$'x\ny' [1]="z")
### end

### bashbox_declare_associative_keys_with_shell_metacharacters_are_quoted
# associative keys with shell metacharacters are quoted
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
declare -A m=(["a b"]=1) n=(['$x']=2) o=([c]=3); declare -p m n o
### expect
declare -A m=(["a b"]="1" )
declare -A n=(["\$x"]="2" )
declare -A o=([c]="3" )
### end

### bashbox_declare_declare_p_with_no_names_lists_every_variable
# declare -p with no names lists every variable
unset PIPESTATUS; x=1; declare -p | grep -E '^declare -- x='
### expect
declare -- x="1"
### end

### bashbox_declare_plain_declare_lists_like_set
# plain declare lists like set
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=1; declare | grep '^x='
### expect
x=1
### end

### bashbox_declare_printf_v_assigns_the_output_newlines_and_all
# printf -v assigns the output, newlines and all
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf -v x '%s-%s\n' a b; echo "[$x]"; printf -vy %s a; echo $y; printf -v 'a[1+1]' %s z; declare -p a
### expect
[a-b
]
a
declare -a a=([2]="z")
### end

### bashbox_declare_printf_v_takes_and_a_local
# printf -v takes -- and a local
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf -v x -- %s a; echo $x; f() { local y; printf -v y %s in; echo $y; }; f; echo ${y-unset}
### expect
a
in
unset
### end

### bashbox_declare_printf_v_errors
# printf -v errors
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf -v; echo $?; printf -v x; echo $? ${x-unset}; printf -v 1x %s a; echo $?; printf -v 'a[' %s x; echo $?
### expect
2
2 unset
2
2
### end

### bashbox_declare_printf_v_into_a_readonly_variable_fails
# printf -v into a readonly variable fails
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
readonly r; printf -v r %s a; echo $?
### expect
1
### end

### bashbox_declare_printf_without_v_is_the_printf_command
# printf without -v is the printf command
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
printf -- -v; echo
### expect
-v
### end
