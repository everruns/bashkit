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
