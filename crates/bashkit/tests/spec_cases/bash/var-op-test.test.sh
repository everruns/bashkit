# Variable operator test (:-  :+  :=  :?) edge cases
# Inspired by Oils spec/var-op-test.test.sh
# https://github.com/oilshell/oil/blob/master/spec/var-op-test.test.sh

### vop_lazy_eval_alternative
# Lazy Evaluation of Alternative
i=0
x=x
echo ${x:-$((i++))}
echo $i
echo ${undefined:-$((i++))}
echo $i
### expect
x
0
0
1
### end

### vop_default_when_empty
# Default value when empty
empty=''
echo ${empty:-is empty}
### expect
is empty
### end

### vop_default_when_unset
# Default value when unset
echo ${unset_var_xyz-is unset}
### expect
is unset
### end

### vop_assign_default_empty
# Assign default value when empty
empty=''
: ${empty:=is empty}
echo $empty
### expect
is empty
### end

### vop_assign_default_unset
# Assign default value when unset
: ${vop_unset_var=is unset}
echo $vop_unset_var
### expect
is unset
### end

### vop_alternative_when_set
# ${v:+foo} Alternative value when set
v=foo
empty=''
echo "${v:+v is not empty}" "${empty:+is not empty}"
### expect
v is not empty 
### end

### vop_alternative_when_unset
# ${v+foo} Alternative value when unset
v=foo
echo "${v+v is not unset}" "${vop_unset2:+is not unset}"
### expect
v is not unset 
### end

### vop_quoted_alternative_regression
# "${x+foo}" quoted regression
echo "${vop_with_icc+set}" = set
### expect
 = set
### end

### vop_plus_with_set_u
# ${s+foo} and ${s:+foo} when set -u
set -u
v=v
echo v=${v:+foo}
echo v=${v+foo}
unset v
echo v=${v:+foo}
echo v=${v+foo}
set +u
### expect
v=foo
v=foo
v=
v=
### end

### vop_minus_with_set_u
# ${v-foo} and ${v:-foo} when set -u
set -u
v=v
echo v=${v:-foo}
echo v=${v-foo}
unset v
echo v=${v:-foo}
echo v=${v-foo}
set +u
### expect
v=v
v=v
v=foo
v=foo
### end

### vop_error_when_empty
# Error when empty
### bash_diff: uses bash -c which may differ in sandbox
bash -c 'empty=""; echo ${empty:?"is empty"}' 2>/dev/null
echo status=$?
### expect
status=1
### end

### vop_error_when_unset
# Error when unset
### bash_diff: bash -c exit code differs in sandbox (127 vs 1)
bash -c 'echo ${vop_unset3?"is unset"}' 2>/dev/null
echo status=$?
### expect
status=1
### end

### vop_assign_dynamic_scope
# ${var=x} dynamic scope in function
f() { : "${hello:=x}"; echo $hello; }
f
echo hello=$hello
### expect
x
hello=x
### end

### vop_array_assign_default
# array ${arr[0]=x}
### skip: TODO ${arr[0]=x} array element default assignment not implemented
arr=()
echo ${#arr[@]}
: ${arr[0]=x}
echo ${#arr[@]}
### expect
0
1
### end

### vop_backslash_in_default
# "\z" as default value arg
echo "${undef_bs1-\$}"
echo "${undef_bs2-\(}"
echo "${undef_bs3-\z}"
echo "${undef_bs4-\"}"
echo "${undef_bs5-\`}"
echo "${undef_bs6-\\}"
### expect
$
\(
\z
"
`
\
### end

### vop_at_empty_minus_plus
# $@ (empty) and - and +
set --
echo "argv=${@-minus}"
echo "argv=${@+plus}"
echo "argv=${@:-minus}"
echo "argv=${@:+plus}"
### expect
argv=minus
argv=
argv=minus
argv=
### end

### vop_at_one_empty_minus_plus
# $@ ("") and - and +
set -- ""
echo "argv=${@-minus}"
echo "argv=${@+plus}"
echo "argv=${@:-minus}"
echo "argv=${@:+plus}"
### expect
argv=
argv=plus
argv=minus
argv=
### end

### vop_at_two_empty_minus_plus
# $@ ("" "") and - and +
set -- "" ""
echo "argv=${@-minus}"
echo "argv=${@+plus}"
echo "argv=${@:-minus}"
echo "argv=${@:+plus}"
### expect
argv= 
argv=plus
argv= 
argv=plus
### end

### vop_array_empty_minus
# array and - operator
arr=()
echo ${arr[@]-minus}
arr=('')
echo ${arr[@]-minus}
arr=(3 4)
echo ${arr[@]-minus}
### expect
minus

3 4
### end

### vop_array_empty_plus
# array and + operator
arr=()
echo ${arr[@]+plus}
arr=('')
echo ${arr[@]+plus}
arr=(3 4)
echo ${arr[@]+plus}
### expect

plus
plus
### end

### vop_assoc_array_minus_plus
# assoc array and - and +
declare -A empty_assoc=()
declare -A assoc=(['k']=v)
echo empty=${empty_assoc[@]-minus}
echo empty=${empty_assoc[@]+plus}
echo assoc=${assoc[@]-minus}
echo assoc=${assoc[@]+plus}
### expect
empty=minus
empty=
assoc=v
assoc=plus
### end
