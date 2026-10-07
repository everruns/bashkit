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

### find_not_type_consumed_before_name
# -not must apply to -type and must not leak to a later -name predicate
mkdir -p /tmp/fnt_test/dir.txt
touch /tmp/fnt_test/file.txt /tmp/fnt_test/file.md
find /tmp/fnt_test -maxdepth 1 -not -type f -name '*.txt' | sort
### expect
/tmp/fnt_test/dir.txt
### end


### find_not_type_exec_uses_negated_match_set
# -exec should receive the -not -type match set, not a later leaked -name negation
mkdir -p /tmp/fnte_test/dir.txt
touch /tmp/fnte_test/file.txt /tmp/fnte_test/file.md
find /tmp/fnte_test -maxdepth 1 -not -type f -name '*.txt' -exec echo EXEC:{} \; | sort
### expect
EXEC:/tmp/fnte_test/dir.txt
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

### find_expr_or_grouping
# ( A -o B ) groups; -a binds tighter than -o
rm -rf /tmp/fx1 && mkdir -p /tmp/fx1/s && cd /tmp/fx1
touch s/a.c s/b.h s/c.txt
find . \( -name '*.c' -o -name '*.h' \) -print | LC_ALL=C sort
echo --
find . -type f -name '*.c' -o -name '*.h' | LC_ALL=C sort
### expect
./s/a.c
./s/b.h
--
./s/a.c
./s/b.h
### end

### find_prune_idiom
rm -rf /tmp/fx2 && mkdir -p /tmp/fx2/.git/objects /tmp/fx2/src && cd /tmp/fx2
touch .git/objects/ab src/main.rs
find . -path ./.git -prune -o -type f -print
### expect
./src/main.rs
### end

### find_exec_as_predicate
# -exec ... \; is a test: its exit status gates the rest of the expression
rm -rf /tmp/fx3 && mkdir -p /tmp/fx3 && cd /tmp/fx3
echo needle > a.txt; echo hay > b.txt
find . -type f -exec grep -q needle {} \; -print
find . -name '*.txt' -exec false \; ; echo rc=$?
find . -name '*.txt' -exec false {} + ; echo rc=$?
### expect
./a.txt
rc=0
rc=1
### end

### find_exec_output_interleaves_with_print
rm -rf /tmp/fx4 && mkdir -p /tmp/fx4 && cd /tmp/fx4
touch one
find . -name one -print -exec echo ran {} \; -printf 'after %f\n'
### expect
./one
ran ./one
after one
### end

### find_execdir_runs_in_parent
rm -rf /tmp/fx5 && mkdir -p /tmp/fx5/d && cd /tmp/fx5
touch d/f
find . -name f -execdir echo {} \;
find . -name f -execdir pwd \;
### expect
./f
/tmp/fx5/d
### end

### find_size_empty_perm
rm -rf /tmp/fx6 && mkdir -p /tmp/fx6/e && cd /tmp/fx6
printf '%3000s' '' > big; : > zero; chmod 600 big; chmod 755 zero
find . -type f -size +2k
find . -empty | LC_ALL=C sort
find . -perm 600
find . -type f -perm -u+x
find . -type f -perm /o+x
### expect
./big
./e
./zero
./big
./zero
./zero
### end

### find_regex_types
rm -rf /tmp/fx7 && mkdir -p /tmp/fx7 && cd /tmp/fx7
touch x.c y.h z.txt
find . -regex '.*\.\(c\|h\)' | LC_ALL=C sort
find . -regextype posix-extended -regex '.*/[xz]\.(c|txt)' | LC_ALL=C sort
find . -iregex '.*Z\.TXT'
### expect
./x.c
./y.h
./x.c
./z.txt
./z.txt
### end

### find_delete_depth_first
rm -rf /tmp/fx8 && mkdir -p /tmp/fx8/a/b && cd /tmp/fx8
touch a/b/c a/keep.o
find a -name '*.o' -delete
find a -depth | head -1
find a -delete; echo rc=$?
ls /tmp/fx8
### expect
a/b/c
rc=0
### end

### find_delete_nonempty_dir_fails
rm -rf /tmp/fx9 && mkdir -p /tmp/fx9/d && cd /tmp/fx9
touch d/f
find . -type d -name d -delete; echo rc=$?
### expect
rc=1
### end

### find_quit_and_comma
rm -rf /tmp/fx10 && mkdir -p /tmp/fx10 && cd /tmp/fx10
touch a b
find . -type f -print -quit | wc -l
find . -name a -printf 'A\n' , -name b -printf 'B\n' | LC_ALL=C sort
### expect
1
A
B
### end

### find_printf_directives
rm -rf /tmp/fx11 && mkdir -p /tmp/fx11/d && cd /tmp/fx11
printf 'hey' > d/f
find d -printf '[%p|%P|%f|%h|%H|%d|%y]\n' | LC_ALL=C sort
find d/f -printf '%-6f|%6s|%.1f|%m %M %k\n'
find d/ -maxdepth 1 | LC_ALL=C sort
### expect
[d/f|f|f|d|d|1|f]
[d||d|.|d|0|d]
f     |     3|f|644 -rw-r--r-- 4
d/
d/f
### end

### find_symlink_types
rm -rf /tmp/fx12 && mkdir -p /tmp/fx12 && cd /tmp/fx12
touch real; ln -s real good; ln -s nowhere dangling
find . -type l | LC_ALL=C sort
find . -xtype l
find -L . -type l
find . -lname 'r*'
### expect
./dangling
./good
./dangling
./dangling
./good
### end

### find_time_tests
rm -rf /tmp/fx13 && mkdir -p /tmp/fx13 && cd /tmp/fx13
touch new; touch -d '2001-02-03 04:05:06' old
find . -type f -mtime +30
find . -type f -mmin -10
find . -type f -newer old
find . -type f -newermt '2010-01-01'
find . -type f ! -newermt '2010-01-01'
### expect
./old
./new
./new
./new
./old
### end

### find_gnu_errors
cd /tmp
find . -bogus; echo rc=$?
find . -name; echo rc=$?
find . -name x extra; echo rc=$?
find . \( -name x; echo rc=$?
find . -type q; echo rc=$?
find . -newer /nonexistent_ref; echo rc=$?
### expect
rc=1
rc=1
rc=1
rc=1
rc=1
rc=1
### end

### find_gnu_error_texts
cd /tmp
find . -bogus 2>&1
find . -name 2>&1
find . -o -name x 2>&1
find . -name x extra 2>&1
find . -type q 2>&1
find . -size 3q 2>&1
### expect
find: unknown predicate `-bogus'
find: missing argument to `-name'
find: invalid expression; you have used a binary operator '-o' with nothing before it.
find: paths must precede expression: `extra'
find: Unknown argument to -type: q
find: invalid -size type `q'
### end

### find_follow_detects_loops
### bash_diff: host locale picks ASCII vs curly quotes around names
# THREAT[TM-DOS-121]: -L symlink loops are reported, never followed forever
rm -rf /tmp/fx14 && mkdir -p /tmp/fx14/d && cd /tmp/fx14
ln -s .. d/up; touch f
find -L . -name zz 2>&1; echo rc=$?
find -L . -name f 2>/dev/null
### expect
find: File system loop detected; './d/up' is part of the same file system loop as '.'.
rc=1
./f
### end

### find_ls_layout
cd /tmp/ && rm -rf fls && mkdir -p fls/d && echo hi > fls/d/f && cd fls
find d -ls | awk '{print NF, $3, $NF}'
find d -type f -ls | awk '{print $7}'
### expect
11 drwxr-xr-x d
11 -rw-r--r-- d/f
3
### end

### find_fprint_family
# -fprint/-fprintf write to a file, truncated even when nothing matches
rm -rf /tmp/fpr && mkdir -p /tmp/fpr/d && touch /tmp/fpr/d/a && cd /tmp/fpr
echo old > out.txt
find d -type f -fprint out.txt; cat out.txt
find d -type f -fprintf out2.txt '%f|'; cat out2.txt; echo
find d -name nomatch -fprint out.txt; wc -c < out.txt
### expect
d/a
a|
0
### end
