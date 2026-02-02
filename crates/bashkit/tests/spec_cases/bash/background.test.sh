### background_simple
### bash_diff: Background command output order is non-deterministic
# Background execution with &
echo hello &
echo world
### expect
hello
world
### end

### background_multiple
### bash_diff: Background command output order is non-deterministic
# Multiple background commands
echo first &
echo second &
echo third
### expect
first
second
third
### end
