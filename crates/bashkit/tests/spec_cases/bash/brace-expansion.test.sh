### brace_simple
# Simple brace expansion with comma
echo {a,b,c}
### expect
a b c
### end

### brace_prefix
# Brace expansion with prefix
echo pre{a,b,c}
### expect
prea preb prec
### end

### brace_suffix
# Brace expansion with suffix
echo {a,b,c}suf
### expect
asuf bsuf csuf
### end

### brace_prefix_suffix
# Brace expansion with both prefix and suffix
echo pre{a,b,c}suf
### expect
preasuf prebsuf precsuf
### end

### brace_numeric_range
# Numeric range expansion
echo {1..5}
### expect
1 2 3 4 5
### end

### brace_alpha_range
# Alphabetic range expansion
echo {a..e}
### expect
a b c d e
### end

### brace_range_prefix
# Range with prefix
echo file{1..3}.txt
### expect
file1.txt file2.txt file3.txt
### end

### brace_nested
# Nested brace expansion
echo {a,b}{1,2}
### expect
a1 a2 b1 b2
### end

### brace_empty_item
### bash_diff: Bashkit preserves leading space from empty brace item, bash strips it
# Brace with empty item
echo {,a,b}
### expect
 a b
### end

### brace_no_expand_single
# Single item doesn't expand
echo {single}
### expect
{single}
### end

### brace_reverse_range
# Reverse numeric range
echo {5..1}
### expect
5 4 3 2 1
### end

### brace_in_for_numeric_range
# Numeric range brace expansion in for-loop word list
for i in {1..5}; do echo $i; done
### expect
1
2
3
4
5
### end

### brace_in_for_comma
# Comma brace expansion in for-loop
for x in {a,b,c}; do echo $x; done
### expect
a
b
c
### end

### brace_in_for_with_prefix
# Brace expansion with prefix in for-loop
for f in file{1..3}.txt; do echo $f; done
### expect
file1.txt
file2.txt
file3.txt
### end

### brace_in_for_nested
# Nested brace expansion in for-loop
for x in {a,b}{1,2}; do echo $x; done
### expect
a1
a2
b1
b2
### end

### brace_in_for_reverse_range
# Reverse range in for-loop
for i in {3..1}; do echo $i; done
### expect
3
2
1
### end

### brace_in_for_mixed_words
# Mix of plain words and brace expansion in for-loop
for x in hello {1..3} world; do echo $x; done
### expect
hello
1
2
3
world
### end

### brace_in_for_alpha_range
# Alpha range in for-loop
for c in {a..d}; do echo $c; done
### expect
a
b
c
d
### end

### brace_in_for_quoted_skip
# Quoted braces should NOT expand in for-loop
for x in "{1..3}"; do echo "$x"; done
### expect
{1..3}
### end

### brace_in_for_single_no_expand
# Single item braces don't expand in for-loop
for x in {only}; do echo $x; done
### expect
{only}
### end
