### watch_basic
### bash_diff: watch shows one-time output in VFS, no continuous execution
# watch displays the command info
watch echo hello 2>&1 | head -1
### expect
Every 2.0s: echo hello
### end

### watch_custom_interval
### bash_diff: watch shows one-time output in VFS
# watch -n sets interval
watch -n 5 echo test 2>&1 | head -1
### expect
Every 5.0s: echo test
### end
