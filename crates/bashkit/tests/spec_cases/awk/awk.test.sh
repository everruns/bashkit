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
# gsub with regex literal
printf 'hello hello hello\n' | awk '{gsub(/hello/, "hi"); print}'
### expect
hi hi hi
### end

### awk_split
# split with array indexing
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

### awk_variable_v_flag
### skip: -v variable assignment flag not implemented
printf 'world\n' | awk -v greeting="hello" '{print greeting, $0}'
### expect
hello world
### end

### awk_string_concat
# String concatenation
printf 'hello world\n' | awk '{print $1 "-" $2}'
### expect
hello-world
### end

### awk_compound_plus_equals
# Compound += operator
printf '10\n20\n30\n' | awk 'BEGIN {x=0} {x += $1} END {print x}'
### expect
60
### end

### awk_compound_minus_equals
# Compound -= operator
printf '5\n' | awk 'BEGIN {x=100} {x -= $1} END {print x}'
### expect
95
### end

### awk_compound_times_equals
# Compound *= operator
printf '2\n3\n' | awk 'BEGIN {x=1} {x *= $1} END {print x}'
### expect
6
### end

### awk_compound_divide_equals
# Compound /= operator
printf '2\n' | awk 'BEGIN {x=100} {x /= $1} END {print x}'
### expect
50
### end

### awk_postfix_increment
### skip: postfix increment operator not implemented
printf 'a\n' | awk 'BEGIN {x=5} {print x++; print x}'
### expect
5
6
### end

### awk_prefix_increment
### skip: prefix increment operator not implemented
printf 'a\n' | awk 'BEGIN {x=5} {print ++x}'
### expect
6
### end

### awk_postfix_decrement
### skip: postfix decrement operator not implemented
printf 'a\n' | awk 'BEGIN {x=5} {print x--; print x}'
### expect
5
4
### end

### awk_prefix_decrement
### skip: prefix decrement operator not implemented
printf 'a\n' | awk 'BEGIN {x=5} {print --x}'
### expect
4
### end

### awk_logical_and
# Logical AND operator
printf '5\n' | awk '$1 > 3 && $1 < 10 {print "yes"}'
### expect
yes
### end

### awk_logical_or
# Logical OR operator
printf '1\n5\n10\n' | awk '$1 < 2 || $1 > 8 {print $1}'
### expect
1
10
### end

### awk_power_caret
### skip: power operator ^ not implemented
printf '2 3\n' | awk '{print $1 ^ $2}'
### expect
8
### end

### awk_power_double_star
### skip: power operator ** not implemented
printf '2 4\n' | awk '{print $1 ** $2}'
### expect
16
### end

### awk_nr_condition_equal
# NR equality condition
printf 'a\nb\nc\n' | awk 'NR == 2 {print}'
### expect
b
### end

### awk_nr_condition_range
# NR range with &&
printf 'a\nb\nc\nd\ne\n' | awk 'NR >= 2 && NR <= 4 {print}'
### expect
b
c
d
### end

### awk_begin_empty_input
# BEGIN executes even with no input
echo -n | awk 'BEGIN {print "start"}'
### expect
start
### end

### awk_printf_hex
### skip: printf %x format not implemented
printf '255\n' | awk '{printf "%x\n", $1}'
### expect
ff
### end

### awk_printf_octal
### skip: printf %o format not implemented
printf '8\n' | awk '{printf "%o\n", $1}'
### expect
10
### end

### awk_printf_char
### skip: printf %c format not implemented
printf '65\n' | awk '{printf "%c\n", $1}'
### expect
A
### end

### awk_printf_string_width
### skip: printf width specifier not implemented
printf 'hi\n' | awk '{printf "%5s\n", $1}'
### expect
   hi
### end

### awk_field_sep_no_space
# Field separator without space after -F
printf 'a,b,c\n' | awk -F, '{print $2}'
### expect
b
### end

### awk_field_sep_tab
### skip: tab escape in -F not parsed correctly
printf 'a\tb\tc\n' | awk -F'\t' '{print $2}'
### expect
b
### end

### awk_nf_empty_line
# NF for empty line
printf '\n' | awk '{print NF}'
### expect
0
### end

### awk_missing_field
### skip: missing field outputs newline instead of empty
printf 'a b\n' | awk '{print $5}'
### expect

### end

### awk_subtraction
# Subtraction operation
printf '10 3\n' | awk '{print $1 - $2}'
### expect
7
### end

### awk_multiplication
# Multiplication operation
printf '6 7\n' | awk '{print $1 * $2}'
### expect
42
### end

### awk_division
# Division operation
printf '20 4\n' | awk '{print $1 / $2}'
### expect
5
### end

### awk_modulo
# Modulo operation
printf '17 5\n' | awk '{print $1 % $2}'
### expect
2
### end

### awk_comparison_lt
# Less than comparison
printf '3\n5\n2\n' | awk '$1 < 4 {print}'
### expect
3
2
### end

### awk_comparison_le
# Less than or equal comparison
printf '3\n5\n2\n' | awk '$1 <= 3 {print}'
### expect
3
2
### end

### awk_comparison_ge
# Greater than or equal comparison
printf '3\n5\n2\n' | awk '$1 >= 3 {print}'
### expect
3
5
### end

### awk_comparison_eq
# Equality comparison
printf '3\n5\n3\n' | awk '$1 == 3 {print NR}'
### expect
1
3
### end

### awk_comparison_ne
# Not equal comparison
printf '3\n5\n3\n' | awk '$1 != 3 {print}'
### expect
5
### end

### awk_negation
### skip: logical negation in patterns not implemented
printf '0\n1\n' | awk '!$1 {print "zero"}'
### expect
zero
### end

### awk_index_func
# Index function
printf 'hello world\n' | awk '{print index($0, "world")}'
### expect
7
### end

### awk_sub_func
### skip: regex literal in function args not implemented
printf 'hello hello\n' | awk '{sub(/hello/, "hi"); print}'
### expect
hi hello
### end

### awk_sprintf_func
### skip: sprintf function not implemented
printf '42\n' | awk '{x = sprintf("num=%d", $1); print x}'
### expect
num=42
### end

### awk_int_func
# Int function (truncation)
printf '3.7\n' | awk '{print int($1)}'
### expect
3
### end

### awk_sqrt_func
# Sqrt function
printf '16\n' | awk '{print sqrt($1)}'
### expect
4
### end

### awk_sin_cos_func
### skip: sin/cos not implemented
printf '0\n' | awk '{print sin($1), cos($1)}'
### expect
0 1
### end

### awk_exp_log_func
### skip: exp/log not implemented
printf '1\n' | awk '{print exp($1)}'
### expect
2.71828
### end

### awk_match_func
### skip: match function not implemented
printf 'hello world\n' | awk '{if (match($0, /wor/)) print RSTART, RLENGTH}'
### expect
7 3
### end

### awk_gensub_func
### skip: gensub function not implemented
printf 'hello hello hello\n' | awk '{print gensub(/hello/, "hi", "g")}'
### expect
hi hi hi
### end

### awk_exit_code
### skip: exit with code not implemented
printf 'a\n' | awk '{exit 42}'
### exit_code: 42
### expect
### end

### awk_next_statement
### skip: next statement not implemented
printf '1\n2\n3\n' | awk '{if ($1 == 2) next; print}'
### expect
1
3
### end

### awk_for_loop
### skip: for loops not implemented
printf 'a\n' | awk '{for (i=1; i<=3; i++) print i}'
### expect
1
2
3
### end

### awk_while_loop
### skip: while loops not implemented
printf 'a\n' | awk '{i=1; while (i<=3) {print i; i++}}'
### expect
1
2
3
### end

### awk_do_while_loop
### skip: do-while loop not implemented
printf 'a\n' | awk '{i=1; do {print i; i++} while (i<=3)}'
### expect
1
2
3
### end

### awk_break_statement
### skip: break in loops not implemented
printf 'a\n' | awk '{for (i=1; i<=5; i++) {if (i==3) break; print i}}'
### expect
1
2
### end

### awk_continue_statement
### skip: continue in loops not implemented
printf 'a\n' | awk '{for (i=1; i<=3; i++) {if (i==2) continue; print i}}'
### expect
1
3
### end

### awk_if_else
### skip: if-else newline handling differs
printf '5\n2\n' | awk '{if ($1 > 3) print "big"; else print "small"}'
### expect
big
small
### end

### awk_ternary
### skip: ternary operator not implemented
printf '5\n2\n' | awk '{print ($1 > 3 ? "big" : "small")}'
### expect
big
small
### end

### awk_array_basic
### skip: arrays not implemented
printf 'a\n' | awk 'BEGIN {arr[1]="x"; arr[2]="y"} {print arr[1], arr[2]}'
### expect
x y
### end

### awk_array_in
### skip: arrays not implemented
printf 'a\n' | awk 'BEGIN {arr["key"]="val"} {if ("key" in arr) print "found"}'
### expect
found
### end

### awk_for_in_array
### skip: for-in array iteration not implemented
printf 'a\n' | awk 'BEGIN {a[1]="x"; a[2]="y"} {for (k in a) print k, a[k]}'
### expect
1 x
2 y
### end

### awk_delete_array
### skip: delete array element not implemented
printf 'a\n' | awk 'BEGIN {a[1]="x"; delete a[1]} {print (1 in a) ? "yes" : "no"}'
### expect
no
### end

### awk_getline
### skip: getline not implemented
printf 'line1\nline2\n' | awk '{getline; print}'
### expect
line2
### end

### awk_multiple_patterns
# Multiple pattern-action pairs
printf '1\n2\n3\n' | awk '/1/ {print "one"} /3/ {print "three"}'
### expect
one
three
### end

### awk_regex_match_operator
### skip: regex match operator ~ not implemented
printf 'hello\nworld\nhello world\n' | awk '$0 ~ /hello/ {print NR}'
### expect
1
3
### end

### awk_regex_not_match_operator
### skip: regex not match operator !~ not implemented
printf 'hello\nworld\n' | awk '$0 !~ /hello/ {print}'
### expect
world
### end

### awk_field_assignment
### skip: field assignment not implemented
printf 'a b c\n' | awk '{$2 = "X"; print}'
### expect
a X c
### end

### awk_ofs
# Output field separator OFS
printf 'a b c\n' | awk 'BEGIN {OFS=","} {print $1, $2, $3}'
### expect
a,b,c
### end

### awk_ors
### skip: ORS trailing newline differs
printf 'a\nb\n' | awk 'BEGIN {ORS=";"} {print $0}'
### expect
a;b;
### end

### awk_fs_variable
# FS variable instead of -F
printf 'a:b:c\n' | awk 'BEGIN {FS=":"} {print $2}'
### expect
b
### end

### awk_gsub_string
### skip: gsub with string args not working
printf 'aaa\n' | awk '{gsub("a", "b"); print}'
### expect
bbb
### end

### awk_sub_string
### skip: sub with string args not working
printf 'aaa\n' | awk '{sub("a", "b"); print}'
### expect
baa
### end

### awk_print_no_args
# Print with no arguments prints $0
printf 'hello\n' | awk '{print}'
### expect
hello
### end

### awk_dollar_zero_modification
### skip: $0 assignment not implemented
printf 'a b c\n' | awk '{$0 = "x y z"; print $2}'
### expect
y
### end

### awk_numeric_string_comparison
# Numeric string comparison
printf '10\n2\n' | awk '{if ($1 > 5) print $1}'
### expect
10
### end
