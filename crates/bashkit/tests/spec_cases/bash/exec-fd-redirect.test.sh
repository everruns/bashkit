### exec_fd_to_dev_null
# exec N>/dev/null should discard writes to fd N
exec 3>/dev/null
echo "discarded" >&3
exec 3>&-
echo "visible"
### expect
visible
### end

### exec_fd_to_file
# exec N>file should redirect writes to fd N into file
exec 3>/tmp/fd_test_out.txt
echo "captured" >&3
exec 3>&-
cat /tmp/fd_test_out.txt
### expect
captured
### end

### exec_fd_dup_stdout
# exec 3>&1 should duplicate stdout to fd 3
exec 3>&1
echo "on fd3" >&3
exec 3>&-
### expect
on fd3
### end

### exec_fd_close
# exec 3>&- should close fd 3
exec 3>/dev/null
exec 3>&-
echo "closed ok"
### expect
closed ok
### end

### fd3_redirect_pattern
# { cmd 1>&3; cmd; } 3>&1 >file — fd3 captures original stdout (issue #1115)
{ echo "progress" 1>&3; echo "file content"; } 3>&1 > /tmp/test_fd.txt
cat /tmp/test_fd.txt
### expect
progress
file content
### end

### high_fd_file_redirect_keeps_stdout
# `N>file` (N>=3) opens fd N only; stdout still reaches the terminal.
d=$(mktemp -d); cd "$d"
echo pre > g
echo visible 4>g
wc -c < g
echo z 5>>h; [ -f h ] && echo created
### expect
visible
0
z
created
### end

### compound_high_fd_file_redirect
# Writes to fd N inside the block land in the file; stdout is untouched.
d=$(mktemp -d); cd "$d"
{ echo to-fd >&3; echo out; } 3>f
cat f
( echo sub ) 9>lk; [ -f lk ] && echo lock-created
for i in 1; do echo loop; echo app >&3; done 3>>f
cat f
### expect
out
to-fd
sub
lock-created
loop
to-fd
app
### end

### function_high_fd_file_redirect
d=$(mktemp -d); cd "$d"
f() { echo fn-out; echo fn-fd >&3; }
f 3>a
cat a
### expect
fn-out
fn-fd
### end
