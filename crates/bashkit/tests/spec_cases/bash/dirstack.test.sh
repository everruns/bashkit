### pushd_basic
# pushd changes directory and pushes old dir
mkdir -p /tmp/pushd_test
pushd /tmp/pushd_test > /dev/null
pwd
### expect
/tmp/pushd_test
### end

### popd_basic
# popd returns to previous directory
mkdir -p /tmp/popd_test
cd /tmp
pushd /tmp/popd_test > /dev/null
popd > /dev/null
pwd
### expect
/tmp
### end

### pushd_shows_stack
# pushd prints directory stack
### bash_diff
mkdir -p /tmp/dir1
cd /tmp
pushd /tmp/dir1
### expect
/tmp/dir1 /tmp
### end

### popd_shows_stack
# popd prints directory stack
### bash_diff
mkdir -p /tmp/dir2
cd /tmp
pushd /tmp/dir2 > /dev/null
popd
### expect
/tmp
### end

### pushd_multiple
# Multiple pushd calls build stack
### bash_diff
mkdir -p /tmp/a /tmp/b
cd /tmp
pushd /tmp/a > /dev/null
pushd /tmp/b > /dev/null
pwd
popd > /dev/null
pwd
popd > /dev/null
pwd
### expect
/tmp/b
/tmp/a
/tmp
### end

### dirs_shows_stack
# dirs displays current stack
### bash_diff
mkdir -p /tmp/d1 /tmp/d2
cd /tmp
pushd /tmp/d1 > /dev/null
pushd /tmp/d2 > /dev/null
dirs
### expect
/tmp/d2 /tmp/d1 /tmp
### end

### dirs_clear
# dirs -c clears the stack
### bash_diff
mkdir -p /tmp/dc
cd /tmp
pushd /tmp/dc > /dev/null
dirs -c
dirs
### expect
/tmp/dc
### end

### dirs_per_line
# dirs -p shows one entry per line
### bash_diff
mkdir -p /tmp/dp1 /tmp/dp2
cd /tmp
pushd /tmp/dp1 > /dev/null
pushd /tmp/dp2 > /dev/null
dirs -p
### expect
/tmp/dp2
/tmp/dp1
/tmp
### end

### popd_empty_stack
# popd on empty stack returns error
popd 2>/dev/null
echo "exit:$?"
### expect
exit:1
### end

### pushd_nonexistent
# pushd to nonexistent directory returns error
pushd /tmp/nonexistent_pushd_dir 2>/dev/null
echo "exit:$?"
### expect
exit:1
### end

### pushd_no_args_empty
# pushd with no args and empty stack returns error
pushd 2>/dev/null
echo "exit:$?"
### expect
exit:1
### end

### pushd_swap
# pushd with no args swaps top two dirs
### bash_diff
mkdir -p /tmp/sw1
cd /tmp
pushd /tmp/sw1 > /dev/null
pushd > /dev/null
pwd
### expect
/tmp
### end
