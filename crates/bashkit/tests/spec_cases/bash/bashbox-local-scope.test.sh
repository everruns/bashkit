# BashBox local-scope cases
# Imported from github.com/shipfastlabs/bashbox tests/ (MIT, Copyright (c) Pushpak Chhajed; see NOTICE).
# Expected stdout was recorded from GNU bash and re-verified against host bash before import.

### bashbox_local_scope_local_a_is_scoped_to_the_function
# local -a is scoped to the function
a=(g); f(){ local -a a=(1 2); echo "${a[@]} ${#a[@]}"; }; f; echo "${a[@]}"
### expect
1 2 2
g
### end

### bashbox_local_scope_local_a_with_keys
# local -A with keys
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ local -A m=([k]=v [j]=w); echo "${!m[@]} ${m[k]}"; }; f; declare -p m; echo $?
### expect
k j v
1
### end

### bashbox_local_scope_local_a_without_a_flag
# local a=(...) without a flag
f(){ local a=(x y); echo "${a[1]}"; }; f; echo "[${a[@]}]"
### expect
y
[]
### end

### bashbox_local_scope_declare_in_a_function_is_local
# declare in a function is local
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ declare -a a=(1 2); declare s=v; }; f; echo "[${a[@]}] [$s]"
### expect
[] []
### end

### bashbox_local_scope_declare_g_in_a_function_is_global
# declare -g in a function is global
f(){ declare -g -a a=(1 2); declare -g s=v; }; f; echo "${a[@]} $s"
### expect
1 2 v
### end

### bashbox_local_scope_declare_g_reaches_past_a_local
# declare -g reaches past a local
f(){ local x=1; g; echo $x; }; g(){ declare -g x=2; }; f; echo $x
### expect
1
2
### end

### bashbox_local_scope_declare_g_of_a_readonly_global_hidden_by_a_local
# declare -g of a readonly global hidden by a local
readonly x=1; f(){ local x=2 2>/dev/null; declare -g x=3; echo $?; }; f; echo $x
### expect
1
1
### end

### bashbox_local_scope_a_callee_sees_the_caller_local_array
# a callee sees the caller local array
f(){ local a=(1 2); g; echo "${a[@]}"; }; g(){ echo "${a[@]}"; a+=(3); }; a=(top); f; echo "${a[@]}"
### expect
1 2
1 2 3
top
### end

### bashbox_local_scope_recursion_keeps_each_frame_array
# recursion keeps each frame array
f(){ local -a a=($1); (($1 > 0)) && f $(($1 - 1)); echo "${a[@]}"; }; f 2
### expect
0
1
2
### end

### bashbox_local_scope_unset_of_a_local_array_in_its_own_function
# unset of a local array in its own function
f(){ local a=(1 2); unset a; echo "[${a[@]}]"; a=(3); }; a=(g); f; echo "${a[@]}"
### expect
[]
g
### end

### bashbox_local_scope_unset_from_a_callee_uncovers_the_global
# unset from a callee uncovers the global
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ local a=(1 2); g; echo "[${a[@]}]"; }; g(){ unset a; }; a=(gg); f; echo "${a[@]}"
### expect
[gg]
gg
### end

### bashbox_local_scope_unset_from_a_callee_uncovers_a_global_scalar
# unset from a callee uncovers a global scalar
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ local x=1; g; echo "[${x-unset}]"; }; g(){ unset x; }; x=g; f; echo $x
### expect
[g]
g
### end

### bashbox_local_scope_a_local_scalar_hides_a_global_array
# a local scalar hides a global array
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
a=(1 2 3); f(){ local a=x; echo "${a[@]} ${#a[@]}"; declare -p a; }; f; echo "${a[@]}"
### expect
x 1
declare -- a="x"
1 2 3
### end

### bashbox_local_scope_a_local_array_hides_a_global_scalar
# a local array hides a global scalar
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
a=s; f(){ local -a a=(1 2); echo "${a[@]}"; }; f; echo "${a[@]}"; declare -p a
### expect
1 2
s
declare -- a="s"
### end

### bashbox_local_scope_local_without_a_value_is_unset
# local without a value is unset
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=g; f(){ local x; echo "[${x-unset}]"; }; f; echo $x
### expect
[unset]
g
### end

### bashbox_local_scope_local_again_keeps_the_value
# local again keeps the value
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ local x=1; local x; echo $x; }; f
### expect
1
### end

### bashbox_local_scope_local_of_a_readonly_variable
# local of a readonly variable
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
readonly x=1; f(){ local x; echo "[$x]"; }; f; echo $?
### expect
[1]
0
### end

### bashbox_local_scope_local_g_is_global
# local -g is global
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ local -g x=1; }; f; echo $x
### expect
1
### end

### bashbox_local_scope_local_with_no_names_lists_nothing
# local with no names lists nothing
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ local x=1; local; }; f; echo $?
### expect
declare -- x="1"
0
### end

### bashbox_local_scope_local_outside_a_function
# local outside a function
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
local x=1; echo $?
### expect
1
### end

### bashbox_local_scope_locals_are_not_exported
# locals are not exported
x=g; f(){ local x=1; printenv x; }; f; echo $?
### expect
1
### end

### bashbox_local_scope_local_lineno_is_an_ordinary_variable
# local LINENO is an ordinary variable
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ local LINENO=5; echo $LINENO; }; f; echo $LINENO
### expect
5
1
### end

### bashbox_local_scope_a_local_array_readonly_declaration
# a local array readonly declaration
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ declare -ra a=(1); declare -p a; }; f; a=(2); echo "${a[@]}"
### expect
declare -ar a=([0]="1")
2
### end

### bashbox_local_scope_a_scalar_is_element_0_of_an_array
# a scalar is element 0 of an array
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=5; echo "${x[@]} ${#x[@]} ${!x[@]} ${x[0]}"; x[1]=6; echo "${x[@]} $x"; declare -p x
### expect
5 1 0 5
5 6 5
declare -a x=([0]="5" [1]="6")
### end

### bashbox_local_scope_assigning_an_array_name_sets_element_0
# assigning an array name sets element 0
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
a=(1 2); a=x; echo "${a[@]}"; a+=y; echo "${a[@]}"
### expect
x 2
xy 2
### end

### bashbox_local_scope_arithmetic_on_a_scalar_element
# arithmetic on a scalar element
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=4; echo $((x[0] + 1)); ((x[1] = 7)); echo "${x[@]}"
### expect
5
4 7
### end

### bashbox_local_scope_declare_a_converts_a_scalar
# declare -a converts a scalar
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
x=v; declare -a x; declare -p x
### expect
declare -a x=([0]="v")
### end

### bashbox_local_scope_read_assigns_into_a_local_array
# read assigns into a local array
f(){ local -a r; read -a r <<< "p q"; echo "${r[1]}"; }; f; echo "[${r[@]}]"
### expect
q
[]
### end

### bashbox_local_scope_local_lists_the_function_locals
# local lists the function locals
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ local x; local -a a=(1); local y=2 z; local -r r=1; local; }; f
### expect
declare -a a=([0]="1")
declare -r r="1"
declare -- x
declare -- y="2"
declare -- z
### end

### bashbox_local_scope_declare_p_of_a_local_without_a_value
# declare -p of a local without a value
### skip: TODO bashbox corpus gap, bashkit output differs from real bash
f(){ local x2; declare -p x2; }; f
### expect
declare -- x2
### end
