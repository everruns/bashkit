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

### rm_nonexistent_error
# rm without -f should error on nonexistent
rm /tmp/does_not_exist_at_all 2>/dev/null
echo $?
### expect
1
### end

### mkdir_nested_error
# mkdir without -p should error on nested path
mkdir /tmp/nonexistent_parent/child 2>/dev/null
echo $?
### expect
1
### end

### cp_missing_source
# cp with missing source should error
cp /tmp/source_not_here /tmp/dest 2>/dev/null
echo $?
### expect
1
### end

### mv_missing_source
# mv with missing source should error
mv /tmp/source_not_here /tmp/dest 2>/dev/null
echo $?
### expect
1
### end

### touch_multiple
# touch can create multiple files
touch /tmp/file1 /tmp/file2 /tmp/file3
[ -f /tmp/file1 ] && [ -f /tmp/file2 ] && [ -f /tmp/file3 ] && echo ok
### expect
ok
### end

### chmod_missing_file
# chmod on missing file should error
chmod 644 /tmp/missing_file_here 2>/dev/null
echo $?
### expect
1
### end

### mkdir_on_existing_file
# mkdir should fail when file exists at path
echo test > /tmp/existingfile
mkdir /tmp/existingfile 2>/dev/null
echo $?
### expect
1
### end

### mkdir_p_on_existing_file
# mkdir -p should also fail when file exists at path
echo test > /tmp/existingfile2
mkdir -p /tmp/existingfile2 2>/dev/null
echo $?
### expect
1
### end

### redirect_to_directory
# Writing to directory should fail
mkdir -p /tmp/existingdir
echo test > /tmp/existingdir 2>/dev/null
echo $?
### expect
1
### end

### append_to_directory
# Appending to directory should fail
mkdir -p /tmp/appenddir
echo test >> /tmp/appenddir 2>/dev/null
echo $?
### expect
1
### end

### cat_redirect_to_directory
# cat redirect to directory should fail
mkdir -p /tmp/catdir
cat <<< "test" > /tmp/catdir 2>/dev/null
echo $?
### expect
1
### end

### touch_existing_directory
# touch on existing directory should succeed (updates mtime)
mkdir -p /tmp/touchdir
touch /tmp/touchdir
echo $?
### expect
0
### end
