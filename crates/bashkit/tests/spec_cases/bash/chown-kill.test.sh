### chown_basic
### bash_diff: VFS chown is a no-op, no real ownership
# chown accepts owner:group syntax
echo hello > /tmp/chown_test.txt
chown root:root /tmp/chown_test.txt
echo $?
### expect
0
### end

### chown_recursive
### bash_diff: VFS chown is a no-op
# chown -R accepted
mkdir -p /tmp/chown_dir
echo a > /tmp/chown_dir/file.txt
chown -R user:user /tmp/chown_dir
echo $?
### expect
0
### end

### chown_missing_operand
### exit_code:1
# chown with missing operand
chown root
### expect
### end

### chown_nonexistent_file
### exit_code:1
# chown on nonexistent file
chown root:root /tmp/nonexistent_chown_xyz
### expect
### end

### kill_list_signals
# kill -l lists signal names
kill -l | grep -q HUP && echo "ok"
### expect
ok
### end

### kill_no_args
### exit_code:2
# kill with no PID
kill
### expect
### end

### kill_noop
### bash_diff: VFS has no real processes
# kill accepts PID and succeeds (no-op in VFS)
kill -0 1
echo $?
### expect
0
### end
