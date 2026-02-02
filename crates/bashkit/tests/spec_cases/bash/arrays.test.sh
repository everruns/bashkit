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
### skip: array slicing not implemented
arr=(a b c d e); echo ${arr[@]:1:3}
### expect
b c d
### end
