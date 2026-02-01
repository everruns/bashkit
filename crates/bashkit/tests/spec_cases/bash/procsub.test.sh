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
