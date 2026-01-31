### awk_print_all
# Print all input
printf 'hello world\n' | awk '{print}'
### expect
hello world
### end

### awk_print_field
# Print specific field
printf 'a b c\n' | awk '{print $2}'
### expect
b
### end

### awk_multiple_fields
# Print multiple fields
printf 'one two three\n' | awk '{print $1, $3}'
### expect
one three
### end

### awk_nf
# Number of fields
printf 'a b c d e\n' | awk '{print NF}'
### expect
5
### end

### awk_nr
# Line number
printf 'a\nb\nc\n' | awk '{print NR, $0}'
### expect
1 a
2 b
3 c
### end

### awk_begin
# BEGIN block
printf 'data\n' | awk 'BEGIN {print "start"} {print $0}'
### expect
start
data
### end

### awk_end
# END block
printf 'a\nb\n' | awk '{print $0} END {print "done"}'
### expect
a
b
done
### end

### awk_pattern
# Pattern matching
printf 'foo\nbar\nbaz\n' | awk '/bar/ {print}'
### expect
bar
### end

### awk_field_sep
# Custom field separator
printf 'a:b:c\n' | awk -F: '{print $2}'
### expect
b
### end

### awk_arithmetic
# Arithmetic operations
printf '5 3\n' | awk '{print $1 + $2}'
### expect
8
### end

### awk_variables
# User variables
printf '1\n2\n3\n' | awk '{sum += $1} END {print sum}'
### expect
6
### end

### awk_condition
# Conditional in action
printf '1\n2\n3\n4\n5\n' | awk '$1 > 3 {print}'
### expect
4
5
### end

### awk_length
# Length function
printf 'hello\n' | awk '{print length($0)}'
### expect
5
### end

### awk_substr
# Substring function
printf 'hello world\n' | awk '{print substr($0, 1, 5)}'
### expect
hello
### end

### awk_toupper
# Toupper function
printf 'hello\n' | awk '{print toupper($0)}'
### expect
HELLO
### end

### awk_tolower
# Tolower function
printf 'HELLO\n' | awk '{print tolower($0)}'
### expect
hello
### end

### awk_gsub
### skip: regex literal in function args not implemented
printf 'hello hello hello\n' | awk '{gsub(/hello/, "hi"); print}'
### expect
hi hi hi
### end

### awk_split
### skip: split with array assignment not fully implemented
printf 'a:b:c\n' | awk '{n = split($0, arr, ":"); print arr[2]}'
### expect
b
### end

### awk_printf
# Printf formatting
printf '42\n' | awk '{printf "value: %d\n", $1}'
### expect
value: 42
### end
