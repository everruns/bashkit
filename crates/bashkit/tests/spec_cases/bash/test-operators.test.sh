### test_file_exists
# Test file exists (-e)
echo test > /tmp/exists.txt
[ -e /tmp/exists.txt ] && echo "exists"
### expect
exists
### end

### test_file_not_exists
# Test file not exists
[ -e /tmp/nonexistent ] || echo "not found"
### expect
not found
### end

### test_file_regular
# Test regular file (-f)
echo test > /tmp/regular.txt
[ -f /tmp/regular.txt ] && echo "regular"
### expect
regular
### end

### test_directory
# Test directory (-d)
mkdir -p /tmp/testdir
[ -d /tmp/testdir ] && echo "dir"
### expect
dir
### end

### test_readable
# Test readable (-r)
echo test > /tmp/readable.txt
[ -r /tmp/readable.txt ] && echo "readable"
### expect
readable
### end

### test_writable
# Test writable (-w)
echo test > /tmp/writable.txt
[ -w /tmp/writable.txt ] && echo "writable"
### expect
writable
### end

### test_nonempty
# Test non-empty file (-s)
echo "content" > /tmp/nonempty.txt
[ -s /tmp/nonempty.txt ] && echo "nonempty"
### expect
nonempty
### end

### test_empty_file
# Test empty file is falsy for -s
> /tmp/empty.txt
[ -s /tmp/empty.txt ] || echo "empty"
### expect
empty
### end

### test_string_lt
### skip: test string comparison with \< not fully implemented
# String less than comparison
[ "apple" \< "banana" ] && echo "less"
### expect
less
### end

### test_string_gt
### skip: test string comparison with \> not fully implemented
# String greater than comparison
[ "zoo" \> "apple" ] && echo "greater"
### expect
greater
### end

### test_string_equal
# String equality
[ "hello" = "hello" ] && echo "equal"
### expect
equal
### end

### test_string_not_equal
# String inequality
[ "hello" != "world" ] && echo "not equal"
### expect
not equal
### end

### test_numeric_eq
# Numeric equality
[ 5 -eq 5 ] && echo "equal"
### expect
equal
### end

### test_numeric_lt
# Numeric less than
[ 3 -lt 5 ] && echo "less"
### expect
less
### end

### test_negation
# Test negation with !
[ ! -e /tmp/nonexistent ] && echo "negated"
### expect
negated
### end

### test_and_operator
# Test -a (AND) operator
[ 1 = 1 -a 2 = 2 ] && echo "both true"
### expect
both true
### end

### test_or_operator
# Test -o (OR) operator
[ 1 = 2 -o 2 = 2 ] && echo "one true"
### expect
one true
### end
