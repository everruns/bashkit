# Nameref tests
# Inspired by Oils spec/nameref.test.sh
# https://github.com/oilshell/oil/blob/master/spec/nameref.test.sh

### nameref_pass_array_by_ref
# pass array by reference
### skip: TODO nameref (local -n / typeset -n) not implemented
show_value() {
  local -n array_name=$1
  local idx=$2
  echo "${array_name[$idx]}"
}
shadock=(ga bu zo meu)
show_value shadock 2
### expect
zo
### end

### nameref_mutate_array
# mutate array by reference
### skip: TODO nameref (local -n / typeset -n) not implemented
set1() {
  local -n array_name=$1
  local val=$2
  array_name[1]=$val
}
shadock=(a b c d)
set1 shadock ZZZ
echo ${shadock[@]}
### expect
a ZZZ c d
### end

### nameref_assoc_array
# pass assoc array by reference
### skip: TODO nameref (local -n / typeset -n) not implemented
show_value() {
  local -n array_name=$1
  local idx=$2
  echo "${array_name[$idx]}"
}
declare -A days=([monday]=eggs [tuesday]=bread [sunday]=jam)
show_value days sunday
### expect
jam
### end

### nameref_local_dynamic_scope
# pass local array by reference via dynamic scoping
### skip: TODO nameref (local -n / typeset -n) not implemented
show_value() {
  local -n array_name=$1
  local idx=$2
  echo "${array_name[$idx]}"
}
caller() {
  local shadock=(ga bu zo meu)
  show_value shadock 2
}
caller
### expect
zo
### end

### nameref_flag_n_plus_n
# flag -n and +n for typeset
### skip: TODO nameref (local -n / typeset -n) not implemented
x=foo
ref=x
echo ref=$ref
typeset -n ref
echo ref=$ref
x=bar
echo ref=$ref
typeset +n ref
echo ref=$ref
### expect
ref=x
ref=foo
ref=bar
ref=x
### end

### nameref_mutate_through
# mutating through nameref: ref=
### skip: TODO nameref (local -n / typeset -n) not implemented
x=XX
y=YY
ref=y
typeset -n ref
echo ref=$ref
ref=XXXX
echo ref=$ref
echo y=$y
### expect
ref=YY
ref=XXXX
y=XXXX
### end

### nameref_bang_inverts
# flag -n combined ${!ref} -- bash INVERTS
### skip: TODO nameref (local -n / typeset -n) not implemented
foo=FOO
x=foo
ref=x
echo "!ref=${!ref}"
typeset -n ref
echo ref=$ref
echo "!ref=${!ref}"
### expect
!ref=foo
ref=foo
!ref=x
### end

### nameref_unset
# unset through nameref unsets the target
### skip: TODO nameref (local -n / typeset -n) not implemented
x=X
typeset -n ref=x
echo ref=$ref
unset ref
echo "ref=$ref"
echo "x=$x"
### expect
ref=X
ref=
x=
### end

### nameref_chain
# Chain of namerefs
### skip: TODO nameref (local -n / typeset -n) not implemented
x=foo
typeset -n ref=x
typeset -n ref_to_ref=ref
echo ref_to_ref=$ref_to_ref
echo ref=$ref
### expect
ref_to_ref=foo
ref=foo
### end

### nameref_dynamic_scope
# Dynamic scope with namerefs
### skip: TODO nameref (local -n / typeset -n) not implemented
f3() {
  local -n ref=$1
  ref=x
}
f2() {
  f3 "$@"
}
f1() {
  local F1=F1
  echo F1=$F1
  f2 F1
  echo F1=$F1
}
f1
### expect
F1=F1
F1=x
### end

### nameref_change_reference
# change reference itself
### skip: TODO nameref (local -n / typeset -n) not implemented
x=XX
y=YY
typeset -n ref=x
echo ref=$ref
typeset -n ref=y
echo ref=$ref
ref=z
echo x=$x
echo y=$y
### expect
ref=XX
ref=YY
x=XX
y=z
### end

### nameref_array_element
# a[2] in nameref
### skip: TODO nameref (local -n / typeset -n) not implemented
typeset -n ref='a[2]'
a=(zero one two three)
echo ref=$ref
### expect
ref=two
### end

### nameref_mutate_array_element
# mutate through nameref: ref[0]=
### skip: TODO nameref (local -n / typeset -n) not implemented
array=(X Y Z)
typeset -n ref=array
ref[0]=xx
echo ${array[@]}
### expect
xx Y Z
### end

### nameref_local_basic
# local -n basic usage
### skip: TODO nameref (local -n / typeset -n) not implemented
x=hello
f() {
  local -n r=x
  echo $r
  r=world
}
f
echo $x
### expect
hello
world
### end
