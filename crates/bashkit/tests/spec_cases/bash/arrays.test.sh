### array_declare
# Basic array declaration
arr=(a b c); echo ${arr[0]}
### expect
a
### end

### array_index
# Array indexing
arr=(one two three); echo ${arr[1]}
### expect
two
### end

### array_all
# All array elements
arr=(a b c); echo ${arr[@]}
### expect
a b c
### end

### array_length
# Array length
arr=(a b c d e); echo ${#arr[@]}
### expect
5
### end

### array_assign_index
# Assign by index
arr[0]=first; arr[1]=second; echo ${arr[0]} ${arr[1]}
### expect
first second
### end

### array_modify
# Modify array element
arr=(a b c); arr[1]=X; echo ${arr[@]}
### expect
a X c
### end

### array_append
# Append to array
arr=(a b); arr+=(c d); echo ${arr[@]}
### expect
a b c d
### end

### array_in_loop
# Array in for loop
arr=(one two three)
for item in "${arr[@]}"; do
  echo $item
done
### expect
one
two
three
### end

### array_sparse
# Sparse array
arr[0]=a; arr[5]=b; arr[10]=c; echo ${arr[@]}
### expect
a b c
### end

### array_element_length
# Length of array element
arr=(hello world); echo ${#arr[0]}
### expect
5
### end

### array_quoted
# Quoted array elements
arr=("hello world" "foo bar"); echo ${arr[0]}
### expect
hello world
### end

### array_from_command
# Array from command substitution
arr=($(echo a b c)); echo ${arr[1]}
### expect
b
### end

### array_indices
# Array indices expansion
arr=(a b c); echo ${!arr[@]}
### expect
0 1 2
### end

### array_slice
# Array slicing with offset and length
arr=(a b c d e); echo ${arr[@]:1:3}
### expect
b c d
### end

### array_slice_no_length
# Array slicing with offset only
arr=(a b c d e); echo ${arr[@]:2}
### expect
c d e
### end

### array_slice_from_start
# Array slicing from start
arr=(a b c d e); echo ${arr[@]:0:2}
### expect
a b
### end

### array_slice_negative_length_no_panic
# Negative length should not panic interpreter
arr=(a b c d e); echo ${arr[@]:1:-1}
### expect
b c d e
### end

### array_at_expansion_as_args
# "${arr[@]}" expands to separate arguments for commands
arr=(one two three)
printf "%s\n" "${arr[@]}"
### expect
one
two
three
### end

### array_star_expansion_quoted
# "${arr[*]}" joins into single argument when quoted
arr=(one two three)
printf "[%s]\n" "${arr[*]}"
### expect
[one two three]
### end

### array_at_expansion_unquoted
# ${arr[@]} unquoted also produces separate args
arr=(x y z)
printf "(%s)\n" ${arr[@]}
### expect
(x)
(y)
(z)
### end

### array_for_loop_with_brace
# Array and brace expansion mixed in for-loop word list
arr=(a b)
for x in "${arr[@]}" {1..3}; do echo $x; done
### expect
a
b
1
2
3
### end

### mapfile_basic
### bash_diff: pipes don't create subshells in bashkit (stateless model)
# mapfile reads lines into array from pipe
printf 'a\nb\nc\n' | mapfile -t lines; echo ${#lines[@]}; echo ${lines[0]}; echo ${lines[1]}; echo ${lines[2]}
### expect
3
a
b
c
### end

### readarray_alias
### bash_diff: pipes don't create subshells in bashkit (stateless model)
# readarray is an alias for mapfile
printf 'x\ny\n' | readarray -t arr; echo ${arr[0]} ${arr[1]}
### expect
x y
### end

### mapfile_default_name
### bash_diff: pipes don't create subshells in bashkit (stateless model)
# mapfile default array name is MAPFILE
printf 'hello\nworld\n' | mapfile -t; echo ${MAPFILE[0]}; echo ${MAPFILE[1]}
### expect
hello
world
### end

### array_negative_index_last
# Negative index gets last element
arr=(a b c d e); echo "${arr[-1]}"
### expect
e
### end

### array_negative_index_second_last
# Negative index gets second-to-last
arr=(a b c d e); echo "${arr[-2]}"
### expect
d
### end

### array_negative_index_first
# Negative index wrapping to first
arr=(a b c); echo "${arr[-3]}"
### expect
a
### end

### array_negative_all_values
# ${arr[@]} with negative indexing for assignment
arr=(10 20 30 40 50)
arr[-1]=99
echo "${arr[@]}"
### expect
10 20 30 40 99
### end

### local_array_compound_assignment
# local arr=(a b c) should initialize the array
myfunc() {
  local arr=(one two three)
  echo "count: ${#arr[@]}"
  echo "values: ${arr[*]}"
}
myfunc
### expect
count: 3
values: one two three
### end

### local_array_compound_in_global
# local arr=(...) at global scope should also work
local arr=(x y z)
echo "${#arr[@]}"
echo "${arr[1]}"
### expect
3
y
### end

### quoted_expansion_no_word_split_in_array
# arr=("test ${X} done") should NOT word-split inside quotes
X="hello world"
arr=(-a "test ${X} done")
echo "count: ${#arr[@]}"
printf "<%s>\n" "${arr[@]}"
### expect
count: 2
<-a>
<test hello world done>
### end

### quoted_single_quote_no_word_split_in_array
# arr=('multi word') should NOT word-split inside single quotes
arr=('hello world' 'foo bar')
echo "count: ${#arr[@]}"
printf "<%s>\n" "${arr[@]}"
### expect
count: 2
<hello world>
<foo bar>
### end

### quoted_mixed_elements_in_array
# Mix of quoted and unquoted elements preserves quoting
X="a b"
arr=(plain "quoted ${X} end" 'literal $X')
echo "count: ${#arr[@]}"
printf "<%s>\n" "${arr[@]}"
### expect
count: 3
<plain>
<quoted a b end>
<literal $X>
### end

### unquoted_expansion_word_split_in_array
# arr=($x) should word-split on IFS
x="alpha beta gamma"
arr=($x)
echo "${#arr[@]}"
echo "${arr[1]}"
### expect
3
beta
### end

### unquoted_expansion_custom_ifs_in_array
# arr=($x) with custom IFS
IFS=","; x="a,b,c"; arr=($x); echo "${#arr[@]}"
### expect
3
### end
