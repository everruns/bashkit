### command_not_found_exit_code
# Unknown command returns exit code 127
nonexistent_command_xyz
echo $?
### expect
127
### exit_code: 0
### end

### command_not_found_continues_script
# Script continues after command not found
unknown_cmd_abc
echo after
### expect
after
### exit_code: 0
### end

### command_not_found_or_fallback
# Or operator provides fallback after failure
nonexistent || echo fallback
### expect
fallback
### exit_code: 0
### end

### command_not_found_and_stops
# And operator stops on failure
nonexistent && echo success
echo done
### expect
done
### exit_code: 0
### end

### command_not_found_if_else
# Conditional takes else branch on command not found
if nonexistent_cmd; then echo yes; else echo no; fi
### expect
no
### exit_code: 0
### end

### command_not_found_pipeline_exit
# Pipeline exit code is from last command
echo hello | nonexistent_filter
echo $?
### expect
127
### exit_code: 0
### end

### command_not_found_subshell
# Subshell propagates exit code 127
(nonexistent_in_subshell)
echo $?
### expect
127
### exit_code: 0
### end

### builtin_echo_works
# Verify builtin echo works correctly
echo hello world
### expect
hello world
### end

### builtin_true_false
# Verify true/false builtins work
true
echo $?
false
echo $?
### expect
0
1
### exit_code: 0
### end
