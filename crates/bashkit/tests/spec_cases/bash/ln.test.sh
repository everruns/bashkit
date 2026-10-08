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

### ln_force_dir_dest_links_inside
# Regression: issue #1577. ln -f must never replace a non-empty directory
# with a symlink (that would orphan its children in the VFS). Like GNU ln, a
# directory destination receives the link inside it instead.
echo content > /tmp/force_dir_target.txt
mkdir -p /tmp/force_dir_dest
echo child > /tmp/force_dir_dest/child.txt
ln -sf /tmp/force_dir_target.txt /tmp/force_dir_dest
cat /tmp/force_dir_dest/child.txt
readlink /tmp/force_dir_dest/force_dir_target.txt
### expect
child
/tmp/force_dir_target.txt
### end

### ln_force_T_over_directory_fails
### exit_code:1
# -T treats the destination as the link itself; a real directory is never replaced
mkdir -p /tmp/lnTdir/dest
ln -sfT /tmp/lnTdir/x /tmp/lnTdir/dest 2>/dev/null
### expect
### end

### ln_sfn_replaces_dir_symlink
# ln -sfn swaps a symlink that points at a directory (release switch idiom)
mkdir -p /tmp/lnn/releases/v1 /tmp/lnn/releases/v2
cd /tmp/lnn
ln -s releases/v1 current
ln -sfn releases/v2 current
readlink current
ls releases/v1
### expect
releases/v2
### end

### ln_sf_follows_dir_symlink
# without -n an existing link to a directory is followed: the new link lands inside it
mkdir -p /tmp/lnd/releases/v1 /tmp/lnd/releases/v2
cd /tmp/lnd
ln -s releases/v1 current
ln -sf releases/v2 current
readlink current
ls releases/v1
### expect
releases/v1
v2
### end

### ln_into_directory
# a directory destination gets DIR/basename(TARGET)
mkdir -p /tmp/lni/d
echo x > /tmp/lni/t.txt
ln -s /tmp/lni/t.txt /tmp/lni/d
readlink /tmp/lni/d/t.txt
### expect
/tmp/lni/t.txt
### end

### ln_multiple_targets_into_directory
mkdir -p /tmp/lnm/d
cd /tmp/lnm
ln -s /x/a /x/b d
readlink d/a d/b
### expect
/x/a
/x/b
### end

### ln_sfT_no_target_directory
mkdir -p /tmp/lnt/a /tmp/lnt/b
cd /tmp/lnt
ln -s a cur
ln -sfT b cur
readlink cur
### expect
b
### end

### ln_verbose_and_long_options
cd /tmp/
ln --symbolic --verbose tgt_v lnk_v
ln --symbolic --force --no-dereference tgt_w lnk_v
readlink lnk_v
### expect
'lnk_v' -> 'tgt_v'
tgt_w
### end
