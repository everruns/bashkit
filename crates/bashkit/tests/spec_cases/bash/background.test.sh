### background_simple
# Background execution with &
echo hello &
echo world
### expect
hello
world
### end

### background_multiple
# Multiple background commands
echo first &
echo second &
echo third
### expect
first
second
third
### end
