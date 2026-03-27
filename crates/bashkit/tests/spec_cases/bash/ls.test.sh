### ls_file_preserves_path
# ls with file arguments should preserve the full path in output
mkdir -p /tmp/lsdir
echo x > /tmp/lsdir/a.md
echo y > /tmp/lsdir/b.md
ls /tmp/lsdir/a.md /tmp/lsdir/b.md
### expect
/tmp/lsdir/a.md
/tmp/lsdir/b.md
### end

### ls_file_preserves_path_sorted_by_time
# ls -t with file arguments should preserve the full path in output
mkdir -p /tmp/lstdir
echo x > /tmp/lstdir/a.md
sleep 0.01
echo y > /tmp/lstdir/b.md
ls -t /tmp/lstdir/a.md /tmp/lstdir/b.md
### expect
/tmp/lstdir/b.md
/tmp/lstdir/a.md
### end

### ls_directory_shows_filenames_only
# ls on a directory should show filenames only, not full paths
mkdir -p /tmp/lsdironly
echo x > /tmp/lsdironly/file1.txt
echo y > /tmp/lsdironly/file2.txt
ls /tmp/lsdironly
### expect
file1.txt
file2.txt
### end

### ls_single_file_preserves_path
# ls with a single file argument should preserve the full path
mkdir -p /tmp/lssingle
echo x > /tmp/lssingle/test.txt
ls /tmp/lssingle/test.txt
### expect
/tmp/lssingle/test.txt
### end
