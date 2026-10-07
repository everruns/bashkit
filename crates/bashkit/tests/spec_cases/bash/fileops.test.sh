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

### touch_t_sets_file_mtime
# touch -t should set the file mtime
echo "test" > /tmp/touch_timestamp.txt
touch -t 202604061200.00 /tmp/touch_timestamp.txt
date -r /tmp/touch_timestamp.txt +%Y%m%d%H%M.%S
### expect
202604061200.00
### end

### mktemp_creates_file
# mktemp creates a temp file and prints its path
f=$(mktemp)
[ -f "$f" ] && echo "ok"
### expect
ok
### end

### mktemp_in_tmp
# mktemp creates file under /tmp
f=$(mktemp)
echo "$f" | grep -q "^/tmp/" && echo "in_tmp"
### expect
in_tmp
### end

### mktemp_directory
# mktemp -d creates a directory
d=$(mktemp -d)
[ -d "$d" ] && echo "ok"
### expect
ok
### end

### mktemp_template
# mktemp with template replaces XXXXXX
f=$(mktemp /tmp/myapp.XXXXXX)
echo "$f" | grep -q "^/tmp/myapp\." && echo "matched"
[ -f "$f" ] && echo "exists"
### expect
matched
exists
### end

### mktemp_dir_template
# mktemp -d with template
d=$(mktemp -d /tmp/mydir.XXXXXX)
echo "$d" | grep -q "^/tmp/mydir\." && echo "matched"
[ -d "$d" ] && echo "exists"
### expect
matched
exists
### end

### mktemp_unique
# mktemp creates unique names
f1=$(mktemp)
f2=$(mktemp)
[ "$f1" != "$f2" ] && echo "unique"
### expect
unique
### end

### mktemp_p_flag
# mktemp -p uses specified directory
### bash_diff
mkdir -p /tmp/custom
f=$(mktemp -p /tmp/custom)
echo "$f" | grep -q "^/tmp/custom/" && echo "in_custom"
[ -f "$f" ] && echo "exists"
### expect
in_custom
exists
### end

### touch_date_reference_nocreate
rm -rf /tmp/tdr && mkdir -p /tmp/tdr && cd /tmp/tdr
TZ=UTC touch -d '2001-02-03 04:05:06' old
TZ=UTC date -r old '+%Y-%m-%d %H:%M:%S' 2>/dev/null || stat -c %Y old
touch -r old copy
[ old -nt copy ] || [ copy -nt old ] || echo same-mtime
touch -c missing; ls missing 2>/dev/null || echo not-created
touch -cm --date=@0 old; TZ=UTC date -r old +%Y
touch -t 200102030405.06 stamp; TZ=UTC date -r stamp '+%m/%d %H:%M:%S'
touch --bogus 2>/dev/null; echo rc=$?
### expect
2001-02-03 04:05:06
same-mtime
not-created
1970
02/03 04:05:06
rc=1
### end

### cp_recursive_tree
# cp -r copies a tree, refuses to copy into itself, and skips dirs without -r
rm -rf /tmp/cpr && mkdir -p /tmp/cpr/a/b && cd /tmp/cpr
echo x > a/b/f; ln -s f a/b/l
cp -r a c; cat c/b/f; readlink c/b/l
cp -R a c; ls c
cp -r a/ a/z 2>&1; echo "exit=$?"
mkdir q; cp q q2 2>&1; echo "exit=$?"
### expect
x
f
a
b
cp: cannot copy a directory, 'a/', into itself, 'a/z'
exit=1
cp: -r not specified; omitting directory 'q'
exit=1
### end

### rmdir_verbose_parents
# rmdir -v reports each removal; -p walks the operand's prefixes
rm -rf /tmp/rmv && mkdir -p /tmp/rmv/a/b/c /tmp/rmv/x/y && cd /tmp/rmv
rmdir -v a/b/c; ls a
rmdir -pv x/y
touch a/b/f; rmdir a/b 2>&1; rmdir --ignore-fail-on-non-empty a/b; echo "exit=$?"
### expect
rmdir: removing directory, 'a/b/c'
b
rmdir: removing directory, 'x/y'
rmdir: removing directory, 'x'
rmdir: failed to remove 'a/b': Directory not empty
exit=0
### end

### mktemp_template_rules
# mktemp replaces the trailing X run, keeps a relative template relative
rm -rf /tmp/mkt && mkdir -p /tmp/mkt && cd /tmp/mkt
mktemp fooXXXX | grep -c '^foo[A-Za-z0-9]\{4\}$'
mktemp -u -p /tmp/mkt barXXX.txt | grep -c '^/tmp/mkt/bar...\.txt$'
mktemp fooXX 2>&1; echo "exit=$?"
mktemp -q -p /nonexist fooXXX; echo "exit=$?"
### expect
1
1
mktemp: too few X's in template 'fooXX'
exit=1
exit=1
### end
