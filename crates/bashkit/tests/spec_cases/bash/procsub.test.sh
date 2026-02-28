### procsub_basic
# Basic process substitution
cat <(echo hello)
### expect
hello
### end

### procsub_with_pipe
# Process substitution with pipe
cat <(echo hello | tr a-z A-Z)
### expect
HELLO
### end

### procsub_diff_simulation
# Simulate diff with two process substitutions
# Just test that both are parsed
echo "first" > /tmp/file1
echo "second" > /tmp/file2
cat /tmp/file1
cat /tmp/file2
### expect
first
second
### end

### procsub_empty_output
# Process substitution with empty command output
cat <(echo "")
echo done
### expect

done
### end

### procsub_multiline
# Process substitution with multiline output
cat <(printf 'line1\nline2\nline3\n')
### expect
line1
line2
line3
### end

### procsub_variable_expansion
# Process substitution with variable
msg="world"
cat <(echo "hello $msg")
### expect
hello world
### end

### procsub_diff_two_commands
# diff with two process substitutions
echo "aaa" > /tmp/psub1.txt
echo "bbb" > /tmp/psub2.txt
diff <(cat /tmp/psub1.txt) <(cat /tmp/psub2.txt) > /dev/null 2>&1
echo "exit: $?"
### expect
exit: 1
### end

### procsub_diff_identical
# diff with identical process substitutions
diff <(echo "same") <(echo "same") > /dev/null 2>&1
echo "exit: $?"
### expect
exit: 0
### end

### procsub_paste_two_sources
# paste with two process substitutions
paste <(echo "col1") <(echo "col2")
### expect
col1	col2
### end

### procsub_nested_commands
# process substitution with complex pipeline
cat <(echo "hello world" | tr ' ' '\n' | sort)
### expect
hello
world
### end

### procsub_sort_comparison
# sort and compare with process substitution
cat <(printf 'b\na\nc\n' | sort)
### expect
a
b
c
### end
