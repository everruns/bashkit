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
# String less than comparison
[ "apple" \< "banana" ] && echo "less"
### expect
less
### end

### test_string_gt
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

### test_file_newer_than
# Test -nt (file1 newer than file2)
### bash_diff
echo "old" > /tmp/old.txt
sleep 0.01
echo "new" > /tmp/new.txt
[ /tmp/new.txt -nt /tmp/old.txt ] && echo "newer"
### expect
newer
### end

### test_file_older_than
# Test -ot (file1 older than file2)
### bash_diff
echo "first" > /tmp/first.txt
sleep 0.01
echo "second" > /tmp/second.txt
[ /tmp/first.txt -ot /tmp/second.txt ] && echo "older"
### expect
older
### end

### test_file_nt_nonexistent
# -nt returns true if left exists and right doesn't
echo "exists" > /tmp/exists_nt.txt
[ /tmp/exists_nt.txt -nt /tmp/nonexistent_nt ] && echo "newer"
### expect
newer
### end

### test_file_ot_nonexistent
# -ot returns true if left doesn't exist and right does
echo "exists" > /tmp/exists_ot.txt
[ /tmp/nonexistent_ot -ot /tmp/exists_ot.txt ] && echo "older"
### expect
older
### end

### test_file_ef_same_path
# -ef returns true for same file
echo "data" > /tmp/ef_test.txt
[ /tmp/ef_test.txt -ef /tmp/ef_test.txt ] && echo "same"
### expect
same
### end

### test_file_ef_different_path
# -ef returns false for different files
echo "a" > /tmp/ef_a.txt
echo "b" > /tmp/ef_b.txt
[ /tmp/ef_a.txt -ef /tmp/ef_b.txt ] || echo "different"
### expect
different
### end

### test_file_nt_both_nonexistent
# -nt returns false if both don't exist
[ /tmp/no1 -nt /tmp/no2 ] || echo "false"
### expect
false
### end

### test_cond_nt
# [[ ]] also supports -nt
### bash_diff
echo "old" > /tmp/c_old.txt
sleep 0.01
echo "new" > /tmp/c_new.txt
[[ /tmp/c_new.txt -nt /tmp/c_old.txt ]] && echo "newer"
### expect
newer
### end

### test_cond_ot
# [[ ]] also supports -ot
### bash_diff
echo "first" > /tmp/c_first.txt
sleep 0.01
echo "second" > /tmp/c_second.txt
[[ /tmp/c_first.txt -ot /tmp/c_second.txt ]] && echo "older"
### expect
older
### end

### test_cond_ef
# [[ ]] also supports -ef
echo "data" > /tmp/c_ef.txt
[[ /tmp/c_ef.txt -ef /tmp/c_ef.txt ]] && echo "same"
### expect
same
### end
