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

### find_mindepth
# Find with mindepth should skip entries below minimum depth
mkdir -p /tmp/mdtest/a/b
touch /tmp/mdtest/top.txt
touch /tmp/mdtest/a/mid.txt
touch /tmp/mdtest/a/b/deep.txt
find /tmp/mdtest -mindepth 1 -type f | sort
### expect
/tmp/mdtest/a/b/deep.txt
/tmp/mdtest/a/mid.txt
/tmp/mdtest/top.txt
### end

### find_mindepth_2
# Find with mindepth 2 should skip depth 0 and 1
mkdir -p /tmp/md2test/a/b
touch /tmp/md2test/top.txt
touch /tmp/md2test/a/mid.txt
touch /tmp/md2test/a/b/deep.txt
find /tmp/md2test -mindepth 2 -type f | sort
### expect
/tmp/md2test/a/b/deep.txt
/tmp/md2test/a/mid.txt
### end

### find_printf_filename
# find -printf '%f\n' should print basenames
mkdir -p /tmp/pf1
touch /tmp/pf1/alpha.txt
touch /tmp/pf1/beta.txt
find /tmp/pf1 -type f -printf '%f\n' | sort
### expect
alpha.txt
beta.txt
### end

### find_printf_path
# find -printf '%p\n' should print full paths (same as -print)
mkdir -p /tmp/pf2
touch /tmp/pf2/file.txt
find /tmp/pf2 -type f -printf '%p\n'
### expect
/tmp/pf2/file.txt
### end

### find_printf_type
# find -printf '%y' should print type chars
mkdir -p /tmp/pf3/sub
touch /tmp/pf3/sub/file.txt
find /tmp/pf3 -maxdepth 1 -printf '%y %f\n' | sort
### expect
d pf3
d sub
### end

### find_printf_size
# find -printf '%s' should print file size
mkdir -p /tmp/pf4
echo -n "hello" > /tmp/pf4/five.txt
find /tmp/pf4 -type f -printf '%f %s\n'
### expect
five.txt 5
### end

### find_printf_escapes
# find -printf should handle escape sequences
mkdir -p /tmp/pf5
touch /tmp/pf5/a.txt
find /tmp/pf5 -type f -printf '%f\t%y\n'
### expect
a.txt	f
### end

### find_multi_path_one_missing
### bash_diff: Virtual /tmp vs real /tmp may have different state
# Find with multiple paths where one doesn't exist should still output results from valid paths
mkdir -p /tmp/multi_exist
touch /tmp/multi_exist/file1.txt
touch /tmp/multi_exist/file2.txt
find /tmp/multi_exist /tmp/multi_nonexist -type f 2>/dev/null | sort
### expect
/tmp/multi_exist/file1.txt
/tmp/multi_exist/file2.txt
### end

### find_multi_path_missing_first
### bash_diff: Virtual /tmp vs real /tmp may have different state
# Find with missing first path should still output results from second path
mkdir -p /tmp/multi_second
touch /tmp/multi_second/found.txt
find /tmp/multi_missing_first /tmp/multi_second -type f 2>/dev/null
### expect
/tmp/multi_second/found.txt
### end

### find_multi_path_all_valid
### bash_diff: Virtual /tmp vs real /tmp may have different state
# Find with multiple valid paths should output results from all
mkdir -p /tmp/multi_a
mkdir -p /tmp/multi_b
touch /tmp/multi_a/a.txt
touch /tmp/multi_b/b.txt
find /tmp/multi_a /tmp/multi_b -type f | sort
### expect
/tmp/multi_a/a.txt
/tmp/multi_b/b.txt
### end

### find_missing_path_stderr
### bash_diff: Virtual /tmp vs real /tmp may have different state
# Find with missing path should output error to stderr and exit 1
find /tmp/totally_nonexistent_path 2>&1
### expect
find: '/tmp/totally_nonexistent_path': No such file or directory
### end

### find_path_predicate
# find -path should filter by path pattern
mkdir -p /tmp/fp_test/a/b
touch /tmp/fp_test/a/b/file.txt /tmp/fp_test/top.txt
find /tmp/fp_test -path '*/a/*' | sort
### expect
/tmp/fp_test/a/b
/tmp/fp_test/a/b/file.txt
### end

### find_not_name
# find -not -name should negate
mkdir -p /tmp/fn_test
touch /tmp/fn_test/keep.txt /tmp/fn_test/skip.log
find /tmp/fn_test -maxdepth 1 -type f -not -name '*.log'
### expect
/tmp/fn_test/keep.txt
### end

### find_not_path_exclude
# find -not -path should exclude paths
mkdir -p /tmp/fnp_test/.git /tmp/fnp_test/src
touch /tmp/fnp_test/src/main.rs /tmp/fnp_test/.git/config
find /tmp/fnp_test -type f -not -path '*/.git/*' | sort
### expect
/tmp/fnp_test/src/main.rs
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
