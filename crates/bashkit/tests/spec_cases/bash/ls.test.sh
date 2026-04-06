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

### ls_classify_directory
# ls -F should append / to directories
mkdir -p /tmp/lsclass/subdir
echo x > /tmp/lsclass/file.txt
ls -F /tmp/lsclass
### expect
file.txt
subdir/
### end

### ls_classify_executable
# ls -F should append * to executable files
mkdir -p /tmp/lsexec
echo x > /tmp/lsexec/script.sh
chmod 755 /tmp/lsexec/script.sh
echo y > /tmp/lsexec/data.txt
ls -F /tmp/lsexec
### expect
data.txt
script.sh*
### end

### ls_classify_file_arg
# ls -F with file argument should append indicator
mkdir -p /tmp/lscf
mkdir -p /tmp/lscf/mydir
echo x > /tmp/lscf/normal.txt
ls -F /tmp/lscf/mydir /tmp/lscf/normal.txt
### expect
/tmp/lscf/normal.txt

/tmp/lscf/mydir:
### end

### ls_columns_basic
# ls -C should produce multi-column output
mkdir -p /tmp/lscol
touch /tmp/lscol/alpha /tmp/lscol/beta /tmp/lscol/delta /tmp/lscol/gamma
ls -C /tmp/lscol
### expect
alpha  beta  delta  gamma
### end

### ls_columns_with_classify
# ls -CF should combine classify and columns
mkdir -p /tmp/lscf2/subdir
touch /tmp/lscf2/file.txt
ls -CF /tmp/lscf2
### expect
file.txt  subdir/
### end

### ls_one_per_line_overrides_columns
# ls -1 should override -C (one per line)
mkdir -p /tmp/ls1c
touch /tmp/ls1c/aaa /tmp/ls1c/bbb /tmp/ls1c/ccc
ls -C1 /tmp/ls1c
### expect
aaa
bbb
ccc
### end

### ls_classify_long
### bash_diff: bashkit ls -l omits 'total' line
# ls -lF should append indicators in long format
mkdir -p /tmp/lslf
mkdir -p /tmp/lslf/sub
echo x > /tmp/lslf/file.txt
ls -lF /tmp/lslf | grep -v "^total" | awk '{print $NF}'
### expect
file.txt
sub/
### end
