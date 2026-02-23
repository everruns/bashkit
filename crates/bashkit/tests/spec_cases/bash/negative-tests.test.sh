### neg_array_indices_empty
### skip: empty array indices expansion outputs extra newline
# Empty array has no indices
arr=(); echo "${!arr[@]}"
### expect

### end

### neg_test_nonexistent_file
# Test non-existent file returns false
### exit_code:1
[ -e /nonexistent/path ]
### expect
### end

### neg_test_file_not_directory
# Regular file is not a directory
echo test > /tmp/notdir.txt
### exit_code:1
[ -d /tmp/notdir.txt ]
### expect
### end

### neg_brace_no_expand_no_comma
# Single item in braces doesn't expand
echo {item}
### expect
{item}
### end

### neg_brace_no_expand_space
### skip: brace with space parsing issue
# Brace with space doesn't expand
echo { a,b,c }
### expect
{ a,b,c }
### end

### neg_arith_logical_and_zero
# 0 && anything is 0
echo $((0 && 5))
### expect
0
### end

### neg_arith_logical_or_both_zero
# 0 || 0 is 0
echo $((0 || 0))
### expect
0
### end

### neg_string_compare_equal
# Equal strings not less/greater
[ "abc" \< "abc" ] && echo "less" || echo "not less"
### expect
not less
### end

### neg_numeric_not_equal
# Different numbers not equal
[ 5 -eq 3 ] && echo "equal" || echo "not equal"
### expect
not equal
### end

### neg_test_empty_string
# Empty string is false for -n
[ -n "" ] && echo "nonempty" || echo "empty"
### expect
empty
### end

### neg_test_nonempty_for_z
# Non-empty string is false for -z
[ -z "text" ] && echo "empty" || echo "nonempty"
### expect
nonempty
### end

### neg_errexit_stops
### skip: errexit timing with set -e not working correctly
# set -e stops execution on error
### exit_code:1
set -e
false
echo "should not reach here"
### expect
### end

### neg_range_no_middle
# Range expansion requires two values
echo {1..}
### expect
{1..}
### end
