### ln_symlink
### bash_diff: Bashkit VFS only supports symbolic links
# ln -s creates symbolic link
echo hello > /tmp/target.txt
ln -s /tmp/target.txt /tmp/link.txt
echo "ok"
### expect
ok
### end

### ln_force_overwrite
### bash_diff: Bashkit VFS symlinks
# ln -sf overwrites existing link
echo a > /tmp/force_a.txt
echo b > /tmp/force_b.txt
ln -s /tmp/force_a.txt /tmp/force_link.txt
ln -sf /tmp/force_b.txt /tmp/force_link.txt
echo "ok"
### expect
ok
### end

### ln_no_force_exists
### exit_code:1
# ln fails if link exists without -f
echo a > /tmp/noforce_target.txt
echo b > /tmp/noforce_link.txt
ln -s /tmp/noforce_target.txt /tmp/noforce_link.txt
### expect
### end

### ln_missing_operand
### exit_code:1
### bash_diff: real ln -s with one arg creates link in cwd
# ln with missing operand
ln -s /tmp/only_one
### expect
### end

### ln_default_symbolic
### bash_diff: Bashkit VFS treats all ln as symbolic
# ln without -s still creates link (VFS only supports symlinks)
echo hello > /tmp/def_target.txt
ln /tmp/def_target.txt /tmp/def_link.txt
echo "ok"
### expect
ok
### end

### ln_force_dir_dest_fails
### exit_code:1
### bash_diff: Bashkit VFS rejects ln -sf over a non-empty directory; real ln also fails here.
# Regression: issue #1577. ln -f must not silently overwrite a non-empty
# directory with a symlink — that would orphan its children in the VFS.
echo content > /tmp/force_dir_target.txt
mkdir -p /tmp/force_dir_dest
echo child > /tmp/force_dir_dest/child.txt
ln -sf /tmp/force_dir_target.txt /tmp/force_dir_dest
### expect
### end
