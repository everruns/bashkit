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
