### array_element_replace_and_trim
a=(xa ya)
echo "${a[@]/a/b}" "${a[1]#y}" "${a[@]%a}" "${a[@]##?}" "${a[0]//a/-}"
### expect
xb yb a x y a a x-
### end

### array_element_case
a=(xa ya); b=(AB CD)
echo "${a[0]^}" "${a[@]^^}" "${b[@],}" "${b[*],,}"
### expect
Xa XA YA aB cD ab cd
### end

### array_quoted_at_keeps_fields
a=('a b' c)
printf '<%s>' "${a[@]/b/x}" "${a[@]^}"; echo
printf '<%s>' "${a[*]/b/x}"; echo
### expect
<a x><c><A b><C>
<a x c>
### end

### assoc_element_ops
declare -A m=([k]=val)
echo "${m[k]^}" "${m[@]%l}" "${m[k]/a/A}"
### expect
Val va vAl
### end

### array_transform_ops
a=(x 'y z')
echo ${a[@]@Q}
echo "${a[*]@Q}" "${a[0]@U}" "${a[1]@u}"
### expect
'x' 'y z'
'x' 'y z' X Y z
### end

### positional_transform_per_element
set -- ab 'c d'
printf '<%s>' "${@@Q}"; echo
### expect
<'ab'><'c d'>
### end

### case_mod_with_pattern
v="hello world"
echo "${v^^[lo]}" "${v^[hw]}" "${v^[a-g]}" "${v^^?}"
V=HELLO
echo "${V,,[L]}" "${V,[H]}" "${V,[X]}" "${V,,}"
### expect
heLLO wOrLd Hello world hello world HELLO WORLD
HEllO hELLO HELLO hello
### end

### case_mod_pattern_on_array
a=(hello yellow)
echo "${a[@]^^l}" "${a[1]^y}"
### expect
heLLo yeLLow Yellow
### end
