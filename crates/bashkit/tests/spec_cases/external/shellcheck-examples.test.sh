### External test cases imported from ShellCheck wiki examples
### These tests verify BashKit compatibility with common bash idioms.

### sc_variable_quoting
# Variable quoting patterns (SC2086)
x="hello world"; echo "$x"
### expect
hello world
### end

### sc_command_subst
# Command substitution patterns (SC2046)
x=$(pwd); echo "$x" > /dev/null; echo ok
### expect
ok
### end

### sc_arithmetic
# Arithmetic patterns (SC2004)
x=5; echo $((x + 1))
### expect
6
### end

### sc_exit_status
# Exit status checks (SC2181)
true; if [ $? -eq 0 ]; then echo success; fi
### expect
success
### end

### sc_string_compare
# String comparison (SC2072)
x=hello; if [ "$x" = "hello" ]; then echo match; fi
### expect
match
### end

### sc_here_string
### skip: here strings not implemented
# Here strings (SC2059)
cat <<< "hello"
### expect
hello
### end

### sc_process_subst
### skip: process substitution not implemented
# Process substitution (SC2024)
cat <(echo hello)
### expect
hello
### end

### sc_brace_expansion
### skip: brace expansion not implemented
# Brace expansion (SC2039)
echo {a,b,c}
### expect
a b c
### end

### sc_default_unset
# Default value for unset variable
unset x; echo ${x:-default}
### expect
default
### end

### sc_default_set
# Default value when set
x=set; echo ${x:-default}
### expect
set
### end

### sc_length
# Length expansion
x=hello; echo ${#x}
### expect
5
### end

### sc_substring
### skip: substring expansion not implemented
# Substring expansion
x=hello; echo ${x:0:2}
### expect
he
### end
