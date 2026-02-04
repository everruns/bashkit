### subst_simple
# Simple command substitution
echo $(echo hello)
### expect
hello
### end

### subst_in_string
# Command substitution in string
echo "result: $(echo 42)"
### expect
result: 42
### end

### subst_pipeline
# Command substitution with pipeline
echo $(echo hello | cat)
### expect
hello
### end

### subst_assign
# Assign command substitution to variable
VAR=$(echo test); echo $VAR
### expect
test
### end

### subst_nested
# Nested command substitution
echo $(echo $(echo deep))
### expect
deep
### end

### subst_multiline
# Multi-line output
echo "$(printf 'a\nb\nc')"
### expect
a
b
c
### end

### subst_with_args
# Command with arguments
echo $(printf '%s %s' hello world)
### expect
hello world
### end

### subst_arithmetic
# Command in arithmetic context
X=$(echo 5); echo $((X + 3))
### expect
8
### end

### subst_in_condition
# Command substitution in condition
if [ "$(echo yes)" = "yes" ]; then echo matched; fi
### expect
matched
### end

### subst_exit_code
### skip: exit code propagation from command substitution not implemented
# Exit code from command substitution
result=$(false); echo $?
### expect
1
### end

### subst_backtick
### skip: backtick command substitution not implemented
echo `echo hello`
### expect
hello
### end

### subst_multiple
# Multiple substitutions
echo $(echo a) $(echo b) $(echo c)
### expect
a b c
### end

### subst_with_variable
# Substitution using variable
NAME=test; echo $(echo $NAME)
### expect
test
### end

### subst_strip_trailing_newlines
# Command substitution strips trailing newlines
VAR=$(printf 'hello\n\n\n'); echo "x${VAR}y"
### expect
xhelloy
### end
