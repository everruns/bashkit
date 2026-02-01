### mkdir_simple
# Create a directory
mkdir /tmp/testdir
[ -d /tmp/testdir ] && echo ok
### expect
ok
### end

### mkdir_recursive
# Create nested directories with -p
mkdir -p /tmp/a/b/c
[ -d /tmp/a/b/c ] && echo ok
### expect
ok
### end

### mkdir_exists_with_p
# mkdir -p on existing directory should not error
mkdir -p /tmp
echo $?
### expect
0
### end

### touch_create
# Create empty file with touch
touch /tmp/newfile
[ -f /tmp/newfile ] && echo ok
### expect
ok
### end

### rm_file
# Remove a file
echo content > /tmp/toremove
rm /tmp/toremove
[ -f /tmp/toremove ] && echo exists || echo removed
### expect
removed
### end

### rm_force
# rm -f should not error on nonexistent
rm -f /tmp/nonexistent
echo $?
### expect
0
### end

### cp_file
# Copy a file
echo original > /tmp/source
cp /tmp/source /tmp/dest
cat /tmp/dest
### expect
original
### end

### mv_file
# Move a file
echo content > /tmp/oldname
mv /tmp/oldname /tmp/newname
[ -f /tmp/oldname ] && echo old_exists || echo old_gone
[ -f /tmp/newname ] && echo new_exists || echo new_missing
### expect
old_gone
new_exists
### end

### chmod_octal
# Change file permissions
touch /tmp/script
chmod 755 /tmp/script
echo $?
### expect
0
### end
