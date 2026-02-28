### history_basic
### bash_diff: VFS has no persistent history tracking
# history runs without error
history
echo $?
### expect
0
### end

### history_clear
### bash_diff: VFS has no persistent history
# history -c clears history successfully
history -c
echo $?
### expect
0
### end
