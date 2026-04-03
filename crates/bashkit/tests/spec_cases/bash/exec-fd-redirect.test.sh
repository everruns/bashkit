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
