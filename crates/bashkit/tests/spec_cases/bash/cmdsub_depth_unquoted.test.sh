# Command substitution depth limit in unquoted context
# Regression tests for issue #996

### shallow_cmdsub_unquoted
# Shallow command substitution in unquoted context works
x=$(echo hello)
echo $x
### expect
hello
### end

### nested_cmdsub_unquoted
# Nested command substitution in unquoted context works
x=$(echo $(echo nested))
echo $x
### expect
nested
### end

### cmdsub_depth_3_unquoted
# Three levels of nesting works
x=$(echo $(echo $(echo deep)))
echo $x
### expect
deep
### end
