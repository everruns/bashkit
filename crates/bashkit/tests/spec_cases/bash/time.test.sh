### time_basic
# Basic time command should output timing to stderr
time echo hello
### expect
hello
### end

### time_exit_code_preserved
# Time should preserve the exit code of the command
time true
echo $?
### expect
0
### end

### time_exit_code_failure
# Time should preserve failing exit code
time false
echo $?
### expect
1
### end

### time_no_command
# Time with no command should just output timing
time
echo done
### expect
done
### end

### time_pipeline
# Time can wrap a pipeline
time echo hello | cat
### expect
hello
### end

### time_posix_format
# Time -p uses POSIX format (simpler output)
time -p echo hello
### expect
hello
### end

### time_subshell
# Time a subshell
time (echo one; echo two)
### expect
one
two
### end

### time_compound_command
# Time a brace group
time { echo a; echo b; }
### expect
a
b
### end

### time_loop
# Time a for loop
time for i in 1 2; do echo $i; done
### expect
1
2
### end

### time_with_variable
# Time with variable expansion
msg="world"
time echo hello $msg
### expect
hello world
### end

### time_exit_code_from_pipeline
# Time preserves pipeline exit code
time true | false
echo $?
### expect
1
### end
