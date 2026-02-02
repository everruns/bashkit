### timeout_basic
# Basic timeout with command that completes in time
timeout 5 echo hello
### expect
hello
### end

### timeout_exit_code_success
# Timeout preserves successful exit code
timeout 5 true
echo $?
### expect
0
### end

### timeout_exit_code_failure
# Timeout preserves failing exit code
timeout 5 false
echo $?
### expect
1
### end

### timeout_seconds_suffix
# Timeout with 's' suffix
timeout 5s echo hello
### expect
hello
### end

### timeout_minutes_suffix
# Timeout with 'm' suffix (capped to 5 minutes)
timeout 1m echo hello
### expect
hello
### end

### timeout_no_duration
# Timeout without duration should error
timeout
echo exit: $?
### expect
exit: 125
### end

### timeout_no_command
# Timeout with duration but no command should error
timeout 5
echo exit: $?
### expect
exit: 125
### end

### timeout_invalid_duration
# Timeout with invalid duration should error
timeout abc echo hello
echo exit: $?
### expect
exit: 125
### end

### timeout_with_args
# Timeout passes arguments to command
timeout 5 echo one two three
### expect
one two three
### end

### timeout_preserve_status_option
# Timeout --preserve-status option (no effect when command completes)
timeout --preserve-status 5 echo hello
### expect
hello
### end

### timeout_with_pipeline_stdin
# Timeout passes stdin from pipeline to command
echo "input data" | timeout 5 cat
### expect
input data
### end

### timeout_expired
### skip: timing-dependent test, verified manually
# Timeout that expires should return 124
timeout 0.001 sleep 10
echo $?
### expect
124
### end

### timeout_zero
### skip: timing-dependent test, verified manually
# Timeout of 0 should timeout immediately
timeout 0 sleep 1
echo $?
### expect
124
### end

### timeout_builtin_command
# Timeout with builtin command
timeout 5 printf "hello\n"
### expect
hello
### end

### timeout_with_variable
# Timeout with variable in command
msg="world"
timeout 5 echo hello $msg
### expect
hello world
### end

### timeout_nested
# Nested timeout commands
timeout 5 timeout 3 echo nested
### expect
nested
### end
