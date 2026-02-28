### wait_basic
### bash_diff: VFS runs background jobs synchronously
# wait returns success
wait
echo $?
### expect
0
### end

### wait_with_pid
### bash_diff: VFS runs background jobs synchronously
# wait with a PID argument
echo "hello" &
wait $!
echo $?
### expect
hello
0
### end
