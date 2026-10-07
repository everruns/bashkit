# /dev/stdout, /dev/stderr, /dev/fd/N as redirection targets alias the
# shell's current file descriptors (Linux semantics).

### dev_stderr_redirect
{ echo err > /dev/stderr; echo out; } 2>/dev/null
### expect
out
### end

### dev_stderr_captured_by_2to1
{ echo err > /dev/stderr; } 2>&1
### expect
err
### end

### dev_stderr_append
{ echo a >> /dev/stderr; echo b >> /dev/stderr; } 2>&1
### expect
a
b
### end

### dev_stdout_from_stderr
{ echo e > /dev/stderr; } 2>/dev/stdout
### expect
e
### end

### dev_fd_1_and_2
{ echo one > /dev/fd/2; echo two > /dev/fd/1; } 2>/dev/null
### expect
two
### end

### dev_fd_exec_opened
exec 3>/tmp/devfd_log
echo hi > /dev/fd/3
exec 3>&-
cat /tmp/devfd_log
rm -f /tmp/devfd_log
### expect
hi
### end

### dev_stderr_via_variable
LOG=/dev/stderr
{ echo warn >> "$LOG"; } 2>&1
### expect
warn
### end

### dev_stdin_input
echo piped | cat < /dev/stdin
### expect
piped
### end

### builtin_operand_dev_stdin
echo hi | cat /dev/stdin
echo w | grep w /dev/stdin
### expect
hi
w
### end

### builtin_operand_tee_dev_stderr
echo t | tee /dev/stderr 2>&1 >/dev/null
### expect
t
### end
