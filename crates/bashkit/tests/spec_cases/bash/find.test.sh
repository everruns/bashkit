### find_basic
### bash_diff: Virtual filesystem vs real filesystem produces different output
# Find should list current directory
find .
### expect
.
### end

### find_with_path
### bash_diff: Virtual /tmp vs real /tmp may have different state
# Find in /tmp
touch /tmp/testfile.txt
find /tmp -name "testfile.txt"
### expect
/tmp/testfile.txt
### end

### find_type_file
# Find only files
mkdir -p /tmp/findtest
touch /tmp/findtest/file.txt
mkdir /tmp/findtest/subdir
find /tmp/findtest -type f
### expect
/tmp/findtest/file.txt
### end

### find_type_directory
# Find only directories (sorted for deterministic output)
mkdir -p /tmp/findtest2
touch /tmp/findtest2/file.txt
mkdir /tmp/findtest2/subdir
find /tmp/findtest2 -type d | sort
### expect
/tmp/findtest2
/tmp/findtest2/subdir
### end

### find_deep_recursion
# Find should descend into nested directories
mkdir -p /tmp/deep/a/b/c/d
touch /tmp/deep/a/b/c/d/deep.txt
touch /tmp/deep/a/file1.txt
touch /tmp/deep/a/b/file2.txt
touch /tmp/deep/a/b/c/file3.txt
find /tmp/deep -name "*.txt" | sort
### expect
/tmp/deep/a/b/c/d/deep.txt
/tmp/deep/a/b/c/file3.txt
/tmp/deep/a/b/file2.txt
/tmp/deep/a/file1.txt
### end

### find_maxdepth
# Find with maxdepth should limit recursion depth
mkdir -p /tmp/depth/a/b/c
touch /tmp/depth/level0.txt
touch /tmp/depth/a/level1.txt
touch /tmp/depth/a/b/level2.txt
touch /tmp/depth/a/b/c/level3.txt
find /tmp/depth -maxdepth 1 -name "*.txt"
### expect
/tmp/depth/level0.txt
### end

### find_name_glob
# Find with name pattern using wildcards
mkdir -p /tmp/glob
touch /tmp/glob/test.txt
touch /tmp/glob/test.md
touch /tmp/glob/other.txt
find /tmp/glob -name "test.*" | sort
### expect
/tmp/glob/test.md
/tmp/glob/test.txt
### end

### ls_recursive
# ls -R should list nested directories
mkdir -p /tmp/lsrec/a/b
touch /tmp/lsrec/file.txt
touch /tmp/lsrec/a/nested.txt
touch /tmp/lsrec/a/b/deep.txt
ls -R /tmp/lsrec
### expect
/tmp/lsrec:
a
file.txt

/tmp/lsrec/a:
b
nested.txt

/tmp/lsrec/a/b:
deep.txt
### end
