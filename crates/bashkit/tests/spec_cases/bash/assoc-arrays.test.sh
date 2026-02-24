### assoc_declare_and_access
declare -A mymap
mymap[name]="Alice"
mymap[age]="30"
echo "${mymap[name]}"
echo "${mymap[age]}"
### expect
Alice
30
### end

### assoc_length
declare -A m
m[a]="1"
m[b]="2"
m[c]="3"
echo "${#m[@]}"
### expect
3
### end

### assoc_keys
declare -A m
m[x]="10"
m[y]="20"
for k in "${!m[@]}"; do echo "$k"; done | sort
### expect
x
y
### end

### assoc_all_values
declare -A m
m[a]="alpha"
m[b]="beta"
for v in "${m[@]}"; do echo "$v"; done | sort
### expect
alpha
beta
### end

### assoc_overwrite
declare -A m
m[key]="old"
m[key]="new"
echo "${m[key]}"
### expect
new
### end

### assoc_empty
declare -A m
echo ">${#m[@]}<"
### expect
>0<
### end

### assoc_unset_key
declare -A m
m[a]="1"
m[b]="2"
unset 'm[a]'
echo "${#m[@]}"
echo "${m[b]}"
### expect
1
2
### end

### assoc_declare_inline
declare -A m=([foo]="bar" [baz]="qux")
echo "${m[foo]}"
echo "${m[baz]}"
### expect
bar
qux
### end

### assoc_declare_inline_unquoted
declare -A m=([a]=1 [b]=2 [c]=3)
echo "${m[a]}"
echo "${m[c]}"
### expect
1
3
### end

### assoc_declare_inline_single_entry
declare -A m=([only]="value")
echo "${m[only]}"
echo "${#m[@]}"
### expect
value
1
### end

### assoc_declare_inline_overwrite
declare -A m=([k]="old")
m[k]="new"
echo "${m[k]}"
### expect
new
### end

### assoc_numeric_string_key
declare -A m
m[1]="one"
m[2]="two"
echo "${m[1]}"
echo "${m[2]}"
### expect
one
two
### end

### assoc_variable_key
declare -A m
key="mykey"
m[$key]="value"
echo "${m[$key]}"
echo "${m[mykey]}"
### expect
value
value
### end

### assoc_special_chars_value
declare -A m
m[key]="hello world"
echo "${m[key]}"
### expect
hello world
### end

### assoc_iteration
declare -A m
m[a]="1"
m[b]="2"
m[c]="3"
for key in "${!m[@]}"; do
  echo "$key=${m[$key]}"
done | sort
### expect
a=1
b=2
c=3
### end
